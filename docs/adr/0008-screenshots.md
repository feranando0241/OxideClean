# ADR-0008: Finding screenshots left behind after pasting

**Status:** Accepted
**Date:** 2026-11-11
**Deciders:** M4DEN5K (project owner)

## Context

The owner takes screenshots to paste them somewhere, and macOS also saves each one as a file. Eight of them had piled up in one folder. Not saving them in the first place (⌘⌃⇧4, or "Save to: Clipboard" in ⌘⇧5) is the real fix, and the README says so. This feature cleans up what is already there, and what is saved anyway when the shortcut is forgotten.

Two facts about the files decided the design:

- **macOS marks them.** Every screenshot carries the extended attribute `com.apple.metadata:kMDItemIsScreenCapture`, set by the system. It is the same in every language and survives renaming. All eight files on the owner's disk had it.
- **The names aren't plain text.** Since macOS 13 there is a narrow no-break space (U+202F) before AM/PM. The name depends on the system language ("Captura de pantalla … a las …"), and was "Screen Shot" before macOS 12.

## Decision

`--screenshots [DAYS]` (default **1 day**) lists them, biggest first, and changes nothing. `--trash` asks which to move (`y` for all, numbers for some). `--trash --yes` moves every one listed. In the menu it is option 11. Everything goes to the Trash, never straight to deletion.

**How a screenshot is recognized**, checking only image and video formats (`png`, `jpg`, `heic`, `tiff`, `gif`, `pdf`, `bmp`, `mov`, `mp4`):
1. It has the mark.
2. Or its name *starts* with a name macOS gives captures, followed by a date: `Screenshot 2026-…`, `Screen Shot 2026-…`, `Captura de pantalla 2026-…`, `Screen Recording 2026-…`, `Grabación de pantalla 2026-…`. Requiring the date keeps `Screenshot ideas.png` out, and matching only the start avoids the U+202F. Accents match in either Unicode form. These are flagged `(name)` in the list, because nothing but the name vouches for them.

**Old enough** means both the content (mtime) and the details (ctime) changed more than DAYS ago. A screenshot copied here recently, or with no date or one in the future, isn't listed.

**Photo libraries are never entered** (`.photoslibrary`, `.photolibrary`, `.aplibrary`). Screenshots imported into Photos keep the mark, and removing a file from inside a library damages it.

**Right before each move, from disk:** it is still a regular file with the size and modification date from the list, and still old enough. An edit since the list (in Preview, for example) makes it recent, so it stays.

**Reading the mark.** Rust's standard library has no way to read extended attributes. One function calls `libc::getxattr` with a null buffer, which only asks whether the attribute exists. This is the project's only `unsafe` code: eight lines, macOS only, that read and never write. `libc` was already compiled (through `sha2`), so it adds no new crate. Off macOS only names are recognized.

## Options Considered

| Option | Verdict |
|---|---|
| The `xattr` crate | Rejected: it brings `rustix` and its dependencies for a single yes/no question |
| Running `mdls` or `xattr` for each file | Rejected: one process per image is slow on a big folder, and depends on output that isn't a stable interface |
| Names only | Rejected as the main rule: depends on the language and misses renamed files. Kept as the fallback |
| The mark only | Rejected: AirDrop and downloads lose it |
| A generic `--name <PATTERN> --trash` | Rejected: a typo or a `*` reaches every file |
| Using the date in the name | Rejected: it is local time and stays the same in copies. The file's dates are used, as everywhere else |
| Changing the screenshot settings for the user | Out of scope: a tool that cleans files shouldn't change system settings |

## Consequences

- It isn't verified yet that macOS marks screen recordings. They are found by name either way.
- Files sent from another Mac by AirDrop are listed only if their name matches.
- `--yes` moves screenshots you meant to keep, if they are old enough. That is why it needs `--trash` too, and why they go to the Trash.
- Photo libraries are skipped only here. `--dupes` and `--old` still enter them: fixing that in the scanner is pending, separately.
- **What the tests prove:** names in both languages, with U+202F and decomposed accents, and the look-alikes that must not match; the mark read from a real file whatever its name; the age rules; libraries skipped; and that a screenshot edited, removed or redated after the list stays. **What they don't:** a real screenshot more than a day old moved end to end. Its ctime can't be set back, so the tests judge from a `now` far in the future, and they never touch the real Trash.
