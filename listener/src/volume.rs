// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The shared core: everything that happens once a volume shows up.
//!
//! This is steps 2–5 of `../structure.md` ("Responsibilities / flow") in full,
//! and it is **the** place they exist. Both triggers — the resident Windows
//! message pump and the one-shot Linux udev handoff — call [`handle_volume`]
//! and do nothing else with signatures or launching. A trust check
//! reimplemented per platform is exactly the bug the split is meant to prevent.
//!
//! The order below is the security property, not a style choice:
//!
//! ```text
//! read the bytes -> verify the signature -> run it -> ask what it is
//! ```
//!
//! Nothing is executed until [`crate::trust`] has said yes, and the version
//! question is asked only of a binary that already passed. Reversing the last
//! two steps would mean running an unverified program to decide whether to run
//! it.

use std::path::Path;
use std::process::Command;

use crate::log::Log;
use crate::{alert, trust, version};

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

/// Verifies the volume at `root` and, if it carries a launcher this listener
/// trusts, starts it.
///
/// Every path out of here logs its reason first — with no console and (on
/// Windows) no window, the log is the only way to answer "why didn't it
/// start?".
pub fn handle_volume(root: &Path, log: &Log) -> Outcome {
    let launcher = match trust::verify_launcher(root) {
        Ok(launcher) => launcher,
        Err(reason) => {
            // Refusals share one line shape because they share one meaning:
            // this volume is left alone. `no launcher at the volume root` is by
            // far the most common and is not a problem — it is every ordinary
            // drive anyone ever plugs in — but it is logged at the same level as
            // the rest, because when a cartridge *isn't* being detected, "we
            // looked at E:\ and found nothing" is the line that proves the
            // trigger fired at all.
            log.line(&format!("{} ignored: {reason}", root.display()));
            return Outcome::Ignored;
        }
    };

    // Only now — the binary is one we signed, so asking it about itself is
    // reasonable. See version.rs.
    let ours = version::own();
    match version::probe(&launcher.path) {
        Some(theirs) if theirs.major != ours.major => {
            log.line(&format!(
                "{} ignored: launcher is project version {} and this listener is {} \
                 (signed by the {} key)",
                root.display(),
                theirs.major,
                ours.major,
                launcher.anchor
            ));
            alert::warn(
                "GaCaSy — cartridge not compatible",
                &format!(
                    "This cartridge's launcher is version {theirs}, but the GaCaSy installed \
                     on this PC is version {ours}.\n\n\
                     They share the same signing key, so both are genuine — but the first \
                     number has to match for them to work together. Update whichever is \
                     older.\n\n\
                     Nothing was started."
                ),
            );
            return Outcome::Ignored;
        }
        Some(theirs) => {
            log.line(&format!(
                "{} verified: {} launcher {theirs}, signed by the {} key",
                root.display(),
                trust::LAUNCHER_NAME,
                launcher.anchor
            ));
        }
        None => {
            // Deliberately not fatal. The signature already proved this is our
            // binary; refusing to start a genuine launcher because it fumbled a
            // side question would turn a cosmetic fault into a dead cartridge.
            // A definite mismatch above is a different matter — that is the
            // launcher clearly stating something we cannot work with.
            log.line(&format!(
                "{} launcher did not report a usable version; starting it anyway",
                root.display()
            ));
        }
    }

    // Spawned, never waited on: the Windows build has to get straight back to
    // its message loop, and the Linux build has to exit and leave the launcher
    // running. `current_dir` is the volume root because the launcher resolves
    // all its content (catalog.json, images/, games/) relative to itself.
    match Command::new(&launcher.path).current_dir(root).spawn() {
        Ok(child) => {
            log.line(&format!(
                "{} launched {} (pid {})",
                root.display(),
                launcher.path.display(),
                child.id()
            ));
            Outcome::Launched
        }
        Err(e) => {
            log.line(&format!(
                "{} FAILED to start {}: {e}",
                root.display(),
                launcher.path.display()
            ));
            Outcome::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Builds a fake volume in a temp folder.
    fn fake_volume(name: &str, launcher: Option<&[u8]>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gacasy-volume-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        if let Some(bytes) = launcher {
            fs::write(dir.join(trust::LAUNCHER_NAME), bytes).expect("write launcher");
        }
        dir
    }

    #[test]
    fn a_volume_with_no_launcher_is_ignored() {
        let dir = fake_volume("plain", None);
        assert_eq!(handle_volume(&dir, &Log::silent()), Outcome::Ignored);
    }

    #[test]
    fn an_unsigned_launcher_is_never_started() {
        // The whole point of the change: a binary sitting at a volume root with
        // the right *name* gets nowhere without the right signature.
        let dir = fake_volume("unsigned", Some(b"MZ nobody signed this"));
        assert_eq!(handle_volume(&dir, &Log::silent()), Outcome::Ignored);
    }

    #[test]
    fn a_launcher_signed_by_a_stranger_is_never_started() {
        let signature = "untrusted comment: signature from a key we do not have\n\
                         RUQAAAAAAAAAAOaGxHqZQ0KtvVCJ6iKzXG8bFvKZ0V0kZ1qWzKz0hVYQ4rZ8Xk1t\n\
                         trusted comment: gacasy-launcher 0.2.0\n\
                         AAAA==\n";
        let signed = sigblock::attach(b"MZ signed by someone else", signature);
        let dir = fake_volume("stranger", Some(&signed));
        assert_eq!(handle_volume(&dir, &Log::silent()), Outcome::Ignored);
    }
}
