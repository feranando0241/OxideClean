# ADR-0007: Organizing files into folders

**Status:** Accepted
**Date:** 2026-11-08
**Deciders:** M4DEN5K (project owner)

## Context

Every earlier feature either only reads, or moves things to the Trash, where the system keeps a way back. Organizing moves files to new places on purpose. There is no Trash to undo it, and a mistake breaks things quietly: a project whose files are pulled out of their folders, a photo library emptied, a file replaced by another with the same name.

Three facts shaped the design:

- **`fs::rename` overwrites.** On Unix it replaces whatever is at the destination without a word.
- **Some folders are really files.** On macOS, `.photoslibrary`, `.pages` and `.key` are folders. The scanner doesn't skip them, so they can't be told apart from user folders by their contents.
- **Dates and names aren't clean.** A fresh download can carry an old modification date. macOS often stores accented names decomposed (`o` + a combining accent). Some names aren't valid UTF-8.

## Decision

`--organize year|size|type|words` shows a plan and changes nothing. `--apply` moves, after a y/N question, or without one with `--yes`. Without a terminal and without `--yes`, nothing moves. In the menu it is option 10, which always shows the plan and asks first.

**Only the files directly in the folder.** Subfolders are never entered, so projects and bundles stay whole, and a second run plans nothing because the new folders aren't touched. This also means organizing `~` reads one folder, not the whole disk. Hidden files (`.zshrc`) stay where they are.

**The criteria:**

| Criterion | Folder | From |
|---|---|---|
| `year` | `2024`, `Unknown year` | Modification date in UTC. Missing, pre-1970 or future dates are unknown |
| `size` | `Under 1 MB`, `1 MB to 100 MB`, `100 MB to 1 GB`, `Over 1 GB` | Logical size |
| `type` | `Images`, `Videos`, `Audio`, `Documents`, `Archives`, `Code`, `Other` | A closed list of extensions |
| `words` | `Projects`, `Homework`, `Websites`, `Solutions`, `Resumes`, then `PDF`, `CSV` | Words in the name (English and Spanish), then the real extension |

With `words`, a file whose name has none of the words, and isn't a PDF or CSV, stays where it is. Words are whole (`web` doesn't match `webcam`), lowercase and without accents in either Unicode form. The first rule in the table wins, so topics come before the extension folders. Joined words (`TareaFinal`) aren't split. Folder names are in English, like the CLI.

**What stays, decided when planning:**
- A file with that name is already in the destination.
- Another file in the same plan goes to the same name, comparing without case, as the default macOS disk does.
- A download in progress (`.crdownload`, `.part`, `.partial`, `.download`, `.tmp`), or a file whose modification *or* change time (ctime) is under 10 minutes old or in the future.

**Right before each move, from disk:** the file is still a regular file with the size and modification date from the plan, and still not recently changed.

**Never overwriting.** A move is a hard link to the new name, which fails atomically if the name is taken, followed by removing the old name. If the removal fails, the new link is removed, so the file keeps one name.

**Undo.** Every move is appended to `.oxide_clean_organize.tsv` inside the organized folder, as `name<TAB>folder/name`, relative to the folder. `--undo` (or `undo` in the menu) puts files back newest first, with the same no-overwrite move, after a y/N question. Undone lines leave the log, and the log is removed when empty. The others stay, so undoing again retries them. Folders left empty are removed (`remove_dir` refuses any other).

The log is a plain file anyone can edit, so undo acts only on lines with the exact shape organizing writes: one plain name and one plain `folder/name` with the same file name, with no `..` and no absolute paths. Any other line is reported and kept, never followed.

## Options Considered

| Option | Verdict |
|---|---|
| Entering subfolders | Rejected: it pulls files out of projects and macOS bundles |
| `fs::rename` after checking the destination | Rejected: another file can appear between the check and the move. Also checked in the plan, but the hard link is what guarantees it |
| Copy + delete when a hard link isn't possible | Rejected for now: moves between volumes and onto exFAT fail and are reported instead |
| Renaming on collision (`photo (2).jpg`) | Rejected: it invents names. The file stays and the reason is shown |
| Leaving unmatched files in `Other` for `words` | Rejected: it moves files that no rule chose |
| Grouping similar names ("by title") | Replaced by the keyword table: predictable and easy to explain |
| A config file or `--rule` for the user's own words | Postponed: the table in the code is enough for now |
| The log in the cache folder | Rejected: kept in the folder, it travels with it and needs no key per folder |
| EXIF dates for photos | Postponed: needs a dependency |

## Consequences

- Moving onto exFAT or FAT32 drives always fails (no hard links), and every file is reported as not moved.
- Years are UTC: a file saved late on December 31, west of UTC, lands in the next year.
- Copies made without keeping dates (plain `cp`, Finder) are sorted by the copy's date, the same limit as ADR-0004.
- Other apps that remember paths (recent documents, photo catalogs) lose track of moved files until they are put back.
- In iCloud Drive or Dropbox folders, every move syncs.
- A name with a tab, a newline or invalid UTF-8 is moved, but its log line can't be undone.
- **What the tests prove:** the plan never moves subfolder or hidden files, never overwrites (including a file that appears after planning), never moves a file changed after planning, keeps non-UTF-8 names exactly, and plans nothing on a second run. Undo puts everything back without overwriting, keeps what it couldn't undo in the log, and ignores lines that point outside the folder. **What they don't:** the 10-minute rule against real old files end to end, because a file's ctime can't be set back. The tests judge from a `now` far in the future instead.
