// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The shared core: everything that happens once a volume shows up.
//!
//! This is steps 2–5 of `../structure.md` ("Responsibilities / flow") in full,
//! and it is **the** place they exist. Both triggers — the resident Windows
//! message pump and the one-shot Linux udev handoff — call
//! [`handle_volume`] and do nothing else with markers, keys or launching. A
//! trust check reimplemented per platform is exactly the bug the split is
//! meant to prevent.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::log::Log;
use crate::marker;

/// What became of one volume. Returned as well as logged, because the Windows
/// trigger debounces on it.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The launcher was spawned. The listener does not wait for it.
    Launched,
    /// Not a cartridge, or not one this PC trusts. Either way: leave it alone.
    Ignored,
    /// It *was* a trusted cartridge, but starting the launcher failed.
    Failed,
}

/// Verifies the volume at `root` and, if it is a trusted cartridge, starts its
/// launcher.
///
/// Every path out of here logs its reason first — with no console and (on
/// Windows) no window, the log is the only way to answer "why didn't it
/// start?".
pub fn handle_volume(root: &Path, config: &Config, log: &Log) -> Outcome {
    let marker = match marker::read(root) {
        Ok(marker) => marker,
        Err(marker::Error::Missing) => {
            // The overwhelmingly common case: an ordinary drive. Logged at the
            // same level as everything else anyway — when a cartridge *isn't*
            // being detected, "we looked at E:\ and found no marker" is the
            // line that tells you the trigger fired and the volume was seen.
            log.line(&format!("{} ignored: no marker", root.display()));
            return Outcome::Ignored;
        }
        Err(e) => {
            log.line(&format!("{} ignored: {e}", root.display()));
            return Outcome::Ignored;
        }
    };

    if !config.trusts(&marker.key) {
        log.line(&format!(
            "{} ignored: key {} is not in this listener's trusted keys",
            root.display(),
            redact(&marker.key)
        ));
        return Outcome::Ignored;
    }

    let Some(launcher) = resolve_launcher(root, &marker.launcher) else {
        // A marker whose `launcher` escapes the volume — `..\..\windows\…` or
        // an absolute path — would turn "plug in a disk" into "run an
        // arbitrary local program". The marker is trusted enough to name a
        // binary *on the cartridge*, and no further than that.
        log.line(&format!(
            "{} ignored: launcher `{}` is not a path inside the volume",
            root.display(),
            marker.launcher
        ));
        return Outcome::Ignored;
    };

    if !launcher.is_file() {
        log.line(&format!(
            "{} ignored: launcher {} does not exist",
            root.display(),
            launcher.display()
        ));
        return Outcome::Ignored;
    }

    // Spawned, never waited on: the Windows build has to get straight back to
    // its message loop, and the Linux build has to exit and leave the launcher
    // running. `current_dir` is the volume root because the launcher resolves
    // all its content (catalog.json, images/, games/) relative to itself.
    match Command::new(&launcher).current_dir(root).spawn() {
        Ok(child) => {
            log.line(&format!(
                "{} launched {} (pid {})",
                root.display(),
                launcher.display(),
                child.id()
            ));
            Outcome::Launched
        }
        Err(e) => {
            log.line(&format!(
                "{} FAILED to start {}: {e}",
                root.display(),
                launcher.display()
            ));
            Outcome::Failed
        }
    }
}

/// Joins the marker's `launcher` onto the volume root, refusing anything that
/// could point outside it.
///
/// Only plain path components are allowed — no absolute paths, no drive
/// prefixes, no `..`, no bare `/`. `.` is dropped as harmless.
fn resolve_launcher(root: &Path, launcher: &str) -> Option<PathBuf> {
    let relative = Path::new(launcher);
    let mut resolved = root.to_path_buf();
    let mut components = 0;
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                resolved.push(part);
                components += 1;
            }
            Component::CurDir => {}
            // ParentDir, RootDir, Prefix — all of which can leave the volume.
            _ => return None,
        }
    }
    (components > 0).then_some(resolved)
}

