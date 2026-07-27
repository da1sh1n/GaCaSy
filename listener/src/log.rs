// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Append-only activity log.
//!
//! The listener is a GUI-subsystem process with no console — on Windows it has
//! no window either — so this file is the *only* way to find out why a
//! cartridge didn't launch. Every ignored volume gets a line here with the
//! reason.
//!
//! Logging never fails loudly: a listener that panics because it couldn't open
//! its log is strictly worse than one that quietly stops logging, so every
//! write error is swallowed.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::settings;

/// Rewrite the log from scratch once it passes this size. The Windows build is
/// resident for a whole login session, so an unbounded append would grow
/// forever on a machine that plugs devices in all day.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

pub struct Log {
    /// `None` when no usable path could be resolved — the listener then runs
    /// silently rather than refusing to start.
    path: Option<PathBuf>,
}

impl Log {
    /// Opens (or creates) the log at `path`, creating parent folders as needed.
    ///
    /// `None` means no log at all, and is honoured as-is. A path that *can't* be
    /// written to is different: that is the listener losing its only voice, so
    /// rather than going quiet it retries at [`settings::fallback_log_path`]. An
    /// *installed* listener never gets here — it lives in
    /// `%LOCALAPPDATA%\GaCaSy`, which is both writable and the very folder the
    /// fallback names — so this is for an exe dropped by hand somewhere
    /// read-only. Only if that fails too does it fall silent.
    pub fn open(path: Option<PathBuf>) -> Log {
        let Some(path) = path else {
            return Log { path: None };
        };
        if let Some(path) = try_open(path) {
            return Log { path: Some(path) };
        }
        Log {
            path: try_open(settings::fallback_log_path()),
        }
    }

    /// A log that discards everything, so the core can be exercised without
    /// touching the filesystem.
    #[cfg(test)]
    pub fn silent() -> Log {
        Log { path: None }
    }

    /// Appends one timestamped line. Errors are deliberately ignored.
    pub fn line(&self, message: &str) {
        let Some(path) = &self.path else {
            return;
        };
        rotate_if_large(path);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{} {}", timestamp(), message);
        }
    }
}

/// Creates `path`'s parent folder and proves the file can be appended to,
/// returning it only if so. Writability is checked once here rather than on
/// every line, because a failing log must not cost anything per event.
fn try_open(path: PathBuf) -> Option<PathBuf> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .is_ok()
        .then_some(path)
}

/// Truncates the log once it exceeds [`MAX_LOG_BYTES`]. Dropping the old
/// content outright (rather than keeping a `.1` copy) is deliberate: this is a
/// troubleshooting trail, not an audit record, and only the recent end of it
/// has ever been useful.
fn rotate_if_large(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() > MAX_LOG_BYTES {
        let _ = fs::write(path, b"-- log truncated --\n");
    }
}

/// `YYYY-MM-DD HH:MM:SSZ` in UTC.
///
/// UTC rather than local time so this stays OS-agnostic — the alternative is a
/// `cfg`-gated `GetLocalTime` on one platform and `libc::localtime_r` on the
/// other, which is a lot of machinery for a log line. The trailing `Z` is there
/// so nobody reads a timestamp as their wall clock.
fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let tod = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Days since the Unix epoch → `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days`, which shifts the era to start in March
/// so the leap day lands at the end of a year and the month-length table
/// collapses into arithmetic. Correct for any date the epoch can express, with
/// no dependency and no platform call.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11], March-based
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // 2000-02-29: the leap year the "divisible by 100" rule almost eats.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        // 2100 is not a leap year.
        assert_eq!(civil_from_days(47_540), (2100, 2, 28));
        assert_eq!(civil_from_days(47_541), (2100, 3, 1));
    }
}
