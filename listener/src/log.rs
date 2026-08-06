// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Resolves where this listener writes its log — beside the exe, or
//! `%LOCALAPPDATA%\Romzeta` when that is not writable, or nowhere — and holds
//! the handle. The writing itself is `common::log`.

// ########## THE LISTENER'S LOG ##########

use std::path::{Path, PathBuf};

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
    /// `None` means no log at all and is honoured as-is.
    ///
    /// A path that exists but cannot be written to is different: that is the
    /// listener losing its only voice, so it retries at the fallback under
    /// `%LOCALAPPDATA%` before falling silent. An installed listener never gets
    /// there — it already lives in that folder.
    pub fn open(path: Option<PathBuf>) -> Log {
        let Some(path) = path else {
            return Log { path: None };
        };
        // Writability is proved once here rather than on every line, so a log
        // that cannot be written costs nothing per device event.
        if common::log::openAppendable(&path) {
            return Log { path: Some(path) };
        }
        let fallback = settings::fallbackLogPath();
        Log {
            // `then_some` turns the bool into the Option the field holds.
            path: common::log::openAppendable(&fallback).then_some(fallback),
        }
    }

    /// A log that discards everything, so the core can be exercised without
    /// touching the filesystem. Behind `cfg(test)` so no production path can
    /// build one by accident.
    #[cfg(test)]
    pub fn silent() -> Log {
        Log { path: None }
    }

    /// Where this log is writing, if it resolved a usable path at all. For the
    /// tray menu's "Open log" — appending a line never needs to know this.
    #[cfg(windows)]
    pub fn path(&self) -> Option<&Path> {
        // `as_deref` turns `&Option<PathBuf>` into `Option<&Path>` without
        // cloning the buffer.
        self.path.as_deref()
    }

    /// Appends one timestamped line. Errors are ignored; a log that cannot be
    /// written stops the logging, not the listener.
    pub fn line(&self, message: &str) {
        let Some(path) = &self.path else {
            return;
        };
        common::log::appendLine(path, message, MAX_LOG_BYTES);
    }
}
