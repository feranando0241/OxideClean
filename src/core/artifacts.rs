//! Build artifacts in forgotten projects: `target/`, `node_modules/` and
//! Python virtual environments. Only finds and measures them; nothing here
//! moves a file. The rules and why: docs/adr/0005-build-artifacts.md.

use crate::core::Progress;
use crate::core::files::Usage;
use crate::core::models::{FileId, FileInfo};
use crate::core::old_files::{Verdict, judge, untouched_for};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Days without activity after which a project's artifacts are listed.
pub const DEFAULT_ARTIFACT_DAYS: u64 = 90;

/// Node lockfiles and the command that reinstalls from each, by preference.
const NODE_LOCKFILES: &[(&str, &str)] = &[
    ("package-lock.json", "npm ci"),
    ("npm-shrinkwrap.json", "npm ci"),
    ("yarn.lock", "yarn install"),
    ("pnpm-lock.yaml", "pnpm install"),
    ("bun.lock", "bun install"),
    ("bun.lockb", "bun install"),
];

/// What kind of project an artifact belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Rust,
    Node,
    Python,
}

/// How much to trust that an artifact can be rebuilt as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// A lockfile records the exact versions.
    Pinned,
    /// Rust or Node without a lockfile: versions may come out different.
    NoLockfile,
    /// A Python environment with a package list, which rarely pins versions.
    PythonList,
    /// A Python environment with no record of its packages.
    NoRecord,
}

/// Whether the project of an artifact is in use, judged at the threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectState {
    Forgotten,
    /// A file was used, or arrived on disk, recently.
    Active,
    /// A file has no date or a future one, so the project can't be judged.
    NotJudged,
}

/// A build folder found in a project.
#[derive(Debug, Clone)]
pub struct Artifact {
    /// The folder holding the project (and the artifact).
    pub project: PathBuf,
    pub path: PathBuf,
    pub kind: ArtifactKind,
    pub size: Usage,
    /// Entries that couldn't be read, so `size` may fall short.
    pub unreadable: usize,
    pub state: ProjectState,
    /// How long the whole project has been untouched, as far as its files tell.
    pub untouched: Option<Duration>,
    /// The command that rebuilds it, if anything records what to install.
    pub rebuild: Option<&'static str>,
    pub confidence: Confidence,
    /// Programs in `target/release`, which `cargo build` doesn't bring back.
    pub built_programs: usize,
}

impl Artifact {
    /// True if it may be moved at all: Rust or Node with a lockfile.
    pub fn can_move(&self) -> bool {
        self.kind != ArtifactKind::Python && self.confidence == Confidence::Pinned
    }

    /// True for the clear-cut case that may move without being chosen one by
    /// one: a forgotten project, with nothing that a plain rebuild won't restore.
    pub fn moves_unattended(&self) -> bool {
        self.can_move() && self.state == ProjectState::Forgotten && self.built_programs == 0
    }
}

/// What `find_artifacts` found, including what it left alone and why.
#[derive(Debug, Clone, Default)]
pub struct Artifacts {
    /// Biggest first, then by path.
    pub found: Vec<Artifact>,
    /// Space and count of `found`.
    pub total: Usage,
    /// Artifacts not listed because their project is in use or just arrived.
    pub active: usize,
    /// Artifacts not listed because a project file couldn't be judged.
    pub not_judged: usize,
    /// `target/` folders beside a `Cargo.toml` without Cargo's `CACHEDIR.TAG`.
    pub unverified: usize,
}

/// A build folder recognized in some project, not yet judged or measured.
pub(crate) struct Candidate {
    pub(crate) path: PathBuf,
    pub(crate) kind: ArtifactKind,
    rebuild: Option<&'static str>,
    pub(crate) confidence: Confidence,
    pub(crate) verified: bool,
}

