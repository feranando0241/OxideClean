//! Screenshots and screen recordings left behind after pasting them: found by
//! the mark macOS sets on them, or by the name macOS gives them, and moved to
//! the Trash only after checking each one again. The rules: ADR-0008.

use crate::core::Progress;
use crate::core::models::FileInfo;
use crate::core::organize::fold_accents;
use std::cmp::Reverse;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Screenshots older than this many days are listed by default: they were
/// taken to paste somewhere, and that moment has passed.
pub const DEFAULT_SCREENSHOT_DAYS: u64 = 1;

/// Formats macOS can save screenshots and recordings in. Other files aren't
/// even checked for the mark.
const CAPTURE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "heic", "tif", "tiff", "gif", "pdf", "bmp", "mov", "mp4",
];

/// How macOS names them, in English and Spanish (lowercase, without accents),
/// always followed by the date: "Screenshot 2026-09-25 at 4.11.23 PM.png".
/// "Screen Shot" is the name before macOS 12.
const NAME_PREFIXES: &[&str] = &[
    "screenshot ",
    "screen shot ",
    "captura de pantalla ",
    "screen recording ",
    "grabacion de pantalla ",
];

/// Photo libraries are single documents to the user: files inside them
/// (screenshots imported into Photos keep the mark) must never be touched.
const LIBRARY_SUFFIXES: &[&str] = &[".photoslibrary", ".photolibrary", ".aplibrary"];

/// How a screenshot was recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    /// macOS marked it as a screen capture when it took it.
    Mark,
    /// Only its name says so: the mark was lost (AirDrop, a download) or
    /// this isn't macOS.
    Name,
}

/// A screenshot old enough to be listed.
#[derive(Debug, Clone)]
pub struct Screenshot {
    pub file: FileInfo,
    pub detected: Detected,
    /// Time since its content or details last changed.
    pub age: Duration,
}

/// Why a screenshot was left in place. `core` only says what happened; the
/// UI decides how to word it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenshotSkip {
    /// Missing, replaced, edited, or no longer old enough since the list.
    ChangedSinceScan,
    /// The system refused to move it (e.g. permissions); its message.
    CouldNotMove(String),
}

/// Outcome of moving screenshots to the Trash. Every file is attempted; one
/// failure doesn't stop the rest.
#[derive(Debug, Default)]
pub struct ScreenshotTrashReport {
    pub moved: usize,
    /// Space they hold. It only comes back when the Trash is emptied.
    pub moved_bytes: u64,
    pub skipped: Vec<(PathBuf, ScreenshotSkip)>,
}

/// The screenshots among `files` not changed for longer than `threshold`,
/// biggest first. Reads the mark from disk, but only for capture formats.
pub fn find_screenshots(
    files: &[FileInfo],
    now: SystemTime,
    threshold: Duration,
) -> Vec<Screenshot> {
    let mut found: Vec<Screenshot> = files
        .iter()
        .filter(|f| CAPTURE_EXTENSIONS.contains(&f.extension_or("")))
        .filter(|f| !inside_photo_library(&f.path))
        .filter_map(|file| {
            let age = age_of(file, now).filter(|age| *age > threshold)?;
            let detected = if has_capture_mark(&file.path) {
                Detected::Mark
            } else if named_like_a_capture(&file.path.file_name()?.to_string_lossy()) {
                Detected::Name
            } else {
                return None;
            };
            Some(Screenshot {
                file: file.clone(),
                detected,
                age,
            })
        })
        .collect();
    found.sort_by_key(|s| (Reverse(s.file.size_bytes), s.file.path.clone()));
    found
}

/// True if the name starts like one macOS gives captures, date included, so
/// "Screenshot ideas.png" doesn't count.
pub fn named_like_a_capture(name: &str) -> bool {
    let name = fold_accents(&name.to_lowercase());
    NAME_PREFIXES.iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|rest| {
            let date = rest.as_bytes();
            date.len() >= 10
                && date[..10].iter().enumerate().all(|(i, b)| match i {
                    4 | 7 => *b == b'-',
                    _ => b.is_ascii_digit(),
                })
        })
    })
}

/// Time since the content (mtime) or the details (ctime) last changed,
/// whichever is more recent. `None` without dates, or with one in the future.
fn age_of(file: &FileInfo, now: SystemTime) -> Option<Duration> {
    let modified = now.duration_since(file.modified?).ok()?;
    match file.changed {
        Some(changed) => Some(modified.min(now.duration_since(changed).ok()?)),
        None => Some(modified),
    }
}

fn inside_photo_library(path: &Path) -> bool {
    path.components().any(|c| match c {
        Component::Normal(name) => {
            let name = name.to_string_lossy().to_lowercase();
            LIBRARY_SUFFIXES.iter().any(|s| name.ends_with(s))
        }
        _ => false,
    })
}

/// True if macOS marked the file as a screen capture (the
/// `kMDItemIsScreenCapture` extended attribute). Symlinks aren't followed.
#[cfg(target_os = "macos")]
pub fn has_capture_mark(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let name = c"com.apple.metadata:kMDItemIsScreenCapture";
    // SAFETY: both strings are NUL-terminated and live through the call. A
    // null buffer with size 0 only asks for the value's size: nothing is written.
    let size = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            std::ptr::null_mut(),
            0,
            0,
            libc::XATTR_NOFOLLOW,
        )
    };
    size >= 0
}

/// Off macOS there is no mark; only names are recognized.
#[cfg(not(target_os = "macos"))]
pub fn has_capture_mark(_path: &Path) -> bool {
    false
}

/// Moves `selected` to the operating system Trash, checking each one first.
pub fn trash_screenshots(
    selected: &[Screenshot],
    now: SystemTime,
    threshold: Duration,
    on_progress: Progress,
) -> ScreenshotTrashReport {
    remove_screenshots(selected, now, threshold, on_progress, &|path| {
        trash::delete(path).map_err(|e| e.to_string())
    })
}

/// `trash_screenshots` with the removal step passed in, so tests can run the
/// real checks without touching the user's Trash.
pub fn remove_screenshots(
    selected: &[Screenshot],
    now: SystemTime,
    threshold: Duration,
    on_progress: Progress,
    remove: &dyn Fn(&Path) -> Result<(), String>,
) -> ScreenshotTrashReport {
    let mut report = ScreenshotTrashReport::default();
    on_progress(0, selected.len());
    for (done, shot) in selected.iter().enumerate() {
        let path = &shot.file.path;
        let outcome = if unchanged(shot, now, threshold) {
            remove(path).map_err(ScreenshotSkip::CouldNotMove)
        } else {
            Err(ScreenshotSkip::ChangedSinceScan)
        };
        match outcome {
            Ok(()) => {
                report.moved += 1;
                report.moved_bytes += shot.file.size_bytes;
            }
            Err(skip) => report.skipped.push((path.clone(), skip)),
        }
        on_progress(done + 1, selected.len());
    }
    report
}

/// From disk: still a regular file, same size and date as listed, and still
/// old enough (an edit since the list makes it recent).
fn unchanged(shot: &Screenshot, now: SystemTime, threshold: Duration) -> bool {
    let Ok(metadata) = fs::symlink_metadata(&shot.file.path) else {
        return false;
    };
    let current = FileInfo::from_metadata(shot.file.path.clone(), &metadata);
    metadata.is_file()
        && current.size_bytes == shot.file.size_bytes
        && current.modified == shot.file.modified
        && age_of(&current, now).is_some_and(|age| age > threshold)
}

#[cfg(test)]
#[path = "../tests/core_screenshots.rs"]
mod tests;