/// Shortens a key for logging. The key is not a secret the listener is
/// protecting — the point is that a log file pasted into a bug report doesn't
/// hand over a working pairing.
fn redact(key: &str) -> String {
    let head: String = key.chars().take(6).collect();
    if key.chars().count() > 6 {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;

    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"E:\")
        } else {
            PathBuf::from("/media/cartridge")
        }
    }

    #[test]
    fn resolves_a_plain_relative_launcher() {
        let resolved = resolve_launcher(&root(), "launcher.exe").expect("inside the volume");
        assert_eq!(resolved, root().join("launcher.exe"));
        assert_eq!(
            resolve_launcher(&root(), "bin/launcher").expect("inside the volume"),
            root().join("bin").join("launcher")
        );
        assert_eq!(
            resolve_launcher(&root(), "./launcher.exe").expect("inside the volume"),
            root().join("launcher.exe")
        );
    }

    #[test]
    fn refuses_launchers_that_leave_the_volume() {
        assert!(resolve_launcher(&root(), "../evil.exe").is_none());
        assert!(resolve_launcher(&root(), "games/../../evil.exe").is_none());
        assert!(resolve_launcher(&root(), "/usr/bin/evil").is_none());
        assert!(resolve_launcher(&root(), "").is_none());
        assert!(resolve_launcher(&root(), ".").is_none());
        if cfg!(windows) {
            assert!(resolve_launcher(&root(), r"C:\Windows\System32\cmd.exe").is_none());
        }
    }

    #[test]
    fn redacts_all_but_the_first_few_characters() {
        assert_eq!(redact("3f9a1c04b7"), "3f9a1c…");
        assert_eq!(redact("abc"), "abc");
    }

    /// Builds a fake volume in a temp folder and runs the real core over it.
    fn fake_volume(name: &str, marker: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gacasy-volume-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        if let Some(marker) = marker {
            fs::write(dir.join(marker::MARKER_FILE), marker).expect("write marker");
        }
        dir
    }

    fn config(keys: &[&str]) -> Config {
        Config {
            keys: keys.iter().map(|k| k.to_string()).collect(),
            log_file: None,
            debounce_seconds: 0,
        }
    }

    #[test]
    fn a_volume_with_no_marker_is_ignored() {
        let dir = fake_volume("plain", None);
        let outcome = handle_volume(&dir, &config(&["3f9a1c"]), &Log::silent());
        assert_eq!(outcome, Outcome::Ignored);
    }

    #[test]
    fn an_untrusted_key_is_ignored() {
        let dir = fake_volume(
            "untrusted",
            Some("version = 1\nkey = \"b72e04\"\nlauncher = \"launcher.exe\"\n"),
        );
        fs::write(dir.join("launcher.exe"), b"not really an exe").expect("write launcher");
        let outcome = handle_volume(&dir, &config(&["3f9a1c"]), &Log::silent());
        assert_eq!(outcome, Outcome::Ignored);
    }

    #[test]
    fn a_trusted_cartridge_with_no_launcher_on_disk_is_ignored_not_failed() {
        // Distinct from Failed: nothing was attempted, so there is nothing to
        // retry or report as broken.
        let dir = fake_volume(
            "nolauncher",
            Some("version = 1\nkey = \"3f9a1c\"\nlauncher = \"launcher.exe\"\n"),
        );
        let outcome = handle_volume(&dir, &config(&["3f9a1c"]), &Log::silent());
        assert_eq!(outcome, Outcome::Ignored);
    }

    #[test]
    fn a_marker_escaping_the_volume_is_ignored() {
        let dir = fake_volume(
            "escape",
            Some("version = 1\nkey = \"3f9a1c\"\nlauncher = \"../../evil.exe\"\n"),
        );
        let outcome = handle_volume(&dir, &config(&["3f9a1c"]), &Log::silent());
        assert_eq!(outcome, Outcome::Ignored);
    }
}