/// True for a real folder: a symlink never counts as an artifact.
fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// The build folders in `dir`, as far as the closed list of ADR-0005 goes.
pub(crate) fn candidates_in(dir: &Path) -> Vec<Candidate> {
    let mut found = Vec::new();

    let target = dir.join("target");
    if dir.join("Cargo.toml").is_file() && is_real_dir(&target) {
        found.push(Candidate {
            verified: target.join("CACHEDIR.TAG").is_file(),
            path: target,
            kind: ArtifactKind::Rust,
            rebuild: Some("cargo build"),
            confidence: if dir.join("Cargo.lock").is_file() {
                Confidence::Pinned
            } else {
                Confidence::NoLockfile
            },
        });
    }

    let node_modules = dir.join("node_modules");
    if dir.join("package.json").is_file() && is_real_dir(&node_modules) {
        let lockfile = NODE_LOCKFILES
            .iter()
            .find(|(name, _)| dir.join(name).is_file());
        found.push(Candidate {
            path: node_modules,
            kind: ArtifactKind::Node,
            rebuild: Some(lockfile.map_or("npm install", |(_, command)| command)),
            confidence: if lockfile.is_some() {
                Confidence::Pinned
            } else {
                Confidence::NoLockfile
            },
            verified: true,
        });
    }

    for name in [".venv", "venv"] {
        let venv = dir.join(name);
        if !is_real_dir(&venv) || !venv.join("pyvenv.cfg").is_file() {
            continue;
        }
        let rebuild = if dir.join("requirements.txt").is_file() {
            Some("pip install -r requirements.txt")
        } else if dir.join("pyproject.toml").is_file() {
            Some("pip install .")
        } else {
            None
        };
        found.push(Candidate {
            path: venv,
            kind: ArtifactKind::Python,
            rebuild,
            confidence: if rebuild.is_some() {
                Confidence::PythonList
            } else {
                Confidence::NoRecord
            },
            verified: true,
        });
    }
    found
}

/// What `measure` learned about a folder.
pub(crate) struct Measured {
    pub(crate) size: Usage,
    /// Entries that couldn't be read.
    unreadable: usize,
    /// The most recent modification of a file inside.
    pub(crate) newest: Option<SystemTime>,
}

/// Adds up what's inside `dir` without following symlinks, counting hardlinks
/// once.
pub(crate) fn measure(dir: &Path) -> Measured {
    let mut size = Usage::default();
    let mut unreadable = 0;
    let mut newest: Option<SystemTime> = None;
    let mut seen = HashSet::new();
    let mut pending = vec![dir.to_path_buf()];

    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            unreadable += 1;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                unreadable += 1;
                continue;
            };
            let path = entry.path();
            match (entry.file_type(), entry.metadata()) {
                (Ok(kind), _) if kind.is_dir() => pending.push(path),
                (Ok(kind), Ok(metadata)) if kind.is_file() => {
                    if let Ok(modified) = metadata.modified() {
                        newest = Some(newest.map_or(modified, |n| n.max(modified)));
                    }
                    if FileId::of(&path).is_none_or(|id| seen.insert(id)) {
                        size.bytes += metadata.len();
                        size.files += 1;
                    }
                }
                // A symlink (not followed) or something else that holds no data
                (Ok(kind), _) if !kind.is_file() => {}
                _ => unreadable += 1,
            }
        }
    }
    Measured {
        size,
        unreadable,
        newest,
    }
}

/// How many programs a Cargo build folder holds in `release/`, the only
/// copy of them that `cargo build` won't bring back.
fn built_programs(target: &Path) -> usize {
    fs::read_dir(target.join("release")).map_or(0, |entries| {
        entries
            .flatten()
            .filter(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_file())
                    && entry.metadata().is_ok_and(|m| is_program(&m))
            })
            .count()
    })
}

