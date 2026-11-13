//! Command-line argument parsing (see `docs/adr/0001-cli-arguments.md`).
//!
//! Hand-rolled on top of `std::env::args` to avoid a dependency while the
//! interface is small. Move to `clap` once the ADR's revisit triggers are hit.

use crate::core::artifacts::DEFAULT_ARTIFACT_DAYS;
use crate::core::files::DEFAULT_TOP;
use crate::core::old_files::{DEFAULT_OLD_DAYS, days};
use crate::core::organize::OrganizeBy;
use crate::core::screenshots::DEFAULT_SCREENSHOT_DAYS;
use crate::core::similarity::DEFAULT_THRESHOLD;
use std::iter::Peekable;
use std::str::FromStr;

/// Text printed by `--help`.
pub const HELP: &str = "\
OxideClean — file scanner and organizer

USAGE:
    oxide_clean                       Interactive menus
    oxide_clean <PATH>                Scan PATH, show the summary and the action menu
    oxide_clean <PATH> <ACTION>       Run a single action and exit

ACTIONS:
    --largest [N]          Top N largest files (default 10)
    --ext <EXT>            Files with the given extension (e.g. pdf)
    --dupes                List exact duplicates (read-only)
      --keep-in <FOLDER>   ...keep every copy inside FOLDER instead of the oldest
      --trash              ...and move the extra copies to the Trash (asks first)
      --yes, -y            ...without asking
    --same-name            Files that share the same name
    --old [DAYS]           Files neither modified nor read in DAYS days (default 180),
                           biggest first (read-only). Files that arrived on this
                           disk in the last 30 days are never listed
    --artifacts [DAYS]     Build folders (target, node_modules, Python environments)
                           in projects untouched for DAYS days (default 90), with
                           how to rebuild them (read-only)
      --all                ...in every project, marking the active ones
      --trash              ...and choose which to move to the Trash
      --yes, -y            ...move forgotten projects without asking, except those
                           with programs in target/release (not with --all)
    --screenshots [DAYS]   Screenshots and screen recordings older than DAYS days
                           (default 1), biggest first (read-only). Found by the
                           mark macOS sets on them, or by their name
      --trash              ...and choose which to move to the Trash
      --yes, -y            ...move all of them without asking
    --organize <BY>        Plan moving the files directly in PATH into folders by
                           year, size, type or words (project, tarea, cv, pdf…).
                           Subfolders and hidden files are left alone (read-only)
      --apply              ...and move them (asks first). Nothing is overwritten,
                           and each move is logged in PATH/.oxide_clean_organize.tsv
      --yes, -y            ...without asking
    --undo                 Put back every file --organize --apply moved in PATH,
                           newest first, never overwriting (asks first)
      --yes, -y            ...without asking
    --similar <FILE>       Documents with content similar to FILE
      --threshold <N>      Minimum similarity percentage (default 30)

OPTIONS:
    --exclude <FOLDER>     Leave FOLDER out of the scan (can be repeated)
    --no-cache             Don't reuse or save file hashes between runs
    -h, --help             Show this help

FOLDER can be relative to PATH (e.g. --keep-in Photos) or a full path.

File hashes are cached in ~/Library/Caches/oxide_clean (macOS) or
~/.cache/oxide_clean, or under $XDG_CACHE_HOME if set, so a second scan of the
same folder skips unchanged files. Moving files to the Trash never relies on it.

--old uses the dates the file system keeps. Copies made without keeping dates
(plain cp, Finder) look new, and so does a file whose details changed in the
last 30 days. It errs towards listing less. See docs/adr/0004.

Colors are disabled when the output is not a terminal or NO_COLOR is set.
";

/// A single non-interactive action requested on the command line.
#[derive(Debug, PartialEq)]
pub enum Action {
    Largest(usize),
    Ext(String),
    Dupes {
        trash: bool,
        assume_yes: bool,
        /// Keep the copy inside this folder rather than the oldest.
        keep_in: Option<String>,
    },
    SameName,
    /// Files untouched for more than this many days (validated: at least 1).
    Old(u64),
    /// Build folders of projects untouched for more than `days`; with `all`,
    /// of every project, and `days` only decides which ones count as active.
    Artifacts {
        days: u64,
        all: bool,
        trash: bool,
        assume_yes: bool,
    },
    Similar {
        file: String,
        threshold: f64,
    },
    /// Screenshots older than `days`, optionally moved to the Trash.
    Screenshots {
        days: u64,
        trash: bool,
        assume_yes: bool,
    },
    /// Move the loose files of PATH into folders; with `apply: false`, only
    /// show where each would go.
    Organize {
        by: OrganizeBy,
        apply: bool,
        assume_yes: bool,
    },
    /// Put back what `Organize` moved, from the log in PATH.
    Undo {
        assume_yes: bool,
    },
}

