//! Helpers shared by the unit and property tests of `core`.

use crate::core::duplicates::hash_cache::HashCache;
use crate::core::duplicates::{DuplicateGroup, find_exact_duplicates};
use crate::core::scanner::scan_directory;
use std::fs;
use std::path::{Path, PathBuf};

/// Creates an empty, unique temp directory for a test.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxide_clean_{}_{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A progress callback that ignores progress.
pub fn no_progress(_done: usize, _total: usize) {}

/// Scans `dir` and finds its duplicates, without a cache.
pub fn detect(dir: &Path) -> Vec<DuplicateGroup> {
    let files = scan_directory(dir, &no_progress).unwrap();
    find_exact_duplicates(&files, &HashCache::disabled(), &no_progress)
}

/// Stand-in for the system Trash: moves files into `<dir>/.fake_trash/`
/// (a hidden folder, so later scans of `dir` skip it). Tests never touch the
/// user's real Trash, even if a bug tried to move the wrong file.
pub fn fake_trash(dir: &Path) -> impl Fn(&Path) -> Result<(), String> + '_ {
    move |path| {
        let bin = dir.join(".fake_trash");
        fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        let name = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('/', "__");
        fs::rename(path, bin.join(name)).map_err(|e| e.to_string())
    }
}

/// xorshift64*: small, fast, and deterministic for a given seed.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }
}

/// Writes `size` bytes at `dir/path`, creating its folders.
pub fn write(dir: &Path, path: &str, size: usize) {
    let path = dir.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![0u8; size]).unwrap();
}

/// Every file under `dir` with its content, without following symlinks and
/// leaving out `skip`.
#[cfg(unix)]
pub fn snapshot(dir: &Path, skip: &[PathBuf]) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
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
pub fn random_project(rng: &mut Rng, project: &Path) -> Option<PathBuf> {
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