#[cfg(unix)]
fn is_program(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_program(_metadata: &fs::Metadata) -> bool {
    true
}

/// True if Git shows use within `threshold`: commits, checkouts and pulls can
/// happen without a scanned file changing. The repository may be above `dir`.
fn git_used_recently(dir: &Path, now: SystemTime, threshold: Duration) -> bool {
    let Some(git) = dir
        .ancestors()
        .map(|folder| folder.join(".git"))
        .find(|git| git.is_dir())
    else {
        return false;
    };
    ["index", "HEAD", "FETCH_HEAD", "ORIG_HEAD"]
        .iter()
        .any(|name| {
            fs::metadata(git.join(name))
                .and_then(|m| m.modified())
                .is_ok_and(|modified| {
                    now.duration_since(modified)
                        .map_or(true, |age| age <= threshold)
                })
        })
}

/// How the files under one folder were judged.
#[derive(Default)]
pub(crate) struct Activity {
    in_use: bool,
    not_judged: bool,
    /// Shortest time untouched among its files.
    untouched: Option<Duration>,
}

impl Activity {
    fn record(&mut self, verdict: Verdict, untouched: Option<Duration>) {
        match verdict {
            Verdict::Forgotten(_) => {}
            Verdict::InUse | Verdict::JustArrived => self.in_use = true,
            Verdict::NoDate | Verdict::FutureDate => self.not_judged = true,
        }
        if let Some(age) = untouched {
            self.untouched = Some(self.untouched.map_or(age, |shortest| shortest.min(age)));
        }
    }
}

/// Each folder up to `root` with how the scanned files under it were judged.
pub(crate) fn activity_by_folder<'a>(
    files: &'a [FileInfo],
    root: &Path,
    now: SystemTime,
    threshold: Duration,
) -> BTreeMap<&'a Path, Activity> {
    let mut folders: BTreeMap<&Path, Activity> = BTreeMap::new();
    for file in files {
        let verdict = judge(file, now, threshold);
        let untouched = untouched_for(file, now);
        for dir in file
            .path
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(root))
        {
            folders.entry(dir).or_default().record(verdict, untouched);
        }
    }
    folders
}

/// Whether the project in `dir` is in use, from its files and its Git activity.
pub(crate) fn project_state(
    dir: &Path,
    activity: &Activity,
    now: SystemTime,
    threshold: Duration,
) -> ProjectState {
    if activity.in_use || git_used_recently(dir, now, threshold) {
        ProjectState::Active
    } else if activity.not_judged {
        ProjectState::NotJudged
    } else {
        ProjectState::Forgotten
    }
}

/// Finds the build folders of projects untouched for longer than `threshold`
/// among the scanned `files`, leaving out anything inside `excluded`. With
/// `every_project`, projects in use are listed too, marked by their state.
pub fn find_artifacts(
    files: &[FileInfo],
    root: &Path,
    excluded: &[PathBuf],
    now: SystemTime,
    threshold: Duration,
    every_project: bool,
    on_progress: Progress,
) -> Artifacts {
    let folders = activity_by_folder(files, root, now, threshold);

    let mut report = Artifacts::default();
    let mut listed = Vec::new();
    for (dir, activity) in &folders {
        for candidate in candidates_in(dir) {
            if excluded.iter().any(|e| candidate.path.starts_with(e)) {
                continue;
            }
            let state = project_state(dir, activity, now, threshold);
            match state {
                ProjectState::Active if !every_project => report.active += 1,
                ProjectState::NotJudged if !every_project => report.not_judged += 1,
                _ if !candidate.verified => report.unverified += 1,
                _ => listed.push((dir.to_path_buf(), candidate, state, activity.untouched)),
            }
        }
    }

    on_progress(0, listed.len());
    let total = listed.len();
    for (done, (project, candidate, state, untouched)) in listed.into_iter().enumerate() {
        let measured = measure(&candidate.path);
        let size = measured.size;
        let programs = if candidate.kind == ArtifactKind::Rust {
            built_programs(&candidate.path)
        } else {
            0
        };
        report.total.bytes += size.bytes;
        report.total.files += size.files;
        report.found.push(Artifact {
            project,
            path: candidate.path,
            kind: candidate.kind,
            size,
            unreadable: measured.unreadable,
            state,
            untouched,
            rebuild: candidate.rebuild,
            confidence: candidate.confidence,
            built_programs: programs,
        });
        on_progress(done + 1, total);
    }
    report
        .found
        .sort_by_cached_key(|a| (Reverse(a.size.bytes), a.path.clone()));
    report
}

#[cfg(test)]
#[path = "../tests/core_artifacts.rs"]
mod tests;
