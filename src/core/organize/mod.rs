//! Organizing the loose files of a folder into subfolders by year, size,
//! type or words in the name. Unlike everything else here, it moves files without the Trash to
//! undo it, so it errs towards moving less:
//! - only files directly inside the folder; subfolders may be projects or
//!   bundles that break when their files are pulled out;
//! - hidden files, and downloads still in progress, are left alone;
//! - nothing is ever overwritten, and each file is checked again from disk
//!   right before it moves;
//! - every move is logged, and can be undone (`log`).

mod log;

pub use log::{LOG_NAME, append_log, logged_moves, undo_organize};

use crate::core::Progress;
use crate::core::models::FileInfo;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A file changed this recently may still be downloading or being saved.
const RECENT_CHANGE: Duration = Duration::from_secs(10 * 60);

/// Extensions of downloads that haven't finished.
const IN_PROGRESS_EXTENSIONS: &[&str] = &["crdownload", "part", "partial", "download", "tmp"];

/// Folder per type, by extension. Anything not listed goes to `Other`.
const TYPE_FOLDERS: &[(&str, &[&str])] = &[
    (
        "Images",
        &[
            "jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "tif", "tiff", "bmp", "svg",
            "raw", "cr2", "nef", "arw", "dng", "psd",
        ],
    ),
    (
        "Videos",
        &["mp4", "mov", "avi", "mkv", "webm", "m4v", "wmv", "flv"],
    ),
    (
        "Audio",
        &[
            "mp3", "wav", "m4a", "aac", "flac", "ogg", "aif", "aiff", "opus",
        ],
    ),
    (
        "Documents",
        &[
            "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "pages", "xls", "xlsx", "ods", "csv",
            "numbers", "ppt", "pptx", "odp", "key", "epub",
        ],
    ),
    (
        "Archives",
        &[
            "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "dmg", "iso", "pkg",
        ],
    ),
    (
        "Code",
        &[
            "rs", "py", "js", "ts", "c", "cpp", "h", "java", "go", "rb", "sh", "html", "css",
            "json", "toml", "yaml", "yml",
        ],
    ),
];

/// Folder per keyword, in English and Spanish, for `OrganizeBy::Words`.
/// Words are matched whole against the file name without its extension, in
/// lowercase and without accents (a test checks the table is written that
/// way). The first matching rule wins.
const WORD_FOLDERS: &[(&str, &[&str])] = &[
    (
        "Projects",
        &["proyecto", "proyectos", "project", "projects"],
    ),
    (
        "Homework",
        &["tarea", "tareas", "homework", "assignment", "assignments"],
    ),
    ("Websites", &["website", "websites", "sitio", "web"]),
    (
        "Solutions",
        &["solucion", "soluciones", "solution", "solutions"],
    ),
    (
        "Resumes",
        &["resume", "resumes", "cv", "curriculum", "curriculo"],
    ),
];

/// For `OrganizeBy::Words`, files whose name matches no topic go here by
/// their real extension: "guia pdf.docx" is not a PDF.
const EXTENSION_FOLDERS: &[(&str, &str)] = &[("PDF", "pdf"), ("CSV", "csv")];

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

/// How to group the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrganizeBy {
    /// Year the content last changed (UTC).
    Year,
    /// Under 1 MB, up to 100 MB, up to 1 GB, over 1 GB.
    Size,
    /// Images, Videos, Documents… by extension.
    Type,
    /// Projects, Homework, Resumes… by words in the name. Files with none of
    /// the words stay where they are.
    Words,
}

impl OrganizeBy {
    /// The way of grouping that the command line and the menu call `name`:
    /// year, size, type or words.
    pub fn from_name(name: &str) -> Option<OrganizeBy> {
        match name {
            "year" => Some(OrganizeBy::Year),
            "size" => Some(OrganizeBy::Size),
            "type" => Some(OrganizeBy::Type),
            "words" => Some(OrganizeBy::Words),
            _ => None,
        }
    }
}

/// One file and the place it would move to.
#[derive(Debug, Clone)]
pub struct PlannedMove {
    /// The file as scanned; its size and date are checked again before moving.
    pub file: FileInfo,
    pub to: PathBuf,
}

/// Why a file is not moved. `core` only says what happened; the UI decides
/// how to word it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveSkip {
    /// A file with that name is already in the destination folder.
    DestinationExists,
    /// Another file in this plan goes to the same name.
    SameDestination,
    /// It looks like a download or a save still in progress.
    InProgress,
    /// Missing, replaced, or its size or date changed since the scan.
    ChangedSinceScan,
    /// The system refused to move it (e.g. permissions); its message.
    CouldNotMove(String),
    /// Undo: the file is no longer where it was moved to.
    Gone,
    /// Undo: the log line doesn't describe a move made in this folder (it was
    /// edited, or is damaged), so it is not acted on.
    NotFromThisFolder,
}

