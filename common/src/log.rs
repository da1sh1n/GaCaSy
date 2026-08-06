// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Appends timestamped lines to a log file, and truncates it once it passes a
//! size cap. Every error is discarded.

// ########## LOG FILE PLUMBING ##########

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::time;

/// Appends `message` to the file at `path`, prefixed with a UTC timestamp.
/// Creates the parent folder first, and truncates the file if it has grown past
/// `max_bytes`. Every error is discarded, so this cannot fail from the caller's
/// point of view.
pub fn appendLine(path: &Path, message: &str, max_bytes: u64) {
    // `parent()` is None only for a bare filename, which has nothing to create.
    if let Some(parent) = path.parent() {
        // `let _ =` deliberately drops the Result. The folder normally exists,
        // and if it cannot be made the open below fails and is ignored too.
        let _ = fs::create_dir_all(parent);
    }
    rotateIfLarge(path, max_bytes);
    // `.append(true)` makes every write seek to the end first, so two processes
    // logging at once interleave whole lines instead of overwriting each other.
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        // `writeln!` returns a Result for a disk that filled up mid-line; there
        // is nothing useful to do about that from inside the logger.
        let _ = writeln!(file, "{} {}", time::timestamp(), message);
    }
}

/// Replaces the file at `path` with a single marker line once it grows past
/// `max_bytes`. A no-op when the file does not exist yet or is still small.
pub fn rotateIfLarge(path: &Path, max_bytes: u64) {
    // `let else` exits early on the common first-run case: no file, no metadata,
    // nothing to trim. Its body must diverge, which is why it is a `return`.
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() > max_bytes {
        // The old content goes rather than moving to a `.1` copy — this is a
        // troubleshooting trail, and only its recent end has ever been useful.
        let _ = fs::write(path, b"-- log truncated --\n");
    }
}

/// Creates `path`'s parent folder and proves the file can be appended to,
/// handing it back only if so. Checked once at startup rather than per line, so
/// a log that cannot be written costs nothing on every later event.
pub fn openAppendable(path: &Path) -> bool {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .is_ok()
}
