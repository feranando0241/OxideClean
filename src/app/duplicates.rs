//! `--dupes`: find identical files, show the groups, and move the extra
//! copies to the Trash when asked.

use super::{AppError, all_moved, confirmed, nothing_moved, require_terminal};
use crate::cli::RunOptions;
use crate::core::duplicates::hash_cache::HashCache;
use crate::core::duplicates::trash::trash_duplicates;
use crate::core::duplicates::{
    DuplicateGroup, extra_copies_in, find_exact_duplicates, keep_copies_inside,
};
use crate::core::models::FileInfo;
use crate::ui::duplicates::{
    choose_groups_to_trash, confirm_trash_all, kept_inside_note, print_exact_duplicates,
    print_trash_report,
};
use crate::ui::input::{home_dir, resolve_user_folder};
use crate::ui::progress::with_spinner;
use crate::ui::style::{hint, plural};
use std::path::{Path, PathBuf};

/// What happens after the duplicate groups are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DupesMode {
    /// Only list them (`--dupes`).
    ListOnly,
    /// Move every extra copy after a y/N question, or straight away with
    /// `assume_yes` (`--dupes --trash [--yes]`).
    TrashAll { assume_yes: bool },
    /// Let the user pick groups and a folder to keep (the menu).
    Choose,
}

impl DupesMode {
    /// The mode the command-line flags ask for.
    pub fn from_flags(trash: bool, assume_yes: bool) -> DupesMode {
        if trash {
            DupesMode::TrashAll { assume_yes }
        } else {
            DupesMode::ListOnly
        }
    }
}

/// Finds the duplicate groups, shows them, and then acts according to `mode`.
///
/// With `keep_in`, each group keeps every copy inside that folder (if it has
/// any) instead of the oldest. The folder must exist and hold at least one
/// copy; otherwise nothing happens and an error is returned, so a typo can't
/// make `--trash --yes` remove the copies the user meant to keep.
///
/// Moving anything needs a terminal to confirm on, unless `assume_yes`.
/// With `options.use_cache`, hashes of unchanged files are reused from
/// earlier runs and the cache is updated (a failed save is not an error).
pub fn handle_duplicates(
    root: &Path,
    files: &[FileInfo],
    options: &RunOptions,
    keep_in: Option<&str>,
    mode: DupesMode,
) -> Result<(), AppError> {
    // Check the folder before hashing, so a typo fails fast and the same way
    // whether or not any duplicates turn up
    let keep_path = keep_in
        .map(|folder| folder_to_keep(root, folder))
        .transpose()?;

    let mut duplicate_groups = find_duplicates(files, options.use_cache);
    if duplicate_groups.is_empty() {
        println!("No identical copies found. Nothing to clean up here.\n");
        return Ok(());
    }

    if let Some((folder, path)) = keep_in.zip(keep_path) {
        let found = keep_copies_inside(&mut duplicate_groups, &path);
        if found == 0 {
            return Err(AppError::NoCopyInside(path));
        }
        hint(&kept_inside_note(folder, found, duplicate_groups.len()));
    }

    print_exact_duplicates(root, &duplicate_groups);

    let to_trash = match mode {
        DupesMode::ListOnly => return Ok(()),
        DupesMode::TrashAll { assume_yes } => {
            // Possible when --keep-in holds every copy of every group
            if extra_copies_in(&duplicate_groups) == 0 {
                println!("Every copy is kept, so there's nothing to move.\n");
                return Ok(());
            }
            if !confirmed(assume_yes, || confirm_trash_all(&duplicate_groups))? {
                return nothing_moved();
            }
            duplicate_groups
        }
        DupesMode::Choose => {
            require_terminal()?;
            match choose_groups_to_trash(root, &mut duplicate_groups) {
                Some(selected) => selected,
                None => return nothing_moved(),
            }
        }
    };

    let report = with_spinner(
        "Re-checking files before moving them",
        "files",
        |progress| trash_duplicates(&to_trash, progress),
    );
    print_trash_report(root, &report);
    all_moved(report.failed.len())
}

/// The folder given to `--keep-in`, spelled like the scanned files. It must
/// exist.
fn folder_to_keep(root: &Path, folder: &str) -> Result<PathBuf, AppError> {
    let path = resolve_user_folder(root, folder);
    if path.is_dir() {
        Ok(path)
    } else {
        Err(AppError::NotADirectory(path))
    }
}

/// Finds the duplicate groups among `files`. With `use_cache`, hashes saved
/// by earlier runs are reused and the new ones are saved for the next.
fn find_duplicates(files: &[FileInfo], use_cache: bool) -> Vec<DuplicateGroup> {
    hint("\nComparing file contents …");
    let cache_path = if use_cache { cache_file() } else { None };
    let cache = match &cache_path {
        Some(path) => HashCache::load(path),
        None => HashCache::disabled(),
    };

    let groups = with_spinner("Comparing file contents", "files", |progress| {
        find_exact_duplicates(files, &cache, progress)
    });

    if let Some(path) = &cache_path
        && let Err(err) = cache.save(path)
    {
        hint(&format!(
            "(Couldn't save the hash cache at {}: {})",
            path.display(),
            err
        ));
    }
    if cache.hits() > 0 {
        hint(&format!(
            "Reused {} for files that haven't changed.",
            plural(cache.hits(), "saved hash", "saved hashes")
        ));
    }
    groups
}

/// Where the hash cache is stored: under `$XDG_CACHE_HOME` if set, otherwise
/// `~/Library/Caches` on macOS (Apple's place for app caches) and `~/.cache`
/// elsewhere. `None` if there is no home folder to put it in.
fn cache_file() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CACHE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = home_dir()?;
            if cfg!(target_os = "macos") {
                home.join("Library/Caches")
            } else {
                home.join(".cache")
            }
        }
    };
    Some(base.join("oxide_clean").join("hashes.tsv"))
}
