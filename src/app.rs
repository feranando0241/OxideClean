//! Actions shared by the command line and the interactive menus: each one
//! calls the core logic and shows the result. Anything that must behave the
//! same in both modes (especially the rules for moving files to the Trash)
//! lives here, in one place.
//!
//! This file holds what every action shares: the error type, the scan, the
//! command-line dispatch and the rules for moving files. Each action with
//! more than a screen to show has its own file:
//! - `duplicates`  identical files and their extra copies
//! - `artifacts`   build folders
//! - `screenshots` screenshots left behind
//! - `organize`    sorting loose files into folders, and undoing it
//! - `similar`     comparing one document with the others

mod artifacts;
mod duplicates;
mod organize;
mod screenshots;
mod similar;

pub use artifacts::handle_artifacts;
pub use duplicates::{DupesMode, handle_duplicates};
pub use organize::{handle_organize, handle_undo};
pub use screenshots::handle_screenshots;
pub use similar::show_similar_documents;

use crate::cli::{Action, RunOptions};
use crate::core::files::{filter_by_extension, find_same_name_files, get_top_largest};
use crate::core::models::FileInfo;
use crate::core::old_files::{days, find_old_files};
use crate::core::scanner::scan_directory;
use crate::core::similarity::SimilarityError;
use crate::ui::input::{resolve_user_folder, resolve_user_path};
use crate::ui::progress::with_spinner;
use crate::ui::report::{
    print_filtered_files, print_old_files, print_same_name_results, print_top_largest,
};
use crate::ui::style::{hint, plural};
use std::fmt;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Everything that can make an action fail. Each variant keeps the details
/// (and the underlying error, when there is one) instead of a pre-built
/// message, so callers can tell failures apart; `Display` gives the text.
#[derive(Debug)]
pub enum AppError {
    NotADirectory(PathBuf),
    Scan(io::Error),
    UnreadableFile {
        path: PathBuf,
        source: io::Error,
    },
    NotAFile(PathBuf),
    Similarity(SimilarityError),
    /// Moving to the Trash needs a yes/no answer and there is no terminal to ask.
    NoTerminalToConfirm,
    /// Some files were left in place; the report lists which and why.
    NotAllMoved(usize),
    /// `--keep-in` named a folder that holds no copy of any duplicate group.
    NoCopyInside(PathBuf),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AppError::NotADirectory(path) => write!(f, "'{}' is not a directory", path.display()),
            AppError::Scan(err) => write!(f, "scan failed: {}", err),
            AppError::UnreadableFile { path, source } => write!(f, "cannot read '{}': {}", path.display(), source),
            AppError::NotAFile(path) => write!(f, "'{}' is not a file", path.display()),
            AppError::Similarity(err) => write!(f, "{}", err),
            AppError::NoTerminalToConfirm => f.write_str(
                "Nothing was moved: there's no terminal to ask for confirmation (pass --yes to skip it)",
            ),
            AppError::NotAllMoved(n) => write!(f, "{} item(s) could not be moved", n),
            AppError::NoCopyInside(path) => write!(
                f,
                "no duplicate group has a copy inside '{}'; nothing was changed",
                path.display()
            ),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Scan(err) | AppError::UnreadableFile { source: err, .. } => Some(err),
            AppError::Similarity(err) => Some(err),
            _ => None,
        }
    }
}

impl From<SimilarityError> for AppError {
    fn from(err: SimilarityError) -> AppError {
        AppError::Similarity(err)
    }
}

// ─────────────────────────────────────────────────────────────
// SCANNING AND DISPATCH
// ─────────────────────────────────────────────────────────────

/// Resolves a folder given on the command line, expanding `~`.
pub fn resolve_folder(path: &str) -> Result<PathBuf, AppError> {
    let root = resolve_user_path(path);
    if root.is_dir() {
        Ok(root)
    } else {
        Err(AppError::NotADirectory(root))
    }
}

/// Scans `root` recursively, showing a spinner with the files found so far,
/// and leaves out anything inside the `--exclude` folders.
pub fn scan(root: &Path, options: &RunOptions) -> io::Result<Vec<FileInfo>> {
    let mut files = with_spinner("Scanning", "files found", |progress| {
        scan_directory(root, progress)
    })?;
    leave_out_excluded(&mut files, root, &options.exclude);
    Ok(files)
}

/// Drops the files inside each `--exclude` folder, saying how many each one
/// left out. A folder that isn't there is reported and ignored.
// ponytail: excluded folders are still walked and then filtered out; skip
// them during the walk if excluding huge folders ever feels slow.
fn leave_out_excluded(files: &mut Vec<FileInfo>, root: &Path, exclude: &[String]) {
    for folder in exclude {
        let path = resolve_user_folder(root, folder);
        if !path.is_dir() {
            hint(&format!(
                "(--exclude {}: no such folder here, ignored)",
                folder
            ));
            continue;
        }
        let before = files.len();
        files.retain(|f| !f.path.starts_with(&path));
        hint(&format!(
            "Left out {} inside '{}'.",
            plural(before - files.len(), "file", "files"),
            folder
        ));
    }
}

