//! The log of the moves made by organizing, and undoing them from it.

use super::{MoveReport, MoveSkip, move_no_overwrite, refused};
use crate::core::Progress;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The log of every move, kept hidden inside the organized folder so it
/// travels with it and is never organized itself.
pub const LOG_NAME: &str = ".oxide_clean_organize.tsv";

/// Appends one `from<TAB>to` line per move to `root/LOG_NAME`, creating it
/// if needed, so every move can be undone. Paths are relative to `root`, so
/// the log still works if the folder is moved or reached by another path.
// ponytail: paths are written with `display()`, so a name with a tab, a
// newline or invalid UTF-8 can't be undone from the log; escape if one shows up.
pub fn append_log(root: &Path, moved: &[(PathBuf, PathBuf)]) -> io::Result<()> {
    use std::io::Write;
    let rel = |path: &PathBuf| {
        path.strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string()
    };
    let lines: String = moved
        .iter()
        .map(|(from, to)| format!("{}\t{}\n", rel(from), rel(to)))
        .collect();
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(LOG_NAME))?
        .write_all(lines.as_bytes())
}

/// The moves a log's text records, one per line, oldest first.
fn log_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|line| !line.is_empty())
}

/// How many moves the log of `root` holds; 0 when there is no log.
pub fn logged_moves(root: &Path) -> io::Result<usize> {
    match fs::read_to_string(root.join(LOG_NAME)) {
        Ok(text) => Ok(log_lines(&text).count()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(err),
    }
}

/// Moves every file in the log of `root` back where it was, newest first,
/// never overwriting. Undone lines leave the log (the log goes when empty);
/// the rest stay, so undoing again retries them. Folders left empty are
/// removed. `moved` in the report holds `(where it was, where it is back)`.
pub fn undo_organize(root: &Path, on_progress: Progress) -> io::Result<MoveReport> {
    undo_with(root, on_progress, &move_no_overwrite)
}

/// `undo_organize` with the move step passed in, so tests can make it fail.
pub fn undo_with(
    root: &Path,
    on_progress: Progress,
    mv: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<MoveReport> {
    let log = root.join(LOG_NAME);
    let text = fs::read_to_string(&log)?;
    let lines: Vec<&str> = log_lines(&text).collect();
    let mut report = MoveReport::default();
    let mut kept: Vec<&str> = Vec::new();
    on_progress(0, lines.len());
    for (done, line) in lines.iter().rev().enumerate() {
        match restore(root, line, mv) {
            Ok(pair) => report.moved.push(pair),
            Err(skip) => {
                kept.push(line);
                report.skipped.push(skip);
            }
        }
        on_progress(done + 1, lines.len());
    }

    // A failed rewrite only leaves undone lines behind, and undoing those
    // again finds nothing to move (`Gone`), so it isn't worth failing over.
    kept.reverse();
    let _ = if kept.is_empty() {
        fs::remove_file(&log)
    } else {
        fs::write(&log, kept.join("\n") + "\n")
    };
    // `remove_dir` refuses any folder that isn't empty
    for (was, _) in &report.moved {
        if let Some(folder) = was.parent() {
            let _ = fs::remove_dir(folder);
        }
    }
    Ok(report)
}

/// Undoes one log line: `name<TAB>folder/name` puts `root/folder/name` back
/// at `root/name`.
fn restore(
    root: &Path,
    line: &str,
    mv: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(PathBuf, PathBuf), (PathBuf, MoveSkip)> {
    let (from, to) = line
        .split_once('\t')
        .map(|(a, b)| (Path::new(a), Path::new(b)))
        .ok_or_else(|| (root.join(line), MoveSkip::NotFromThisFolder))?;
    // The log is a plain file anyone can edit: only act on the exact shapes
    // organizing writes, so a line can never reach outside `root`.
    let plain = |path: &Path, depth: usize| {
        path.components().count() == depth
            && path.components().all(|c| matches!(c, Component::Normal(_)))
    };
    if !plain(from, 1) || !plain(to, 2) || from.file_name() != to.file_name() {
        return Err((root.join(to), MoveSkip::NotFromThisFolder));
    }
    let (from, to) = (root.join(from), root.join(to));
    if !fs::symlink_metadata(&to).is_ok_and(|m| m.is_file()) {
        return Err((to, MoveSkip::Gone));
    }
    match mv(&to, &from) {
        Ok(()) => Ok((to, from)),
        Err(err) => Err((to, refused(err))),
    }
}
