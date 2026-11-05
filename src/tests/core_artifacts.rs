//! Unit tests for `src/core/artifacts.rs`, loaded there as its `tests` module.

use super::*;
use crate::core::scanner::scan_directory;
use crate::core::test_support::{Rng, no_progress, temp_dir};

const DAY: Duration = Duration::from_secs(86_400);
const THRESHOLD: Duration = Duration::from_secs(90 * 86_400);

/// Writes `size` bytes at `dir/path`, creating its folders.
fn write(dir: &Path, path: &str, size: usize) {
    let path = dir.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![0u8; size]).unwrap();
}

/// Scans `dir` with every file made to look untouched for 400 days. No tool
/// can set a ctime back, so the dates are changed after the scan.
fn old_files(dir: &Path) -> Vec<FileInfo> {
    let long_ago = SystemTime::now() - DAY * 400;
    let mut files = scan_directory(dir, &no_progress).unwrap();
    for file in &mut files {
        file.modified = Some(long_ago);
        file.accessed = Some(long_ago);
        file.changed = Some(long_ago);
    }
    files
}

fn find(dir: &Path, files: &[FileInfo]) -> Artifacts {
    find_artifacts(
        files,
        dir,
        &[],
        SystemTime::now(),
        THRESHOLD,
        false,
        &no_progress,
    )
}

/// Like `find`, with projects in use listed too.
fn find_every_project(dir: &Path, files: &[FileInfo]) -> Artifacts {
    find_artifacts(
        files,
        dir,
        &[],
        SystemTime::now(),
        THRESHOLD,
        true,
        &no_progress,
    )
}

/// Builds a folder with `build`, finds its artifacts and cleans up.
fn found_in(name: &str, build: impl Fn(&Path)) -> Artifacts {
    let dir = temp_dir(name);
    build(&dir);
    let found = find(&dir, &old_files(&dir));
    fs::remove_dir_all(&dir).unwrap();
    found
}

fn rust_project(dir: &Path) {
    write(dir, "Cargo.toml", 10);
    write(dir, "src/main.rs", 10);
    write(dir, "target/CACHEDIR.TAG", 10);
    write(dir, "target/debug/app", 1000);
}

#[test]
fn test_rust_project_needs_a_manifest_and_cargos_tag() {
    let found = found_in("art_rust", |dir| {
        rust_project(dir);
        write(dir, "Cargo.lock", 10);
    });
    assert_eq!(found.found.len(), 1);
    let artifact = &found.found[0];
    assert_eq!(artifact.kind, ArtifactKind::Rust);
    assert_eq!(artifact.rebuild, Some("cargo build"));
    assert_eq!(artifact.confidence, Confidence::Pinned);
    assert_eq!(artifact.size.bytes, 1010);
    assert_eq!(artifact.state, ProjectState::Forgotten);
    assert_eq!(artifact.untouched.unwrap().as_secs() / 86_400, 400);

    let no_lock = found_in("art_rust_nolock", rust_project);
    assert_eq!(no_lock.found[0].confidence, Confidence::NoLockfile);

    let no_tag = found_in("art_rust_notag", |dir| {
        write(dir, "Cargo.toml", 10);
        write(dir, "target/important.txt", 1000);
    });
    assert!(
        no_tag.found.is_empty(),
        "a target without the tag isn't Cargo's"
    );
    assert_eq!(no_tag.unverified, 1, "and the screen says so");

    let no_manifest = found_in("art_rust_nomanifest", |dir| {
        write(dir, "notes.txt", 10);
        write(dir, "target/CACHEDIR.TAG", 10);
    });
    assert!(no_manifest.found.is_empty());
    assert_eq!(no_manifest.unverified, 0);
}

