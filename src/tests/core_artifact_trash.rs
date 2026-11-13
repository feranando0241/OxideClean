//! Unit tests for `src/core/artifact_trash.rs`, loaded there as its `tests`
//! module. No tool can set a ctime back, so these tests judge real files from
//! a `now` far in the future, and never touch the user's Trash.

use super::*;
use crate::core::artifacts::{ArtifactKind, Artifacts, Confidence, find_artifacts};
use crate::core::test_support::{fake_trash, no_progress, temp_dir, write};
use std::fs::{self, File};
use std::path::PathBuf;

const DAY: Duration = Duration::from_secs(86_400);
const THRESHOLD: Duration = Duration::from_secs(90 * 86_400);

/// A moment when everything created by a test looks long forgotten.
fn later() -> SystemTime {
    SystemTime::now() + DAY * 400
}

fn list(dir: &Path, excluded: &[PathBuf], every_project: bool) -> Artifacts {
    let files = scan_directory(dir, &no_progress).unwrap();
    find_artifacts(
        &files,
        dir,
        excluded,
        later(),
        THRESHOLD,
        every_project,
        &no_progress,
    )
}

fn remove(dir: &Path, selected: &[Artifact]) -> ArtifactTrashReport {
    remove_artifacts(selected, later(), THRESHOLD, &no_progress, &fake_trash(dir))
}

