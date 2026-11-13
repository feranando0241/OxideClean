//! Moving build folders to the Trash, with everything that made them eligible
//! checked again right before each move. The rules: ADR-0005.

use crate::core::Progress;
use crate::core::artifacts::{
    Artifact, ProjectState, activity_by_folder, candidates_in, measure, project_state,
};
use crate::core::scanner::scan_directory;
use crate::core::trash::{TrashOutcome, attempt_each, move_to_trash};
use std::path::Path;
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
pub type ArtifactTrashReport = TrashOutcome<ArtifactSkip>;

/// Moves `selected` to the operating system Trash, checking each one first.
pub fn trash_artifacts(
    selected: &[Artifact],
    now: SystemTime,
    threshold: Duration,
    on_progress: Progress,
) -> ArtifactTrashReport {
    remove_artifacts(selected, now, threshold, on_progress, &move_to_trash)
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
    attempt_each(
        selected,
        |artifact| artifact.path.as_path(),
        on_progress,
        |artifact| {
            let bytes = recheck(artifact, now, threshold)?;
            remove(&artifact.path).map_err(ArtifactSkip::CouldNotMove)?;
            Ok(bytes)
        },
    )
}

/// Checks from disk that `artifact` can still be moved, and returns its size.
fn recheck(artifact: &Artifact, now: SystemTime, threshold: Duration) -> Result<u64, ArtifactSkip> {
    if !artifact.can_move() {
        return Err(ArtifactSkip::NotMovable);
    }
    if !still_a_build_folder(artifact) {
        return Err(ArtifactSkip::NoLongerABuildFolder);
    }
    // Active projects were chosen knowingly
    if artifact.state == ProjectState::Forgotten {
        check_still_forgotten(artifact, now, threshold)?;
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

/// True if the closed list of ADR-0005 still finds this very folder in its
/// project, rated as it was when listed.
fn still_a_build_folder(artifact: &Artifact) -> bool {
    candidates_in(&artifact.project)
        .into_iter()
        .any(|candidate| {
            candidate.path == artifact.path
                && candidate.kind == artifact.kind
                && candidate.verified
                && candidate.confidence == artifact.confidence
        })
}

/// Judges the project again from a fresh scan of all of it: unlike the list,
/// it can't miss a folder left out with --exclude.
fn check_still_forgotten(
    artifact: &Artifact,
    now: SystemTime,
    threshold: Duration,
) -> Result<(), ArtifactSkip> {
    let files = scan_directory(&artifact.project, &|_, _| {})
        .map_err(|_| ArtifactSkip::NoLongerABuildFolder)?;
    let folders = activity_by_folder(&files, &artifact.project, now, threshold);
    let state = folders
        .get(artifact.project.as_path())
        .map(|activity| project_state(&artifact.project, activity, now, threshold));
    if state == Some(ProjectState::Forgotten) {
        Ok(())
    } else {
        Err(ArtifactSkip::ProjectInUse)
    }
}

#[cfg(test)]
#[path = "../tests/core_artifact_trash.rs"]
mod tests;