#[test]
fn test_node_rebuild_command_follows_the_lockfile() {
    for (lockfile, command) in [
        ("package-lock.json", "npm ci"),
        ("yarn.lock", "yarn install"),
        ("pnpm-lock.yaml", "pnpm install"),
    ] {
        let found = found_in("art_node", |dir| {
            write(dir, "package.json", 10);
            write(dir, lockfile, 10);
            write(dir, "node_modules/pkg/index.js", 100);
        });
        assert_eq!(found.found[0].rebuild, Some(command), "{}", lockfile);
        assert_eq!(found.found[0].confidence, Confidence::Pinned);
    }

    let no_lock = found_in("art_node_nolock", |dir| {
        write(dir, "package.json", 10);
        write(dir, "node_modules/pkg/index.js", 100);
    });
    assert_eq!(no_lock.found[0].rebuild, Some("npm install"));
    assert_eq!(no_lock.found[0].confidence, Confidence::NoLockfile);

    let no_manifest = found_in("art_node_nomanifest", |dir| {
        write(dir, "notes.txt", 10);
        write(dir, "node_modules/pkg/index.js", 100);
    });
    assert!(no_manifest.found.is_empty());
}

#[test]
fn test_python_environment_is_never_rated_as_pinned() {
    let with_list = found_in("art_py_list", |dir| {
        write(dir, "requirements.txt", 10);
        write(dir, ".venv/pyvenv.cfg", 10);
    });
    assert_eq!(with_list.found[0].kind, ArtifactKind::Python);
    assert_eq!(
        with_list.found[0].rebuild,
        Some("pip install -r requirements.txt")
    );
    assert_eq!(with_list.found[0].confidence, Confidence::PythonList);

    let no_record = found_in("art_py_norecord", |dir| {
        write(dir, "app.py", 10);
        write(dir, "venv/pyvenv.cfg", 10);
    });
    assert_eq!(no_record.found[0].rebuild, None);
    assert_eq!(no_record.found[0].confidence, Confidence::NoRecord);

    let not_a_venv = found_in("art_py_notvenv", |dir| {
        write(dir, "requirements.txt", 10);
        write(dir, "venv/my_notes.txt", 10);
    });
    assert!(not_a_venv.found.is_empty(), "no pyvenv.cfg: just a folder");
}

#[test]
fn test_any_active_file_protects_the_project() {
    let dir = temp_dir("art_active");
    rust_project(&dir);
    write(&dir, "src/deep/er/edited.rs", 10);
    let mut files = old_files(&dir);
    // Edited yesterday, deep inside the project
    let edited = files
        .iter_mut()
        .find(|f| f.path.ends_with("edited.rs"))
        .unwrap();
    edited.modified = Some(SystemTime::now() - DAY);
    let found = find(&dir, &files);

    // A file that can't be judged also keeps the project off the list
    let mut files = old_files(&dir);
    files[0].modified = None;
    let no_date = find(&dir, &files);
    fs::remove_dir_all(&dir).unwrap();

    assert!(found.found.is_empty());
    assert_eq!(found.active, 1);
    assert!(no_date.found.is_empty());
    assert_eq!(no_date.not_judged, 1);
}

#[test]
fn test_each_nested_project_is_judged_on_its_own_files() {
    let dir = temp_dir("art_nested");
    write(&dir, "package.json", 10);
    write(&dir, "node_modules/a/index.js", 100);
    write(&dir, "client/package.json", 10);
    write(&dir, "client/node_modules/b/index.js", 100);
    write(&dir, "server/app.js", 10);
    let mut files = old_files(&dir);
    files
        .iter_mut()
        .find(|f| f.path.ends_with("app.js"))
        .unwrap()
        .modified = Some(SystemTime::now() - DAY);
    let found = find(&dir, &files);
    fs::remove_dir_all(&dir).unwrap();

    // The outer project is active through server/; client/ is not
    assert_eq!(found.found.len(), 1);
    assert_eq!(found.found[0].project, dir.join("client"));
    assert_eq!(found.active, 1);
}

