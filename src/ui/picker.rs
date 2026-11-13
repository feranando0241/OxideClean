//! Paginated, searchable list for picking one document.

use crate::core::models::FileInfo;
use crate::ui::input::read_line_from_user;
use crate::ui::style::{bold, dim, error, folder_of, hint, plural, prompt, truncate};
use std::path::Path;

/// Documents shown per page.
const PAGE_SIZE: usize = 20;

/// Displays a paginated file list (20 per page) and lets the user
/// navigate pages or search by name before selecting.
///
/// Returns the chosen file, or `None` if the user cancels.
pub fn select_file_paginated<'a>(root: &Path, files: &[&'a FileInfo]) -> Option<&'a FileInfo> {
    let mut current_page: usize = 0;
    // What is listed: every document, or only the ones a search found
    let mut listed: Vec<&'a FileInfo> = files.to_vec();
    let mut searching = false;

    loop {
        let pages = listed.len().div_ceil(PAGE_SIZE).max(1);
        let page = current_page.min(pages - 1);
        print_page(root, &listed, page, pages, searching);
        print_choices(page, pages, searching);
        prompt();

        match read_line_from_user().to_lowercase().as_str() {
            "0" | "cancel" => return None,

            "n" if page + 1 < pages => current_page = page + 1,
            "p" if page > 0 => current_page = page - 1,

            "s" | "b" => match ask_search(files) {
                None => {
                    hint("Empty search, showing all documents.");
                    listed = files.to_vec();
                    searching = false;
                }
                Some((query, found)) if found.is_empty() => {
                    error(&format!(
                        "No document name contains '{}'. Showing all again.",
                        query
                    ));
                    listed = files.to_vec();
                    searching = false;
                    current_page = 0;
                }
                Some((_, found)) => {
                    listed = found;
                    searching = true;
                    current_page = 0;
                }
            },

            "r" => {
                listed = files.to_vec();
                searching = false;
                current_page = 0;
            }

            // Anything else should be the number of a document
            raw => match raw.parse::<usize>() {
                Ok(number) if (1..=listed.len()).contains(&number) => {
                    let chosen = listed[number - 1];
                    println!("Picked: {}", bold(chosen.name()));
                    return Some(chosen);
                }
                Ok(_) => error(&format!("Pick a number from 1 to {}.", listed.len())),
                Err(_) => {
                    error("That's not an option. Type a number, or one of the letters shown.")
                }
            },
        }
    }
}

/// One page of `listed`: how many there are, then each document with the
/// number that picks it.
fn print_page(root: &Path, listed: &[&FileInfo], page: usize, pages: usize, searching: bool) {
    let shown = if searching {
        format!("{} matching your search", listed.len())
    } else {
        plural(listed.len(), "document", "documents")
    };
    let page_info = if pages > 1 {
        format!(" · page {} of {}", page + 1, pages)
    } else {
        String::new()
    };
    hint(&format!("\n{}{}", shown, page_info));

    let first = page * PAGE_SIZE;
    for (i, file) in listed.iter().enumerate().skip(first).take(PAGE_SIZE) {
        println!(
            "  {:>3}  {:<34}  {}",
            i + 1,
            truncate(file.name(), 34),
            dim(&truncate(&folder_of(file, root), 30)),
        );
    }
    println!();
}

/// What can be typed on this page. Moving between pages is only offered
/// when there is a page to go to.
fn print_choices(page: usize, pages: usize, searching: bool) {
    println!("  • Type a number   to pick that document");
    if page + 1 < pages {
        println!("  • Type n          for the next page");
    }
    if page > 0 {
        println!("  • Type p          for the previous page");
    }
    if searching {
        println!("  • Type r          to show all documents again");
    } else {
        println!("  • Type s          to search by name");
    }
    println!("  • Type 0          to cancel");
}

/// Asks for part of a name. `None` if nothing was typed; otherwise what was
/// typed, in lowercase, with the documents whose name contains it (maybe none).
fn ask_search<'a>(files: &[&'a FileInfo]) -> Option<(String, Vec<&'a FileInfo>)> {
    println!("Type part of the name:");
    prompt();
    let query = read_line_from_user().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let found = files
        .iter()
        .filter(|f| f.name().to_lowercase().contains(&query))
        .copied()
        .collect();
    Some((query, found))
}
