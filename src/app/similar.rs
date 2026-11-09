//! `--similar`: comparing one document with the others by content.

use super::AppError;
use crate::core::models::FileInfo;
use crate::core::paths::in_scan_form;
use crate::core::similarity::find_similar_documents;
use crate::ui::input::resolve_user_path;
use crate::ui::progress::Spinner;
use crate::ui::report::print_similarity_results;
use std::fs;
use std::path::Path;

/// Builds the `FileInfo` for the `--similar` base file. When the file lives
/// inside `root`, its path is expressed the same way the scanner builds paths
/// (root + relative part) so the comparison recognizes and skips it.
pub(super) fn locate_base_file(root: &Path, file: &str) -> Result<FileInfo, AppError> {
    let path = resolve_user_path(file);
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) => return Err(AppError::UnreadableFile { path, source }),
    };
    if !metadata.is_file() {
        return Err(AppError::NotAFile(path));
    }
    Ok(FileInfo::from_metadata(in_scan_form(root, path), &metadata))
}

/// Compares `target` with the other documents and prints the matches.
pub fn show_similar_documents(
    root: &Path,
    target: &FileInfo,
    files: &[FileInfo],
    threshold: f64,
) -> Result<(), AppError> {
    let matches = {
        let spinner = Spinner::start("Reading documents", "documents");
        find_similar_documents(target, files, threshold, &|done, total| {
            spinner.update(done, total)
        })?
    };
    print_similarity_results(root, target.name(), &matches, threshold);
    Ok(())
}