#[test]
fn test_found_are_sorted_and_add_up() {
    let found = found_in("art_sorted", |dir| {
        write(dir, "small/package.json", 10);
        write(dir, "small/node_modules/a", 100);
        write(dir, "big/package.json", 10);
        write(dir, "big/node_modules/a", 900);
    });
    let sizes: Vec<u64> = found.found.iter().map(|a| a.size.bytes).collect();
    assert_eq!(sizes, vec![900, 100]);
    assert_eq!(
        found.total,
        Usage {
            bytes: 1000,
            files: 2
        }
    );
}

#[test]
fn test_excluded_artifacts_are_left_out() {
    let dir = temp_dir("art_excluded");
    write(&dir, "package.json", 10);
    write(&dir, "node_modules/a", 100);
    let excluded = [dir.join("node_modules")];
    let found = find_artifacts(
        &old_files(&dir),
        &dir,
        &excluded,
        SystemTime::now(),
        THRESHOLD,
        true,
        &no_progress,
    );
    fs::remove_dir_all(&dir).unwrap();
    assert!(found.found.is_empty(), "not even with every project");
}

#[cfg(unix)]
#[test]
fn test_symlinks_are_neither_artifacts_nor_followed() {
    use std::os::unix::fs::symlink;

    let outside = temp_dir("art_outside");
    write(&outside, "huge.bin", 5000);

    // node_modules itself is a symlink to somewhere else
    let linked = found_in("art_link", |dir| {
        write(dir, "package.json", 10);
        symlink(&outside, dir.join("node_modules")).unwrap();
    });
    // A symlink inside it isn't counted, and hardlinks count once
    let inside = found_in("art_link_inside", |dir| {
        write(dir, "package.json", 10);
        write(dir, "node_modules/a.bin", 100);
        fs::hard_link(
            dir.join("node_modules/a.bin"),
            dir.join("node_modules/b.bin"),
        )
        .unwrap();
        symlink(&outside, dir.join("node_modules/link")).unwrap();
    });
    fs::remove_dir_all(&outside).unwrap();

    assert!(linked.found.is_empty());
    assert_eq!(
        inside.found[0].size,
        Usage {
            bytes: 100,
            files: 1
        }
    );
    assert_eq!(inside.found[0].unreadable, 0);
}

#[test]
fn test_every_project_lists_active_ones_marked_as_such() {
    let dir = temp_dir("art_every");
    write(&dir, "active/package.json", 10);
    write(&dir, "active/node_modules/a", 100);
    write(&dir, "active/src/edited.js", 10);
    write(&dir, "old/package.json", 10);
    write(&dir, "old/node_modules/a", 100);
    write(&dir, "unknown/package.json", 10);
    write(&dir, "unknown/node_modules/a", 100);
    write(&dir, "fake/Cargo.toml", 10);
    write(&dir, "fake/target/important.txt", 10);
    let mut files = old_files(&dir);
    for file in &mut files {
        if file.path.ends_with("edited.js") {
            file.modified = Some(SystemTime::now() - DAY * 2);
        } else if file.path.starts_with(dir.join("unknown")) {
            file.modified = None;
        }
    }
    let every = find_every_project(&dir, &files);
    let default = find(&dir, &files);
    fs::remove_dir_all(&dir).unwrap();

    let state_of = |project: &str| {
        let artifact = every
            .found
            .iter()
            .find(|a| a.project.ends_with(project))
            .unwrap();
        (
            artifact.state,
            artifact.untouched.map(|d| d.as_secs() / 86_400),
        )
    };
    assert_eq!(every.found.len(), 3);
    assert_eq!(state_of("active"), (ProjectState::Active, Some(2)));
    assert_eq!(state_of("old"), (ProjectState::Forgotten, Some(400)));
    assert_eq!(state_of("unknown").0, ProjectState::NotJudged);
    assert_eq!(every.unverified, 1, "an unproven target is never listed");
    assert_eq!((every.active, every.not_judged), (0, 0));

    assert_eq!(
        default.found.len(),
        1,
        "by default, only forgotten projects"
    );
    assert_eq!((default.active, default.not_judged), (1, 1));
}