/// Options that apply to the whole run, whatever the action (or the menu).
/// New flags of this kind go here, so the functions that receive them don't
/// change signature every time.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOptions {
    /// False with `--no-cache`.
    pub use_cache: bool,
    /// Folders left out of the scan (`--exclude`), as typed.
    pub exclude: Vec<String>,
}

impl Default for RunOptions {
    fn default() -> RunOptions {
        RunOptions {
            use_cache: true,
            exclude: Vec::new(),
        }
    }
}

/// What the program should do, as decided by the arguments.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// No arguments: the classic interactive menus.
    Interactive,
    Help,
    /// Scan `path`; run `action` and exit, or open the action menu when `None`.
    Run {
        path: String,
        action: Option<Action>,
        options: RunOptions,
    },
}

/// Parses the arguments (without the program name). `Err` holds a usage error message.
pub fn parse_args(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Ok(Command::Interactive);
    }

    let mut path: Option<String> = None;
    let mut action: Option<Action> = None;
    let mut options = RunOptions::default();
    let mut modifiers = Modifiers::default();

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        let flag = arg.as_str();
        if matches!(flag, "-h" | "--help") {
            return Ok(Command::Help);
        }
        if let Some(new_action) = action_for(flag, &mut iter)? {
            if action.is_some() {
                return Err("only one action can be used at a time".to_string());
            }
            action = Some(new_action);
            continue;
        }
        if modifiers.take(flag, &mut iter)? || options.take(flag, &mut iter)? {
            continue;
        }
        if flag.starts_with('-') {
            return Err(format!("unknown option '{}'", flag));
        }
        if path.is_some() {
            return Err(format!("unexpected extra argument '{}'", flag));
        }
        path = Some(flag.to_string());
    }

    // Attach modifiers to their action, rejecting them anywhere else
    modifiers.check(action.as_ref())?;
    let action = action.map(|action| modifiers.attach(action));

    let path = path.ok_or("missing <PATH> to scan")?;
    Ok(Command::Run {
        path,
        action,
        options,
    })
}

/// The action `flag` asks for, taking the value that follows it if it has
/// one. `None` if `flag` isn't an action. Its modifiers start out off: they
/// are attached once every argument has been read (`Modifiers::attach`).
fn action_for<'a, I: Iterator<Item = &'a String>>(
    flag: &str,
    iter: &mut Peekable<I>,
) -> Result<Option<Action>, String> {
    let action = match flag {
        "--largest" => Action::Largest(optional_number(iter).unwrap_or(DEFAULT_TOP)),
        "--ext" => Action::Ext(take_value(iter, flag)?),
        "--dupes" => Action::Dupes {
            trash: false,
            assume_yes: false,
            keep_in: None,
        },
        "--same-name" => Action::SameName,
        "--old" => Action::Old(optional_days(iter, flag, DEFAULT_OLD_DAYS)?),
        "--artifacts" => Action::Artifacts {
            days: optional_days(iter, flag, DEFAULT_ARTIFACT_DAYS)?,
            all: false,
            trash: false,
            assume_yes: false,
        },
        "--screenshots" => Action::Screenshots {
            days: optional_days(iter, flag, DEFAULT_SCREENSHOT_DAYS)?,
            trash: false,
            assume_yes: false,
        },
        "--organize" => Action::Organize {
            by: organize_by(&take_value(iter, flag)?)?,
            apply: false,
            assume_yes: false,
        },
        "--undo" => Action::Undo { assume_yes: false },
        "--similar" => Action::Similar {
            file: take_value(iter, flag)?,
            threshold: DEFAULT_THRESHOLD,
        },
        _ => return Ok(None),
    };
    Ok(Some(action))
}

/// The flags that change what an action does. They can come before or after
/// their action, so they are collected first and attached at the end.
#[derive(Default)]
struct Modifiers {
    trash: bool,
    assume_yes: bool,
    keep_in: Option<String>,
    threshold: Option<f64>,
    all: bool,
    apply: bool,
}

