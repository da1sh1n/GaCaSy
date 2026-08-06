// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Handles one volume: verify its launcher, compare project versions, start it.
//! Logs the reason on every path out.
//!
//! ```text
//! read the bytes -> verify the signature -> ask the signature what it is -> run it
//! ```

// ########## HANDLING ONE VOLUME ##########

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
/// trusts and can work with, starts it. Returns what happened.
///
/// Every path out logs its reason first — with no console and, on Windows, no
/// window, the log is the only way to answer "why didn't it start?".
pub fn handleVolume(root: &Path, log: &Log) -> Outcome {
    let launcher = match trust::verifyLauncher(root) {
        Ok(launcher) => launcher,
        Err(reason) => {
            // Every refusal shares one line shape because they share one
            // meaning. "no launcher at the volume root" is by far the most
            // common and is not a problem, but it is logged all the same: when
            // a cartridge *isn't* being detected, "we looked at E:\ and found
            // nothing" is the line that proves the trigger fired at all.
            log.line(&format!("{} ignored: {reason}", root.display()));
            return Outcome::Ignored;
        }
    };

    // Straight out of the verified signature — nothing was executed to get it.
    let ours = version::own();
    match version::parse(&launcher.version) {
        // Majors differ: both are genuine, and they cannot work together.
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
                "Romzeta — cartridge not compatible",
                &format!(
                    "This cartridge's launcher is version {theirs}, but the Romzeta installed \
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
            // binary, and refusing a genuine launcher over a comment we cannot
            // parse would turn a cosmetic fault into a dead cartridge. A
            // definite mismatch above is different: that is the signature
            // clearly stating something we cannot work with.
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
    // its content (catalog.json, images/, games/) relative to where it runs.
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