/// What would move, and what wouldn't and why.
#[derive(Debug, Default)]
pub struct OrganizePlan {
    pub moves: Vec<PlannedMove>,
    pub skipped: Vec<(PathBuf, MoveSkip)>,
}

/// Outcome of applying a plan. Every file is attempted; one failure doesn't
/// stop the rest.
#[derive(Debug, Default)]
pub struct MoveReport {
    /// `(from, to)` of every file moved, in order: what undoing needs.
    pub moved: Vec<(PathBuf, PathBuf)>,
    pub skipped: Vec<(PathBuf, MoveSkip)>,
}

/// The subfolder `file` belongs in, or `None` if it stays where it is (only
/// with `Words`). `now` decides which dates are in the future.
pub fn folder_for(file: &FileInfo, by: OrganizeBy, now: SystemTime) -> Option<String> {
    match by {
        OrganizeBy::Year => Some(year_folder(file, now)),
        OrganizeBy::Size => Some(size_folder(file.size_bytes).to_string()),
        OrganizeBy::Type => Some(type_folder(file.extension_or("")).to_string()),
        OrganizeBy::Words => word_folder(file).map(String::from),
    }
}

/// The year the content last changed, unless that date is missing or later
/// than `now`.
fn year_folder(file: &FileInfo, now: SystemTime) -> String {
    file.modified
        .filter(|&t| t <= now)
        .and_then(year_of)
        .map_or("Unknown year".to_string(), |y| y.to_string())
}

fn size_folder(size_bytes: u64) -> &'static str {
    match size_bytes {
        s if s < MB => "Under 1 MB",
        s if s < 100 * MB => "1 MB to 100 MB",
        s if s < GB => "100 MB to 1 GB",
        _ => "Over 1 GB",
    }
}

/// `extension` in lowercase, as `FileInfo` keeps it.
fn type_folder(extension: &str) -> &'static str {
    TYPE_FOLDERS
        .iter()
        .find(|(_, extensions)| extensions.contains(&extension))
        .map_or("Other", |(folder, _)| folder)
}

/// The topic the words of the name point to; without one, the folder of its
/// extension, if it has one. `None` leaves the file where it is.
fn word_folder(file: &FileInfo) -> Option<&'static str> {
    let words = words_of(&file.path.file_stem()?.to_string_lossy());
    let topic = WORD_FOLDERS
        .iter()
        .find(|(_, keys)| keys.iter().any(|k| words.iter().any(|w| w == k)))
        .map(|(folder, _)| *folder);
    let extension = file.extension_or("");
    topic.or_else(|| {
        EXTENSION_FOLDERS
            .iter()
            .find(|(_, e)| *e == extension)
            .map(|(folder, _)| *folder)
    })
}

