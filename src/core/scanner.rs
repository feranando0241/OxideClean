//! Recursive directory scanning and the rules for which folders and files to skip.

use crate::core::Progress;
use crate::core::models::FileInfo;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// List of technical, dependency, and system folders ignored by default.
///
/// These folders generate a lot of "noise" because they contain hundreds of
/// library files, build artifacts, or OS files that are not relevant to the user.
pub const IGNORED_DIRECTORIES: &[&str] = &[
    // Version control
    ".git",
    // Rust build output
    "target",
    // JavaScript/Node dependencies
    "node_modules",
    // IDE and editor cache / config
    ".cache",
    ".vscode",
    ".idea",
    // System Trash
    ".Trash",
    // Python virtual environments (most common exact names)
    "venv",
    ".venv",
    "env",
    ".env",
    "__pycache__",
    // macOS system library
    "Library",
    // Build output folders for web / Java / Python projects
    "dist",
    "build",
    ".next",
    ".nuxt",
    "out",
    // Python and Ruby package managers
    "site-packages",
    ".gem",
    // Tool cache folders
    ".npm",
    ".yarn",
    ".pnpm-store",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
];

/// List of hidden, system, or binary files ignored during scanning.
pub const IGNORED_FILES: &[&str] = &[
    // macOS metadata files
    ".DS_Store",
    ".localized",
    // Windows metadata files
    "Thumbs.db",
    "desktop.ini",
    // Dependency lock files (auto-generated)
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Pipfile.lock",
    "poetry.lock",
];

/// Compiled or binary file extensions to ignore (without the leading dot).
const IGNORED_EXTENSIONS: &[&str] = &[
    "pyc",   // Python compiled bytecode
    "pyo",   // Python optimized bytecode
    "class", // Java compiled bytecode
    "o",     // C/C++ compiled object file
];

/// Endings of folder names ignored whatever comes before them.
const IGNORED_DIRECTORY_SUFFIXES: &[&str] = &[
    // Python virtual environment variants: project_venv, my-env, report_venv, etc.
    "_venv",
    "-venv",
    "-env",
    // macOS application bundles
    ".app",
    ".framework",
    ".bundle",
];

/// Determines whether an individual file should be ignored.
///
/// Ignores files in the exact-match list and those with bytecode/binary extensions.
/// A name without an extension is never ignored for it: a file called `o` or
/// `class` is a file like any other.
pub fn should_ignore_file(file_name: &str) -> bool {
    IGNORED_FILES.contains(&file_name)
        || Path::new(file_name)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| IGNORED_EXTENSIONS.contains(&ext))
}

/// Determines whether a directory name should be ignored during scanning.
///
/// Ignores directories in the explicit list, all hidden folders (`.something`),
/// those ending in `_venv` or `-env` (Python virtual environment variants),
/// and macOS application bundles (`.app`, `.framework`, `.bundle`).
pub fn should_ignore_directory(dir_name: &str) -> bool {
    IGNORED_DIRECTORIES.contains(&dir_name)
        // Hidden system folders (start with a dot), except "." which is the current directory
        || (dir_name.starts_with('.') && dir_name != ".")
        || IGNORED_DIRECTORY_SUFFIXES
            .iter()
            .any(|suffix| dir_name.ends_with(suffix))
}

/// True if the scan skips the folder behind `entry`. A name that isn't valid
/// UTF-8 matches no rule.
fn is_ignored_directory(entry: &fs::DirEntry) -> bool {
    should_ignore_directory(entry.file_name().to_str().unwrap_or(""))
}

/// The entry as a scanned file, or `None` if its name is on the ignore list
/// or its details can't be read.
fn scanned_file(entry: &fs::DirEntry) -> Option<FileInfo> {
    if should_ignore_file(entry.file_name().to_str().unwrap_or("")) {
        return None;
    }
    let metadata = entry.metadata().ok()?;
    Some(FileInfo::from_metadata(entry.path(), &metadata))
}

/// Scans a directory and lists only its immediately accessible subdirectories.
pub fn list_accessible_subdirectories(dir_path: &Path) -> io::Result<Vec<PathBuf>> {
    // Unreadable entries are skipped instead of failing the whole listing
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir_path)?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|entry| !is_ignored_directory(entry))
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// Scans a directory and all its subfolders recursively.
/// Returns a list of all files found. `on_progress` gets the running count
/// of files found (the total is unknown during a scan, so it is always 0).
pub fn scan_directory(dir_path: &Path, on_progress: Progress) -> io::Result<Vec<FileInfo>> {
    let mut files = Vec::new();
    scan_recursive(dir_path, &mut files, on_progress)?;
    Ok(files)
}

/// The files directly inside `dir`, with the same ignore rules as
/// `scan_directory` but without entering subfolders.
pub fn scan_loose_files(dir_path: &Path) -> io::Result<Vec<FileInfo>> {
    Ok(fs::read_dir(dir_path)?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|entry| scanned_file(&entry))
        .collect())
}

/// Recursive helper that traverses the folder hierarchy.
fn scan_recursive(
    current_path: &Path,
    files: &mut Vec<FileInfo>,
    on_progress: Progress,
) -> io::Result<()> {
    let entries = match fs::read_dir(current_path) {
        Ok(read_dir) => read_dir,
        Err(_) => return Ok(()), // No read permission — skip silently
    };

    // Entries that vanish or can't be inspected mid-scan are skipped silently
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if !is_ignored_directory(&entry) {
                scan_recursive(&entry.path(), files, on_progress)?;
            }
        } else if file_type.is_file()
            && let Some(file) = scanned_file(&entry)
        {
            files.push(file);
            on_progress(files.len(), 0);
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "../tests/core_scanner.rs"]
mod tests;
