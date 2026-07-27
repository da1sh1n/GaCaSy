// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The listener's fixed settings, and where its log goes.
//!
//! # There is no config file
//!
//! There used to be a `config.toml` beside the exe. It held two things: the list
//! of cartridge keys this PC trusted, and these tunables.
//!
//! The key list is gone because trust is now cryptographic and compiled in — a
//! list of trusted keys in a writable file beside the exe would have let
//! anything able to edit that file grant itself auto-run on every USB insert,
//! which is precisely the capability the signature exists to deny (see
//! `../build.rs`). And once that was gone, what remained was a debounce window
//! and a log path, neither of which anyone has ever needed to change. A file
//! that exists to be found, read, and left alone is a file worth deleting.
//!
//! So there is no config file and no flag for these. Changing one means a
//! rebuild, which is the right bar for a value nobody should have to touch —
//! and it took the crate's last non-Windows dependency with it, since `toml`
//! was here and in the retired marker parser and nowhere else.

use std::env;
use std::path::{Path, PathBuf};

/// How long to ignore repeat arrivals for a drive letter already handled.
///
/// A flaky USB link can fire several add events for one physical connection;
/// without this, each one launches another copy of the launcher. Long enough to
/// swallow that, short enough that deliberately re-plugging a cartridge still
/// works.
pub const DEBOUNCE_SECONDS: u64 = 5;

pub const LOG_FILE: &str = "listener.log";

/// Where the log belongs: beside the exe, so the listener's two files are in one
/// folder you can open.
///
/// One deployment can't honour that — an exe dropped somewhere read-only — so
/// [`crate::log`] falls back to [`fallback_log_path`] when this can't be
/// opened. That is a fallback and not the default.
pub fn default_log_path(dir: &Path) -> PathBuf {
    dir.join(LOG_FILE)
}

/// Where the log goes when the folder beside the exe is read-only.
#[cfg(windows)]
pub fn fallback_log_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("GaCaSy").join(LOG_FILE)
}

#[cfg(not(windows))]
pub fn fallback_log_path() -> PathBuf {
    let base = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(env::temp_dir);
    base.join("gacasy").join(LOG_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_belongs_beside_the_exe_not_off_in_appdata() {
        // Carried over from the config tests: the default is the folder the
        // listener lives in, and %LOCALAPPDATA% is only ever the fallback.
        let dir = Path::new("C:\\Program Files\\GaCaSy");
        assert_eq!(default_log_path(dir), dir.join("listener.log"));
        assert_ne!(default_log_path(dir), fallback_log_path());
    }

    #[test]
    fn the_fallback_is_an_absolute_path_somewhere_writable() {
        let fallback = fallback_log_path();
        assert!(fallback.is_absolute(), "{}", fallback.display());
        assert!(fallback.ends_with(LOG_FILE));
    }
}
