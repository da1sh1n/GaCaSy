// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Writing down what happened.
//!
//! The launcher is a GUI-subsystem process with no console, so a failed launch
//! has nowhere to complain to. `logs/` is the answer, and it holds two things:
//!
//! - `launcher.log`: every attempt and its outcome, with the full OS error text
//!   (the UI only ever gets a short sentence);
//! - `logs/<game>/out.log` and `err.log`: the game's own stdout/stderr, so a
//!   game that prints why it died leaves that behind too.
//!
//! Logging never fails loudly: a launcher that panics because it couldn't open
//! its log is strictly worse than one that quietly stops logging. Every
//! function here swallows its errors for that reason.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::catalog::Game;
use crate::constants::MAX_LOG_BYTES;

/// Appends one timestamped line to `logs/launcher.log`. Errors are ignored.
pub fn line(base: &Path, message: &str) {
    let path = base.join("logs").join("launcher.log");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    truncate_if_large(&path);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {}", timestamp(), message);
    }
}

fn truncate_if_large(path: &PathBuf) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() > MAX_LOG_BYTES {
        let _ = fs::write(path, b"-- log truncated --\n");
    }
}

/// Fresh stdout/stderr files for this game, truncated per launch so what's in
/// them is always the current run. Falls back to discarding the output if the
/// files can't be opened — no game goes unlaunched over a log.
pub fn game_output(base: &Path, game: &Game, index: usize) -> (Stdio, Stdio) {
    let dir = base.join("logs").join(slug(&game.name, index));
    if fs::create_dir_all(&dir).is_err() {
        return (Stdio::null(), Stdio::null());
    }
    let open = |name: &str| {
        File::create(dir.join(name))
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null())
    };
    (open("out.log"), open("err.log"))
}

/// A game's name reduced to a folder name: lowercase, `[a-z0-9]` kept, every
/// run of anything else collapsed to a single `-`. Falls back to the catalog
/// position for a name that survives none of that.
fn slug(name: &str, index: usize) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        format!("game-{index}")
    } else {
        trimmed.to_string()
    }
}

/// `YYYY-MM-DD HH:MM:SSZ` in UTC.
///
/// UTC rather than local time so this stays OS-agnostic. Deliberately a copy of
/// the listener's `log::timestamp` rather than shared code: the two crates are
/// independent by design (see `../../listener/structure.md`), and a date
/// formatter is not worth a shared library.
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

/// Days since the Unix epoch → `(year, month, day)`. Howard Hinnant's
/// `civil_from_days`; see the listener's copy for the full explanation.
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
