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
//! read the bytes -> verify the signature -> ask the signature what it is -> run it
//! ```
//!
//! Nothing is executed at any point before the last step. The version used to
//! come from running the launcher with `--version` and believing it, which was
//! defensible only because it happened after verification — but it meant the
//! file was opened, executed and read three separate times to answer two
//! questions. minisign signs a comment alongside the payload, `xtask` writes the
//! version into it, and [`crate::trust`] hands it back already authenticated, so
//! the probe bought nothing that the signature was not already carrying.

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

    // Straight out of the verified signature — see version.rs.
    let ours = version::own();
    match version::parse(&launcher.version) {
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
            // binary; refusing to start a genuine launcher over a comment we
            // cannot parse would turn a cosmetic fault into a dead cartridge.
            // A definite mismatch above is a different matter — that is the
            // signature clearly stating something we cannot work with.
            log.line(&format!(
                "{} launcher's signature carries no usable version ({:?}); starting it anyway",
                root.display(),
                launcher.version
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
