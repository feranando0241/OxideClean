//! Reading what the user types and turning it into paths, answers and
//! selections.

use crate::core::paths::folder_in_scan;
use crate::ui::style::{hint, prompt};
use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Reads a line from standard input (keyboard) and trims whitespace and newlines.
pub fn read_line_from_user() -> String {
    // Flush so prompts written with `print!` appear before the user types
    let _ = io::stdout().flush();
    let mut buffer = String::new();
    let bytes_read = io::stdin()
        .read_line(&mut buffer)
        .expect("Failed to read user input");
    // End of input (Ctrl+D or a closed pipe): exit instead of looping on empty answers
    if bytes_read == 0 {
        println!();
        std::process::exit(0);
    }
    buffer.trim().to_string()
}

/// True if `input` is one of `options`, ignoring ASCII case.
pub fn is_one_of(input: &str, options: &[&str]) -> bool {
    options.iter().any(|c| input.eq_ignore_ascii_case(c))
}

/// Returns true if the given input matches a quit command.
pub fn is_exit_command(input: &str) -> bool {
    is_one_of(input, &["0", "q", "quit", "exit"])
}

/// True for `y` or `yes`, the only answers that confirm moving something.
pub fn is_yes(input: &str) -> bool {
    is_one_of(input, &["y", "yes"])
}

/// Asks a yes/no `question`, with `how_to_answer` under it. True only for y
/// or yes: Enter, or anything else, leaves things as they are.
pub fn confirm(question: &str, how_to_answer: &str) -> bool {
    println!("{}", question);
    hint(how_to_answer);
    prompt();
    is_yes(&read_line_from_user())
}

/// The user's home folder, if the environment says where it is.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Converts user input into a valid `PathBuf`, expanding `~` to the home directory.
pub fn resolve_user_path(input: &str) -> PathBuf {
    if (input == "~" || input.starts_with("~/"))
        && let Some(mut path) = home_dir()
    {
        if input.len() > 2 {
            path.push(&input[2..]);
        }
        path
    } else if input.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(input)
    }
}

/// A folder typed by the user (relative to `root`, a full path, or under
/// `~`), spelled the way the scanner spells what is inside `root`, so it can
/// be compared with scanned files. It may not exist; callers check `is_dir`.
pub fn resolve_user_folder(root: &Path, typed: &str) -> PathBuf {
    folder_in_scan(root, &resolve_user_path(typed))
}

/// Parses a list of group numbers like `1,3,5-7` (spaces allowed) into
/// zero-based indices, sorted and without repeats. Numbers go from 1 to `max`.
pub fn parse_selection(input: &str, max: usize) -> Result<Vec<usize>, String> {
    let number = |s: &str| -> Result<usize, String> {
        match s.trim().parse::<usize>() {
            Ok(n) if (1..=max).contains(&n) => Ok(n),
            _ => Err(format!(
                "'{}' isn't a group number from 1 to {}.",
                s.trim(),
                max
            )),
        }
    };

    let mut picked = BTreeSet::new();
    for part in input.split(',').filter(|p| !p.trim().is_empty()) {
        match part.split_once('-') {
            Some((from, to)) => {
                let (from, to) = (number(from)?, number(to)?);
                if from > to {
                    return Err(format!("'{}' goes backwards.", part.trim()));
                }
                picked.extend(from..=to);
            }
            None => {
                picked.insert(number(part)?);
            }
        }
    }
    if picked.is_empty() {
        return Err("No group numbers given.".to_string());
    }
    Ok(picked.into_iter().map(|n| n - 1).collect())
}

/// The items a selection like `1,3,5-7` picks out of `items`, numbered from 1
/// as they are listed on screen (see `parse_selection`).
pub fn pick<T: Clone>(input: &str, items: &[T]) -> Result<Vec<T>, String> {
    let indices = parse_selection(input, items.len())?;
    Ok(indices.into_iter().map(|i| items[i].clone()).collect())
}

#[cfg(test)]
#[path = "../tests/ui_input.rs"]
mod tests;
