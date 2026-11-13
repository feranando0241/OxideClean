//! What every move to the Trash has in common, whatever is being moved: each
//! item gets its own attempt, one failure never stops the rest, and the
//! caller learns what stayed and why.
//!
//! What makes an item safe to move is not decided here. Each feature checks
//! that from disk right before its move: `artifact_trash`, `screenshots` and
//! `duplicates::trash`.

use crate::core::Progress;
use std::path::{Path, PathBuf};

/// Outcome of moving items to the Trash. `S` is the feature's own list of
/// reasons for leaving an item in place.
#[derive(Debug)]
pub struct TrashOutcome<S> {
    pub moved: usize,
    /// Space the moved items hold. It only comes back when the Trash is emptied.
    pub moved_bytes: u64,
    /// Items left in place, with the reason.
    pub skipped: Vec<(PathBuf, S)>,
}

/// Moves `path` to the operating system Trash. An error holds the system's
/// message.
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    ::trash::delete(path).map_err(|e| e.to_string())
}

/// Attempts every item in turn. `attempt` checks one item and moves it,
/// returning the bytes it held, or why it stayed. `path_of` names an item in
/// the report. `on_progress` gets `(attempted, total)`.
pub(crate) fn attempt_each<T, S>(
    items: &[T],
    path_of: impl Fn(&T) -> &Path,
    on_progress: Progress,
    attempt: impl Fn(&T) -> Result<u64, S>,
) -> TrashOutcome<S> {
    let mut outcome = TrashOutcome {
        moved: 0,
        moved_bytes: 0,
        skipped: Vec::new(),
    };
    on_progress(0, items.len());
    for (done, item) in items.iter().enumerate() {
        match attempt(item) {
            Ok(bytes) => {
                outcome.moved += 1;
                outcome.moved_bytes += bytes;
            }
            Err(skip) => outcome.skipped.push((path_of(item).to_path_buf(), skip)),
        }
        on_progress(done + 1, items.len());
    }
    outcome
}