/// The words of a file name, lowercase and without accents: "Mi_CV-Solución.pdf"
/// gives ["mi", "cv", "solucion", "pdf"]. Splitting on anything that isn't a
/// letter or digit means "web" matches "web_final" but not "webcam".
// ponytail: joined words ("TareaFinal") aren't split; split on capitals if missed.
fn words_of(name: &str) -> Vec<String> {
    fold_accents(&name.to_lowercase())
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Removes Spanish accents in both forms a name can arrive in: precomposed
/// ("ó") and decomposed ("o" + a combining accent, common in macOS names).
/// Without this, a decomposed "solución" would split into "solucio" and "n".
pub(crate) fn fold_accents(text: &str) -> String {
    text.chars()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// Plans where each loose file of `root` goes. Files in subfolders are
/// ignored, so a second run plans nothing. Changes nothing on disk.
pub fn plan_organize(
    root: &Path,
    files: &[FileInfo],
    by: OrganizeBy,
    now: SystemTime,
) -> OrganizePlan {
    let mut plan = OrganizePlan::default();
    // The destinations planned so far (see `reason_to_stay`)
    let mut taken: HashSet<String> = HashSet::new();

    for file in files.iter().filter(|f| f.path.parent() == Some(root)) {
        // The real name, not `FileInfo::name()`, which gives up on non-UTF-8 names
        let Some(name) = file.path.file_name() else {
            continue;
        };
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Some(folder) = folder_for(file, by, now) else {
            continue;
        };
        let to = root.join(folder).join(name);
        match reason_to_stay(file, &to, now, &mut taken) {
            Some(reason) => plan.skipped.push((file.path.clone(), reason)),
            None => plan.moves.push(PlannedMove {
                file: file.clone(),
                to,
            }),
        }
    }
    plan.moves.sort_by(|a, b| a.to.cmp(&b.to));
    plan.skipped.sort_by(|a, b| a.0.cmp(&b.0));
    plan
}

/// Why `file` can't be planned to move to `to`, or `None` if it can; then
/// `to` joins `taken`, the destinations already planned.
fn reason_to_stay(
    file: &FileInfo,
    to: &Path,
    now: SystemTime,
    taken: &mut HashSet<String>,
) -> Option<MoveSkip> {
    if IN_PROGRESS_EXTENSIONS.contains(&file.extension_or("")) || recently_changed(file, now) {
        return Some(MoveSkip::InProgress);
    }
    if fs::symlink_metadata(to).is_ok() {
        return Some(MoveSkip::DestinationExists);
    }
    // Lowercase: the default macOS disk treats "A.jpg" and "a.jpg" as the same name
    let is_free = taken.insert(to.to_string_lossy().to_lowercase());
    (!is_free).then_some(MoveSkip::SameDestination)
}

/// Moves every file of `plan`, checking each one first.
pub fn apply_plan(plan: &OrganizePlan, now: SystemTime, on_progress: Progress) -> MoveReport {
    apply_with(plan, now, on_progress, &move_no_overwrite)
}

/// `apply_plan` with the move step passed in, so tests can make it fail.
pub fn apply_with(
    plan: &OrganizePlan,
    now: SystemTime,
    on_progress: Progress,
    mv: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> MoveReport {
    let mut report = MoveReport::default();
    on_progress(0, plan.moves.len());
    for (done, planned) in plan.moves.iter().enumerate() {
        let from = &planned.file.path;
        let outcome = recheck(planned, now).and_then(|()| {
            let folder = planned.to.parent().expect("destination is inside root");
            fs::create_dir_all(folder)
                .and_then(|()| mv(from, &planned.to))
                .map_err(refused)
        });
        match outcome {
            Ok(()) => report.moved.push((from.clone(), planned.to.clone())),
            Err(skip) => report.skipped.push((from.clone(), skip)),
        }
        on_progress(done + 1, plan.moves.len());
    }
    report
}

/// The reason to report when the system refuses a move. A name that got
/// taken in the meantime has its own: nothing is ever overwritten.
fn refused(err: io::Error) -> MoveSkip {
    match err.kind() {
        io::ErrorKind::AlreadyExists => MoveSkip::DestinationExists,
        _ => MoveSkip::CouldNotMove(err.to_string()),
    }
}

/// Checks from disk that the file is still the one that was planned.
fn recheck(planned: &PlannedMove, now: SystemTime) -> Result<(), MoveSkip> {
    let current = planned
        .file
        .unchanged_on_disk()
        .ok_or(MoveSkip::ChangedSinceScan)?;
    if recently_changed(&current, now) {
        return Err(MoveSkip::InProgress);
    }
    Ok(())
}

/// Content or details changed moments ago (or the date is in the future).
/// `changed` counts: a fresh download can carry an old modification date.
fn recently_changed(file: &FileInfo, now: SystemTime) -> bool {
    [file.modified, file.changed]
        .into_iter()
        .flatten()
        .any(|t| {
            now.duration_since(t)
                .map_or(true, |age| age < RECENT_CHANGE)
        })
}

/// Moves `from` to `to`, failing with `AlreadyExists` instead of replacing a
/// file there: `fs::rename` would silently overwrite it. A hard link claims
/// the new name atomically, then the old name is removed.
// ponytail: volumes without hard links (exFAT, FAT32 USB drives) fail here;
// fall back to an exists-check + rename if organizing those is ever needed.
fn move_no_overwrite(from: &Path, to: &Path) -> io::Result<()> {
    fs::hard_link(from, to)?;
    if let Err(err) = fs::remove_file(from) {
        // Leave things as they were: one name, the original
        let _ = fs::remove_file(to);
        return Err(err);
    }
    Ok(())
}

/// The UTC year of `t`, or `None` before 1970.
// ponytail: UTC, so a file from Dec 31 late evening (west of UTC) can land in
// the next year; use the local offset if anyone notices.
fn year_of(t: SystemTime) -> Option<i64> {
    let days = (t.duration_since(UNIX_EPOCH).ok()?.as_secs() / 86_400) as i64;
    // Days since 1970-01-01 to a civil year (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    Some(yoe + era * 400 + i64::from(month <= 2))
}

#[cfg(test)]
#[path = "../../tests/core_organize.rs"]
mod tests;