fn set_modified(path: &Path, when: SystemTime) {
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

fn node_project(dir: &Path) {
    write(dir, "package.json", 10);
    write(dir, "package-lock.json", 10);
    write(dir, "src/app.js", 10);
    write(dir, "node_modules/pkg/index.js", 100);
}

#[test]
fn test_moves_only_the_build_folder() {
    let dir = temp_dir("at_moves");
    node_project(&dir);
    let found = list(&dir, &[], false);
    let report = remove(&dir, &found.found);
    let build_gone = !dir.join("node_modules").exists();
    let in_trash = dir.join(".fake_trash/node_modules/pkg/index.js").exists();
    let rest_kept = ["package.json", "package-lock.json", "src/app.js"]
        .iter()
        .all(|f| dir.join(f).exists());
    fs::remove_dir_all(&dir).unwrap();

    assert_eq!((report.moved, report.moved_bytes), (1, 100));
    assert!(report.skipped.is_empty());
    assert!(build_gone && in_trash && rest_kept);
}

#[test]
fn test_what_cant_be_rebuilt_reliably_is_refused_even_if_selected() {
    let dir = temp_dir("at_refused");
    write(&dir, "py/requirements.txt", 10);
    write(&dir, "py/.venv/pyvenv.cfg", 10);
    write(&dir, "nolock/package.json", 10);
    write(&dir, "nolock/node_modules/a", 10);
    let found = list(&dir, &[], false);
    let report = remove(&dir, &found.found);
    let kept = dir.join("py/.venv").exists() && dir.join("nolock/node_modules").exists();
    fs::remove_dir_all(&dir).unwrap();

    assert_eq!(found.found.len(), 2);
    assert_eq!(report.moved, 0);
    assert!(
        report
            .skipped
            .iter()
            .all(|(_, skip)| *skip == ArtifactSkip::NotMovable)
    );
    assert!(kept);
}

#[test]
fn test_project_used_after_the_list_is_left_alone() {
    let dir = temp_dir("at_used");
    node_project(&dir);
    let found = list(&dir, &[], false);
    assert_eq!(found.found[0].state, ProjectState::Forgotten);
    set_modified(&dir.join("src/app.js"), later() - DAY);
    let report = remove(&dir, &found.found);
    let kept = dir.join("node_modules").exists();
    fs::remove_dir_all(&dir).unwrap();

    assert_eq!(report.moved, 0);
    assert_eq!(report.skipped[0].1, ArtifactSkip::ProjectInUse);
    assert!(kept);
}

#[test]
fn test_excluded_subfolder_still_counts_as_activity_before_moving() {
    let dir = temp_dir("at_excluded");
    node_project(&dir);
    write(&dir, "work/current.js", 10);
    set_modified(&dir.join("work/current.js"), later() - DAY);
    // The list can't see work/, so the project looks forgotten
    let excluded_files: Vec<_> = scan_directory(&dir, &no_progress)
        .unwrap()
        .into_iter()
        .filter(|f| !f.path.starts_with(dir.join("work")))
        .collect();
    let found = find_artifacts(
        &excluded_files,
        &dir,
        &[],
        later(),
        THRESHOLD,
        false,
        &no_progress,
    );
    assert_eq!(found.found.len(), 1);
    let report = remove(&dir, &found.found);
    let kept = dir.join("node_modules").exists();
    fs::remove_dir_all(&dir).unwrap();

    assert_eq!(report.skipped[0].1, ArtifactSkip::ProjectInUse);
    assert!(kept);
}

#[test]
fn test_build_folder_changed_moments_ago_is_left_alone() {
    let dir = temp_dir("at_recent");
    node_project(&dir);
    let found = list(&dir, &[], false);
    let inside = dir.join("node_modules/pkg/index.js");

    // A build writing now, inside a project otherwise forgotten
    set_modified(&inside, later() - Duration::from_secs(60));
    let recent = remove(&dir, &found.found);
    // A timestamp in the future is no proof that it's quiet
    set_modified(&inside, later() + DAY);
    let future = remove(&dir, &found.found);
    // Just past the limit it can go
    set_modified(&inside, later() - Duration::from_secs(11 * 60));
    let quiet = remove(&dir, &found.found);
    fs::remove_dir_all(&dir).unwrap();

    assert_eq!(recent.skipped[0].1, ArtifactSkip::RecentlyChanged);
    assert_eq!(future.skipped[0].1, ArtifactSkip::RecentlyChanged);
    assert_eq!(quiet.moved, 1);
}

#[test]
fn test_anything_that_stops_being_a_build_folder_is_left_alone() {
    for (name, change) in [
        ("manifest", "package.json"),
        ("lockfile", "package-lock.json"),
    ] {
        let dir = temp_dir(&format!("at_gone_{}", name));
        node_project(&dir);
        let found = list(&dir, &[], false);
        fs::remove_file(dir.join(change)).unwrap();
        let report = remove(&dir, &found.found);
        let kept = dir.join("node_modules").exists();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            report.skipped[0].1,
            ArtifactSkip::NoLongerABuildFolder,
            "{}",
            name
        );
        assert!(kept, "{}", name);
    }

    let dir = temp_dir("at_gone_tag");
    write(&dir, "Cargo.toml", 10);
    write(&dir, "Cargo.lock", 10);
    write(&dir, "target/CACHEDIR.TAG", 10);
    write(&dir, "target/debug/app", 10);
    let found = list(&dir, &[], false);
    fs::remove_file(dir.join("target/CACHEDIR.TAG")).unwrap();
    let report = remove(&dir, &found.found);
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(report.skipped[0].1, ArtifactSkip::NoLongerABuildFolder);
}

#[cfg(unix)]
#[test]
fn test_folder_replaced_by_a_symlink_is_left_alone() {
    let dir = temp_dir("at_swapped");
    let elsewhere = temp_dir("at_swapped_elsewhere");
    write(&elsewhere, "precious.txt", 10);
    node_project(&dir);
    let found = list(&dir, &[], false);
    fs::remove_dir_all(dir.join("node_modules")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, dir.join("node_modules")).unwrap();
    let report = remove(&dir, &found.found);
    let precious_kept = elsewhere.join("precious.txt").exists();
    fs::remove_dir_all(&dir).unwrap();
    fs::remove_dir_all(&elsewhere).unwrap();

    assert_eq!(report.skipped[0].1, ArtifactSkip::NoLongerABuildFolder);
    assert!(precious_kept);
}

