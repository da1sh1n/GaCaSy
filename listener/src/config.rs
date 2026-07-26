// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The listener's own `config.toml` — the PC half of the cartridge identity
//! contract (see `../structure.md`, "Cartridge identification system").
//!
//! It sits beside `listener.exe` and is written by the installer, which
//! *appends* to `keys` each time a cartridge is paired.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Baked-in default, used to seed a missing config.toml so a hand-run listener
/// has something to edit. Also documents every key in one place.
const DEFAULT_CONFIG: &str = include_str!("config.toml");

pub const CONFIG_FILE: &str = "config.toml";

/// Fallback for `debounce_seconds`. Long enough to swallow the repeat arrival
/// events a flaky USB link produces, short enough that deliberately
/// re-plugging a cartridge still works.
const DEFAULT_DEBOUNCE_SECONDS: u64 = 5;

pub struct Config {
    /// Every cartridge key this PC trusts, lowercased and trimmed at load time
    /// so the trust check is a plain comparison. **Empty means trust any
    /// cartridge** — see [`Config::trusts`].
    pub keys: Vec<String>,
    /// Where to append activity. `None` means "log nothing".
    pub log_file: Option<PathBuf>,
    pub debounce_seconds: u64,
}

impl Config {
    /// True when `key` (from a `.cartridge` marker) is one this PC trusts.
    ///
    /// **An empty `keys` list trusts every cartridge**, rather than none. That
    /// is what makes a fresh install work the moment you write a `.cartridge`
    /// marker, with no pairing step — the unpaired default is "open", not
    /// "locked". Listing even one key switches the listener to matching only
    /// that list. Note what "open" means in practice: any volume carrying a
    /// valid marker will have its named binary started, so the marker itself is
    /// the only thing standing between a plugged-in disk and a launch.
    ///
    /// Matching is case- and whitespace-insensitive: these keys get copied
    /// between two files by hand often enough that `3F9A` failing to match
    /// `3f9a` would only ever be a support burden. It costs nothing — per the
    /// version note in `../structure.md`, v1 trust is a recognition handshake,
    /// not a security boundary.
    pub fn trusts(&self, key: &str) -> bool {
        if self.keys.is_empty() {
            return true;
        }
        let key = key.trim().to_ascii_lowercase();
        !key.is_empty() && self.keys.contains(&key)
    }
}

/// Reads `config.toml` from `dir`, seeding it from the baked-in default when
/// absent.
///
/// Seeding is best-effort: a listener deployed into `Program Files` runs as a
/// user who cannot write there, so a failure is expected and simply means no
/// file is created. It is worth attempting because it gives every other
/// deployment — `output/`, or any folder the exe was dropped into — a commented
/// config to edit on first run.
///
/// Parsing is deliberately forgiving, key by key, like the launcher's:
/// one wrong-typed value costs that setting only, instead of rejecting the
/// whole file and silently changing which cartridges this PC accepts.
pub fn load(dir: &Path) -> Config {
    let path = dir.join(CONFIG_FILE);
    if !path.exists() {
        let _ = fs::write(&path, DEFAULT_CONFIG);
    }

    let mut config = Config {
        keys: Vec::new(),
        log_file: Some(default_log_path(dir)),
        debounce_seconds: DEFAULT_DEBOUNCE_SECONDS,
    };

    let Ok(contents) = fs::read_to_string(&path) else {
        return config;
    };
    let Ok(table) = contents.parse::<toml::Table>() else {
        return config;
    };

    // Blank entries are kept rather than filtered out. They can never match a
    // marker (`trusts` rejects an empty key outright), but dropping them would
    // turn `keys = [""]` into an empty list — and an empty list now means
    // "trust everything". Silently promoting a locked-down config to an open
    // one over a stray blank string is not a trade worth making.
    if let Some(keys) = table.get("keys").and_then(|v| v.as_array()) {
        config.keys = keys
            .iter()
            .filter_map(|v| v.as_str())
            .map(|k| k.trim().to_ascii_lowercase())
            .collect();
    }

    // An explicit empty string means "don't log"; anything else is a path,
    // resolved against the exe folder when relative.
    if let Some(value) = table.get("log_file").and_then(|v| v.as_str()) {
        let value = value.trim();
        config.log_file = if value.is_empty() {
            None
        } else {
            Some(dir.join(value))
        };
    }

    if let Some(value) = table.get("debounce_seconds").and_then(|v| v.as_integer())
        && value >= 0
    {
        config.debounce_seconds = value as u64;
    }

    config
}

/// Default log location: beside the exe and the config it belongs to, so all
/// three of the listener's files are in one folder you can open.
///
/// One deployment can't honour that — a listener installed into `Program
/// Files` runs as a user with no write access there — so [`crate::log`] falls
/// back to [`fallback_log_path`] when this path can't be opened. That is a
/// fallback and not the default: the folder next to the exe is where the log is
/// meant to be, and where it will be for every deployment that can write.
pub const LOG_FILE: &str = "listener.log";

fn default_log_path(dir: &Path) -> PathBuf {
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

    fn config_with_keys(keys: &[&str]) -> Config {
        Config {
            keys: keys.iter().map(|k| k.to_string()).collect(),
            log_file: None,
            debounce_seconds: 0,
        }
    }

    #[test]
    fn trust_ignores_case_and_padding() {
        let config = config_with_keys(&["3f9a1c"]);
        assert!(config.trusts("3f9a1c"));
        assert!(config.trusts("  3F9A1C "));
        assert!(!config.trusts("b72e04"));
    }

    #[test]
    fn empty_key_is_never_trusted() {
        // A blank `key = ""` in a marker must not match a blank list entry.
        assert!(!config_with_keys(&[""]).trusts(""));
        assert!(!config_with_keys(&["3f9a1c"]).trusts("   "));
    }

    #[test]
    fn an_empty_key_list_trusts_every_cartridge() {
        // The unpaired default is open, not locked: a fresh install works as
        // soon as a volume carries a marker.
        let config = config_with_keys(&[]);
        assert!(config.trusts("3f9a1c"));
        assert!(config.trusts("anything-at-all"));
    }

    #[test]
    fn a_list_of_only_blanks_still_counts_as_a_list() {
        // The trap this guards: if `load` filtered blanks out, `keys = [""]`
        // would collapse to an empty list and silently flip from "trust
        // nothing" to "trust everything".
        let dir = std::env::temp_dir().join("gacasy-config-blanks");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join(CONFIG_FILE), "keys = [\"\"]\n").expect("write config");

        let config = load(&dir);
        assert_eq!(config.keys.len(), 1);
        assert!(!config.trusts("3f9a1c"));
    }

    #[test]
    fn a_missing_config_is_seeded_and_reads_back_as_trust_everything() {
        let dir = std::env::temp_dir().join("gacasy-config-seed");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");

        let config = load(&dir);
        assert!(dir.join(CONFIG_FILE).is_file(), "seed was written");
        assert!(config.keys.is_empty());
        assert!(config.trusts("3f9a1c"));
        // The log belongs beside the exe and its config, not off in AppData.
        assert_eq!(config.log_file, Some(dir.join(LOG_FILE)));
    }

    #[test]
    fn default_config_seed_is_valid_toml() {
        // A malformed seed would leave a fresh install falling back to
        // defaults for everything, silently.
        let table = DEFAULT_CONFIG.parse::<toml::Table>().expect("valid TOML");
        assert!(table["keys"].as_array().expect("keys is a list").is_empty());
    }
}
