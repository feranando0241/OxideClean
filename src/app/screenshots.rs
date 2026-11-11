//! `--screenshots`: list the screenshots left behind after pasting them, and
//! move the chosen ones to the Trash (see ADR-0008).

use super::{AppError, require_terminal};
use crate::core::models::FileInfo;
use crate::core::old_files::days;
use crate::core::screenshots::{find_screenshots, trash_screenshots};
use crate::ui::progress::Spinner;
use crate::ui::screenshots::{
    choose_screenshots, print_screenshot_trash_report, print_screenshots,
};
use std::path::Path;
use std::time::SystemTime;

/// What happens after the screenshots are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotsMode {
    /// Only list them (`--screenshots`).
    ListOnly,
    /// Let the user pick which to move (`--trash`, the menu).
    Choose,
    /// Move every one listed without asking (`--trash --yes`).
    MoveAll,
}

/// Shows the screenshots older than `n` days and acts according to `mode`.
/// `n` must be a valid threshold (`core::old_files::days`).
pub fn handle_screenshots(
    root: &Path,
    files: &[FileInfo],
    n: u64,
    mode: ScreenshotsMode,
) -> Result<(), AppError> {
    let threshold = days(n).expect("the caller checked the number of days");
    // Read the clock after the scan, so no date written during it is in the future
    let found = find_screenshots(files, SystemTime::now(), threshold);
    print_screenshots(root, &found, n, mode == ScreenshotsMode::ListOnly);

    let selected = match mode {
        ScreenshotsMode::ListOnly => return Ok(()),
        _ if found.is_empty() => return Ok(()),
        ScreenshotsMode::MoveAll => found,
        ScreenshotsMode::Choose => {
            require_terminal()?;
            match choose_screenshots(&found) {
                Some(selected) => selected,
                None => {
                    println!("Nothing was moved.\n");
                    return Ok(());
                }
            }
        }
    };

    let report = {
        let spinner = Spinner::start("Checking screenshots before moving them", "files");
        // A fresh clock: the user may have taken a while to choose
        trash_screenshots(&selected, SystemTime::now(), threshold, &|done, total| {
            spinner.update(done, total)
        })
    };
    print_screenshot_trash_report(root, &report);
    if report.skipped.is_empty() {
        Ok(())
    } else {
        Err(AppError::NotAllMoved(report.skipped.len()))
    }
}
