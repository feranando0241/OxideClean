//! Moving build folders to the Trash, with everything that made them eligible
//! checked again right before each move. The rules: ADR-0005.

use crate::core::Progress;
use crate::core::artifacts::{
    Artifact, ProjectState, activity_by_folder, candidates_in, measure, project_state,
};
use crate::core::scanner::scan_directory;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A build folder changed this recently may belong to a build still running.
const RECENT_CHANGE: Duration = Duration::from_secs(10 * 60);

/// Why a build folder was left in place. `core` only says what happened; the
/// UI decides how to word it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactSkip {
    /// A Python environment, or no lockfile: a rebuild can't be relied on.
    NotMovable,
    /// It no longer matches the closed list: its manifest, lockfile or tag is
    /// gone, or the folder was replaced.
    NoLongerABuildFolder,
    /// It was listed as forgotten, but the project has been used since.
    ProjectInUse,
    /// Something inside changed moments ago, maybe a build still running.
    RecentlyChanged,
    /// The system refused to move it (e.g. permissions); its message.
    CouldNotMove(String),
}

/// Outcome of moving build folders. Every folder is attempted; one failure
/// doesn't stop the rest.
#[derive(Debug, Default)]
pub struct ArtifactTrashReport {
    pub moved: usize,
    /// Space those folders hold. It only comes back when the Trash is emptied.
    pub moved_bytes: u64,
    /// Folders left in place, with the reason.
    pub skipped: Vec<(PathBuf, ArtifactSkip)>,
}

/// Moves `selected` to the operating system Trash, checking each one first.
pub fn trash_artifacts(
    selected: &[Artifact],
    now: SystemTime,
    threshold: Duration,
    on_progress: Progress,
) -> ArtifactTrashReport {
    remove_artifacts(selected, now, threshold, on_progress, &|path| {
        trash::delete(path).map_err(|e| e.to_string())
    })
}

/// `trash_artifacts` with the removal step passed in, so tests can run the
/// real checks without touching the user's Trash.
pub fn remove_artifacts(
    selected: &[Artifact],
    now: SystemTime,
    threshold: Duration,
    on_progress: Progress,
    remove: &dyn Fn(&Path) -> Result<(), String>,
) -> ArtifactTrashReport {
    let mut report = ArtifactTrashReport::default();
    on_progress(0, selected.len());
    for (done, artifact) in selected.iter().enumerate() {
        let outcome = recheck(artifact, now, threshold).and_then(|bytes| {
            remove(&artifact.path)
                .map(|()| bytes)
                .map_err(ArtifactSkip::CouldNotMove)
        });
        match outcome {
            Ok(bytes) => {
                report.moved += 1;
                report.moved_bytes += bytes;
            }
            Err(skip) => report.skipped.push((artifact.path.clone(), skip)),
        }
        on_progress(done + 1, selected.len());
    }
    report
}

/// Checks from disk that `artifact` can still be moved, and returns its size.
fn recheck(artifact: &Artifact, now: SystemTime, threshold: Duration) -> Result<u64, ArtifactSkip> {
    if !artifact.can_move() {
        return Err(ArtifactSkip::NotMovable);
    }
    let still_valid = candidates_in(&artifact.project)
        .into_iter()
        .any(|candidate| {
            candidate.path == artifact.path
                && candidate.kind == artifact.kind
                && candidate.verified
                && candidate.confidence == artifact.confidence
        });
    if !still_valid {
        return Err(ArtifactSkip::NoLongerABuildFolder);
    }

    // A fresh scan of the whole project: unlike the list, it can't miss a
    // folder left out with --exclude. Active projects were chosen knowingly.
    if artifact.state == ProjectState::Forgotten {
        let files = scan_directory(&artifact.project, &|_, _| {})
            .map_err(|_| ArtifactSkip::NoLongerABuildFolder)?;
        let folders = activity_by_folder(&files, &artifact.project, now, threshold);
        let state = folders
            .get(artifact.project.as_path())
            .map(|activity| project_state(&artifact.project, activity, now, threshold));
        if state != Some(ProjectState::Forgotten) {
            return Err(ArtifactSkip::ProjectInUse);
        }
    }

    let measured = measure(&artifact.path);
    let recently_changed = measured.newest.is_some_and(|newest| {
        now.duration_since(newest)
            .map_or(true, |age| age < RECENT_CHANGE)
    });
    if recently_changed {
        return Err(ArtifactSkip::RecentlyChanged);
    }
    Ok(measured.size.bytes)
}

#[cfg(test)]
#[path = "../tests/core_artifact_trash.rs"]
mod tests;