/// Every file under `dir` with its content, without following symlinks and
/// leaving out `skip`.
#[cfg(unix)]
fn snapshot(dir: &Path, skip: &[PathBuf]) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).unwrap().flatten() {
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            if skip.contains(&path) {
                continue;
            }
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                files.insert(path.clone(), fs::read(&path).unwrap());
            }
        }
    }
    files
}

/// Fills `project` with a random project or look-alike, returning the build
/// folder it holds, if any.
#[cfg(unix)]
fn random_project(rng: &mut Rng, project: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::symlink;

    let source = rng.bytes(200);
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/main.txt"), source).unwrap();
    write(project, "README.md", 20);
    write(project, ".git/HEAD", 20);
    write(project, "build/user_data.txt", 20);

    // What a build folder holds, with a link back into the source
    let fill = |artifact: &Path| {
        write(artifact, "deep/er/generated.bin", 300);
        symlink(project.join("src/main.txt"), artifact.join("link")).unwrap();
    };
    match rng.below(7) {
        0 => {
            write(project, "Cargo.toml", 20);
            if rng.below(2) == 0 {
                write(project, "Cargo.lock", 20);
            }
            write(project, "target/CACHEDIR.TAG", 20);
            fill(&project.join("target"));
            Some(project.join("target"))
        }
        1 => {
            write(project, "package.json", 20);
            if rng.below(2) == 0 {
                write(project, "package-lock.json", 20);
            }
            fs::create_dir(project.join("node_modules")).unwrap();
            fill(&project.join("node_modules"));
            Some(project.join("node_modules"))
        }
        2 => {
            write(project, "requirements.txt", 20);
            write(project, ".venv/pyvenv.cfg", 20);
            fill(&project.join(".venv"));
            Some(project.join(".venv"))
        }
        // Look-alikes: they must survive
        3 => {
            write(project, "Cargo.toml", 20);
            write(project, "target/thesis.pdf", 20);
            None
        }
        4 => {
            write(project, "target/CACHEDIR.TAG", 20);
            write(project, "target/thesis.pdf", 20);
            None
        }
        5 => {
            write(project, "node_modules/thesis.pdf", 20);
            None
        }
        _ => {
            write(project, "requirements.txt", 20);
            write(project, "venv/thesis.pdf", 20);
            None
        }
    }
}

/// The promise behind any future cleanup, whatever the project: what gets
/// listed is exactly the build folders, and removing all of them leaves every
/// other file untouched, byte for byte.
#[cfg(unix)]
#[test]
fn prop_removing_what_is_listed_only_removes_build_folders() {
    for seed in 1..=60 {
        let mut rng = Rng::new(seed);
        let dir = temp_dir(&format!("art_prop_{}", seed));
        let mut expected = Vec::new();
        for i in 0..1 + rng.below(5) {
            let project = dir.join(format!("p{}", i));
            expected.extend(random_project(&mut rng, &project));
            // Sometimes a project inside another
            if rng.below(3) == 0 {
                expected.extend(random_project(&mut rng, &project.join("client")));
            }
        }
        let before = snapshot(&dir, &expected);

        let files = scan_directory(&dir, &no_progress).unwrap();
        let report = find_every_project(&dir, &files);
        let mut listed: Vec<PathBuf> = report.found.iter().map(|a| a.path.clone()).collect();
        listed.sort();
        expected.sort();
        assert_eq!(listed, expected, "seed {}: wrong folders listed", seed);

        for path in &listed {
            fs::remove_dir_all(path).unwrap();
        }
        let after = snapshot(&dir, &[]);
        fs::remove_dir_all(&dir).unwrap();
        assert!(before == after, "seed {}: something else changed", seed);
    }
}
