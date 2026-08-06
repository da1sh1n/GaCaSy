// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The crate's fixed constants — the arrival debounce window and the log file
//! name — and the two functions that build the log's path.

// ########## FIXED SETTINGS ##########

use std::env;
use std::path::{Path, PathBuf};

/// How long to ignore repeat arrivals for a drive letter already handled. A
/// flaky USB link fires several add events for one physical connection.
pub const DEBOUNCE_SECONDS: u64 = 5;

pub const LOG_FILE: &str = "listener.log";

/// The log beside the exe in `dir`, so the listener's two files sit in one
/// folder you can open.
pub fn defaultLogPath(dir: &Path) -> PathBuf {
    dir.join(LOG_FILE)
}

/// Where the log goes when the folder beside the exe turns out to be read-only.
/// A fallback, not the default — `crate::log` only reaches for it after the
/// preferred path has failed to open.
#[cfg(windows)]
pub fn fallbackLogPath() -> PathBuf {
    // `var_os`, not `var`: a profile path that is not valid UTF-16 is still a
    // perfectly good path to hand back to the filesystem.
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("Romzeta").join(LOG_FILE)
}

#[cfg(not(windows))]
pub fn fallbackLogPath() -> PathBuf {
    // The XDG spec's state directory, with its documented default under `HOME`.
    let base = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(env::temp_dir);
    base.join("romzeta").join(LOG_FILE)
}