/// Scans `root` and runs one command-line action on it.
pub fn run_action(root: &Path, action: Action, options: &RunOptions) -> Result<(), AppError> {
    // Organizing only reads the files directly in `root`, so it skips the scan
    match action {
        Action::Organize {
            by,
            apply,
            assume_yes,
        } => return handle_organize(root, by, apply, assume_yes),
        Action::Undo { assume_yes } => return handle_undo(root, assume_yes),
        _ => {}
    }
    let files = scan(root, options).map_err(AppError::Scan)?;

    match action {
        Action::Largest(n) => print_top_largest(root, &get_top_largest(&files, n)),
        Action::Ext(ext) => show_files_by_extension(root, &files, &ext),
        Action::Dupes {
            trash,
            assume_yes,
            keep_in,
        } => {
            let mode = DupesMode::from_flags(trash, assume_yes);
            handle_duplicates(root, &files, options, keep_in.as_deref(), mode)?
        }
        Action::SameName => show_same_name_files(root, &files),
        Action::Old(n) => show_old_files(root, &files, n),
        Action::Artifacts {
            days,
            all,
            trash,
            assume_yes,
        } => {
            let mode = TrashMode::from_flags(trash, assume_yes);
            handle_artifacts(root, &files, options, days, all, mode)?
        }
        Action::Similar { file, threshold } => {
            let target = similar::locate_base_file(root, &file)?;
            show_similar_documents(root, &target, &files, threshold)?;
        }
        Action::Screenshots {
            days,
            trash,
            assume_yes,
        } => {
            let mode = TrashMode::from_flags(trash, assume_yes);
            handle_screenshots(root, &files, days, mode)?
        }
        Action::Organize { .. } | Action::Undo { .. } => unreachable!("handled before the scan"),
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// ACTIONS THAT ONLY SHOW
// ─────────────────────────────────────────────────────────────

/// Prints the files matching an extension, or a notice when there are none.
pub fn show_files_by_extension(root: &Path, files: &[FileInfo], ext: &str) {
    // Accept both "pdf" and ".pdf"
    let ext = ext.trim().trim_start_matches('.');
    let filtered = filter_by_extension(files, ext);
    if filtered.is_empty() {
        println!("No .{} files in this folder.", ext);
    } else {
        print_filtered_files(root, ext, &filtered);
    }
}

/// Finds and displays files that share the same name within the scanned folder.
///
/// Unlike identical copies, content is not analyzed here: two files with the
/// same name but different content (e.g. different versions of a document)
/// will also appear in the list.
pub fn show_same_name_files(root: &Path, files: &[FileInfo]) {
    let groups = find_same_name_files(files);
    print_same_name_results(root, &groups);
}

/// Prints the files untouched for more than `n` days (see ADR-0004). `n`
/// must be a valid threshold (`core::old_files::days`); the CLI and the menu
/// check it first.
pub fn show_old_files(root: &Path, files: &[FileInfo], n: u64) {
    let threshold = threshold_of(n);
    // Read the clock after the scan, so no scanned date can be in the future
    // just because it was written during the scan
    let now = SystemTime::now();
    print_old_files(root, &find_old_files(files, now, threshold), n);
}

/// `n` days as a threshold. The CLI and the menu only let valid numbers of
/// days through (`core::old_files::days`), so this can't fail.
fn threshold_of(n: u64) -> Duration {
    days(n).expect("the caller checked the number of days")
}

// ─────────────────────────────────────────────────────────────
// THE RULES FOR MOVING FILES
// ─────────────────────────────────────────────────────────────

/// What happens after a list of things that can go to the Trash is shown
/// (build folders, screenshots).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashMode {
    /// Only list them.
    ListOnly,
    /// Let the user pick which to move (`--trash`, the menu).
    Choose,
    /// Move, without asking, the ones that may go unattended (`--trash --yes`).
    Unattended,
}

impl TrashMode {
    /// The mode the command-line flags ask for.
    pub fn from_flags(trash: bool, assume_yes: bool) -> TrashMode {
        match (trash, assume_yes) {
            (false, _) => TrashMode::ListOnly,
            (true, false) => TrashMode::Choose,
            (true, true) => TrashMode::Unattended,
        }
    }
}

/// Moving files needs someone to confirm it, so refuse when there is no
/// terminal to ask on (a script or a pipe) instead of guessing an answer.
fn require_terminal() -> Result<(), AppError> {
    if io::stdin().is_terminal() {
        Ok(())
    } else {
        Err(AppError::NoTerminalToConfirm)
    }
}

/// Whether to go ahead and move files: yes with `assume_yes`; otherwise
/// whatever the user answers to `ask`, which needs a terminal.
fn confirmed(assume_yes: bool, ask: impl FnOnce() -> bool) -> Result<bool, AppError> {
    if assume_yes {
        return Ok(true);
    }
    require_terminal()?;
    Ok(ask())
}

/// How an action that could have moved files ends when it moved none.
fn nothing_moved() -> Result<(), AppError> {
    println!("Nothing was moved.\n");
    Ok(())
}

/// How an action that moved files ends: with an error if `left` of them had
/// to stay where they were (the report already said which and why).
fn all_moved(left: usize) -> Result<(), AppError> {
    if left == 0 {
        Ok(())
    } else {
        Err(AppError::NotAllMoved(left))
    }
}
