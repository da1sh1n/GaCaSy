// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Writes `logs/launcher.log`, and opens the per-game `out.log` and `err.log`
//! a launched process is given. Errors are discarded.

// ########## LOGS ##########

use std::fs::{self, File};
use std::path::Path;
use std::process::Stdio;

use crate::catalog::Game;
use crate::constants::MAX_LOG_BYTES;

/// Appends one timestamped line to `logs/launcher.log` under `base`. Errors are
/// ignored, and the file is truncated once it passes `MAX_LOG_BYTES`.
pub fn line(base: &Path, message: &str) {
    common::log::appendLine(
        &base.join("logs").join("launcher.log"),
        message,
        MAX_LOG_BYTES,
    );
}

/// Fresh stdout/stderr files for `game`, so a game that prints why it died
/// leaves that behind. Truncated per launch, so what is in them is always the
/// current run.
///
/// Falls back to discarding the output when the files cannot be opened — no
/// game goes unlaunched over a log. `index` is the catalog position, used only
/// to name a folder for a game whose title reduces to nothing.
pub fn gameOutput(base: &Path, game: &Game, index: usize) -> (Stdio, Stdio) {
    let dir = base.join("logs").join(slug(&game.name, index));
    if fs::create_dir_all(&dir).is_err() {
        return (Stdio::null(), Stdio::null());
    }
    // A closure because both handles want identical treatment, and `Stdio` is
    // not `Clone` so the two cannot come from one value.
    let open = |name: &str| {
        File::create(dir.join(name))
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null())
    };
    (open("out.log"), open("err.log"))
}

/// A game's name reduced to a folder name: lowercase, `[a-z0-9]` kept, every
/// run of anything else collapsed to a single `-`. Falls back to `game-<index>`
/// for a name that survives none of that.
fn slug(name: &str, index: usize) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        // The `ends_with` test is what collapses a run of punctuation into one
        // dash instead of one dash per character.
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