#[test]
fn test_active_project_chosen_knowingly_can_be_moved() {
    let dir = temp_dir("at_active");
    node_project(&dir);
    set_modified(&dir.join("src/app.js"), later() - DAY);
    let found = list(&dir, &[], true);
    assert_eq!(found.found[0].state, ProjectState::Active);
    let report = remove(&dir, &found.found);
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(report.moved, 1);
}

#[test]
fn test_a_failed_move_is_reported_and_the_rest_go_on() {
    let dir = temp_dir("at_failed");
    node_project(&dir.join("a"));
    node_project(&dir.join("b"));
    let found = list(&dir, &[], false);
    let report = remove_artifacts(&found.found, later(), THRESHOLD, &no_progress, &|path| {
        if path.starts_with(dir.join("a")) {
            Err("permission denied".to_string())
        } else {
            fake_trash(&dir)(path)
        }
    });
    fs::remove_dir_all(&dir).unwrap();

    assert_eq!(report.moved, 1);
    assert_eq!(
        report.skipped[0].1,
        ArtifactSkip::CouldNotMove("permission denied".to_string())
    );
}

#[test]
fn test_only_the_clear_cut_case_moves_without_being_chosen() {
    let artifact = |kind, state, confidence, built_programs| Artifact {
        project: PathBuf::from("p"),
        path: PathBuf::from("p/x"),
        kind,
        size: Default::default(),
        unreadable: 0,
        state,
        untouched: None,
        rebuild: None,
        confidence,
        built_programs,
    };
    use ArtifactKind::{Node, Python, Rust};
    use Confidence::{NoLockfile, Pinned, PythonList};
    use ProjectState::{Active, Forgotten, NotJudged};

    assert!(artifact(Node, Forgotten, Pinned, 0).moves_unattended());
    assert!(artifact(Rust, Forgotten, Pinned, 0).moves_unattended());
    assert!(!artifact(Node, Active, Pinned, 0).moves_unattended());
    assert!(!artifact(Node, NotJudged, Pinned, 0).moves_unattended());
    assert!(!artifact(Rust, Forgotten, Pinned, 1).moves_unattended());
    assert!(!artifact(Node, Forgotten, NoLockfile, 0).moves_unattended());
    assert!(!artifact(Python, Forgotten, PythonList, 0).moves_unattended());

    assert!(artifact(Node, Active, Pinned, 0).can_move());
    assert!(artifact(Rust, Forgotten, Pinned, 1).can_move());
    assert!(!artifact(Node, Forgotten, NoLockfile, 0).can_move());
    assert!(!artifact(Python, Forgotten, PythonList, 0).can_move());
}

/// The promise of `--trash`, on the real code path except for the Trash
/// itself: whatever the projects, only movable build folders leave, and every
/// other file stays identical byte for byte.
#[cfg(unix)]
#[test]
fn prop_moving_everything_movable_leaves_everything_else_intact() {
    use crate::core::test_support::{Rng, random_project, snapshot};

    for seed in 1..=60 {
        let mut rng = Rng::new(seed);
        let dir = temp_dir(&format!("at_prop_{}", seed));
        for i in 0..1 + rng.below(5) {
            let project = dir.join(format!("p{}", i));
            random_project(&mut rng, &project);
            if rng.below(3) == 0 {
                random_project(&mut rng, &project.join("client"));
            }
        }
        let found = list(&dir, &[], true);
        let movable: Vec<PathBuf> = found
            .found
            .iter()
            .filter(|a| a.can_move())
            .map(|a| a.path.clone())
            .collect();
        let before = snapshot(&dir, &movable);

        let report = remove(&dir, &found.found);

        let after = snapshot(&dir, &[dir.join(".fake_trash")]);
        let all_gone = movable.iter().all(|p| !p.exists());
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(report.moved, movable.len(), "seed {}", seed);
        assert!(all_gone, "seed {}", seed);
        assert!(before == after, "seed {}: something else changed", seed);
    }
}
