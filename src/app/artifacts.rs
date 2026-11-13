//! `--artifacts`: list the build folders of projects, and move the chosen
//! ones to the Trash (see ADR-0005).

use super::{AppError, TrashMode, all_moved, nothing_moved, require_terminal, threshold_of};
use crate::cli::RunOptions;
use crate::core::artifact_trash::trash_artifacts;
use crate::core::artifacts::{Artifact, find_artifacts};
use crate::core::models::FileInfo;
use crate::ui::artifacts::{choose_artifacts, print_artifact_trash_report, print_artifacts};
use crate::ui::input::resolve_user_folder;
use crate::ui::progress::with_spinner;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Shows the build folders of projects untouched for more than `n` days (see
/// ADR-0005), or of every project with `all`, and acts according to `mode`.
/// Unattended, only the clear-cut ones move. `n` must be a valid threshold,
/// like in `show_old_files`.
pub fn handle_artifacts(
    root: &Path,
    files: &[FileInfo],
    options: &RunOptions,
    n: u64,
    all: bool,
    mode: TrashMode,
) -> Result<(), AppError> {
    let threshold = threshold_of(n);
    let excluded: Vec<PathBuf> = options
        .exclude
        .iter()
        .map(|folder| resolve_user_folder(root, folder))
        .collect();
    let now = SystemTime::now();
    let found = with_spinner("Measuring build folders", "folders", |progress| {
        find_artifacts(files, root, &excluded, now, threshold, all, progress)
    });
    print_artifacts(root, &found, n, all, mode == TrashMode::ListOnly);

    let selected: Vec<Artifact> = match mode {
        TrashMode::ListOnly => return Ok(()),
        TrashMode::Unattended => found
            .found
            .iter()
            .filter(|a| a.moves_unattended())
            .cloned()
            .collect(),
        TrashMode::Choose => {
            if !found.found.iter().any(|a| a.can_move()) {
                return Ok(());
            }
            require_terminal()?;
            choose_artifacts(&found).unwrap_or_default()
        }
    };
    if selected.is_empty() {
        return nothing_moved();
    }

    let report = with_spinner(
        "Re-checking build folders before moving them",
        "folders",
        // A fresh clock: the user may have taken a while to choose
        |progress| trash_artifacts(&selected, SystemTime::now(), threshold, progress),
    );
    print_artifact_trash_report(root, &report);
    all_moved(report.skipped.len())
}