impl Modifiers {
    /// Records `flag` if it is a modifier, taking the value that follows it
    /// if it has one. False if it isn't a modifier.
    fn take<'a>(
        &mut self,
        flag: &str,
        iter: &mut impl Iterator<Item = &'a String>,
    ) -> Result<bool, String> {
        match flag {
            "--trash" => self.trash = true,
            "--yes" | "-y" => self.assume_yes = true,
            "--all" => self.all = true,
            "--apply" => self.apply = true,
            "--keep-in" => self.keep_in = Some(take_value(iter, flag)?),
            "--threshold" => self.threshold = Some(percentage(&take_value(iter, flag)?)?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Rejects a modifier given without the action it belongs to.
    fn check(&self, action: Option<&Action>) -> Result<(), String> {
        let is_dupes = matches!(action, Some(Action::Dupes { .. }));
        let is_artifacts = matches!(action, Some(Action::Artifacts { .. }));
        let is_screenshots = matches!(action, Some(Action::Screenshots { .. }));
        let is_organize = matches!(action, Some(Action::Organize { .. }));
        let is_undo = matches!(action, Some(Action::Undo { .. }));
        let is_similar = matches!(action, Some(Action::Similar { .. }));

        let problem = if self.trash && !is_dupes && !is_artifacts && !is_screenshots {
            "--trash can only be used with --dupes, --artifacts or --screenshots"
        } else if self.keep_in.is_some() && !is_dupes {
            "--keep-in can only be used with --dupes"
        } else if self.apply && !is_organize {
            "--apply can only be used with --organize"
        } else if self.assume_yes && !self.trash && !self.apply && !is_undo {
            "--yes can only be used with --trash, --apply or --undo"
        } else if self.all && self.assume_yes {
            "--yes can't be used with --all: active projects must be chosen"
        } else if self.all && !is_artifacts {
            "--all can only be used with --artifacts"
        } else if self.threshold.is_some() && !is_similar {
            "--threshold can only be used with --similar"
        } else {
            return Ok(());
        };
        Err(problem.to_string())
    }

    /// `action` with its modifiers filled in. Call `check` first.
    fn attach(self, action: Action) -> Action {
        let Modifiers {
            trash,
            assume_yes,
            keep_in,
            threshold,
            all,
            apply,
        } = self;
        match action {
            Action::Dupes { .. } => Action::Dupes {
                trash,
                assume_yes,
                keep_in,
            },
            Action::Similar { file, .. } => Action::Similar {
                file,
                threshold: threshold.unwrap_or(DEFAULT_THRESHOLD),
            },
            Action::Artifacts { days, .. } => Action::Artifacts {
                days,
                all,
                trash,
                assume_yes,
            },
            Action::Organize { by, .. } => Action::Organize {
                by,
                apply,
                assume_yes,
            },
            Action::Screenshots { days, .. } => Action::Screenshots {
                days,
                trash,
                assume_yes,
            },
            Action::Undo { .. } => Action::Undo { assume_yes },
            // These take no modifiers. Listed one by one so that a new action
            // has to say here which ones it takes.
            Action::Largest(_) | Action::Ext(_) | Action::SameName | Action::Old(_) => action,
        }
    }
}

impl RunOptions {
    /// Records `flag` if it is one of these options, taking the value that
    /// follows it if it has one. False if it isn't one.
    fn take<'a>(
        &mut self,
        flag: &str,
        iter: &mut impl Iterator<Item = &'a String>,
    ) -> Result<bool, String> {
        match flag {
            "--exclude" => self.exclude.push(take_value(iter, flag)?),
            "--no-cache" => self.use_cache = false,
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// Takes the value that must follow `flag`, e.g. `--ext pdf`.
fn take_value<'a>(
    iter: &mut impl Iterator<Item = &'a String>,
    flag: &str,
) -> Result<String, String> {
    match iter.next() {
        Some(value) if !value.starts_with('-') => Ok(value.clone()),
        _ => Err(format!("{} needs a value", flag)),
    }
}

/// A flag's optional number, like N in `--largest [N]`: the next argument is
/// only taken if it is a number.
fn optional_number<'a, T: FromStr, I: Iterator<Item = &'a String>>(
    iter: &mut Peekable<I>,
) -> Option<T> {
    let n = iter.peek().and_then(|v| v.parse::<T>().ok());
    if n.is_some() {
        iter.next();
    }
    n
}

/// A flag's optional number of days, which must be a usable threshold.
fn optional_days<'a, I: Iterator<Item = &'a String>>(
    iter: &mut Peekable<I>,
    flag: &str,
    default: u64,
) -> Result<u64, String> {
    let n = optional_number(iter).unwrap_or(default);
    if n == 0 {
        return Err(format!("{} needs at least 1 day", flag));
    }
    if days(n).is_none() {
        return Err(format!("{} {} is too many days", flag, n));
    }
    Ok(n)
}

/// The value of `--organize`.
fn organize_by(raw: &str) -> Result<OrganizeBy, String> {
    OrganizeBy::from_name(raw).ok_or_else(|| {
        format!(
            "--organize must be year, size, type or words, got '{}'",
            raw
        )
    })
}

/// The value of `--threshold`: a percentage from 0 to 100.
fn percentage(raw: &str) -> Result<f64, String> {
    raw.parse::<f64>()
        .ok()
        .filter(|v| (0.0..=100.0).contains(v))
        .ok_or_else(|| format!("--threshold must be a number from 0 to 100, got '{}'", raw))
}

#[cfg(test)]
#[path = "tests/cli.rs"]
mod tests;
