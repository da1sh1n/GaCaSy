// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! What makes a volume a cartridge: a `launcher.exe` at its root carrying a
//! signature from a key this listener was built to trust.
//!
//! That is the whole definition. There is no marker file, no key to copy off the
//! disk, and nothing on the PC to edit — the previous scheme had all three, and
//! its own docs called it a recognition handshake rather than a security
//! boundary, because anyone who could read a cartridge could clone its secret.
//!
//! # Reading the file, not asking the program
//!
//! The signature is verified by reading `launcher.exe`'s bytes off the disk,
//! never by running it and asking. A binary that reports its own trustworthiness
//! tells you nothing — a hostile one prints whatever makes it look legitimate —
//! and to ask, you would first have to execute the very thing you are deciding
//! whether to permit. So nothing here spawns anything. By the time the listener
//! runs the launcher at all, this module has already said yes.
//!
//! (The version probe in [`crate::version`] *does* run the launcher and believe
//! what it says. That is fine, and only because it happens strictly after this:
//! once you know who wrote a program, believing what it says about itself is
//! reasonable. Before that, it is not.)

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// One public key this build accepts, and a name for it so the log can say
/// which — `release` or `dev`.
pub struct Anchor {
    pub name: &'static str,
    pub base64: &'static str,
}

// `ANCHORS: &[Anchor]`, written by build.rs from keys/*.pub. Compiled in rather
// than read from disk: a trust anchor sitting in a writable file beside the exe
// would let anything that could edit it grant itself auto-run. See build.rs.
include!(concat!(env!("OUT_DIR"), "/trust_anchors.rs"));

/// The binary a cartridge is expected to carry, by name.
///
/// Hardcoded per platform, where the retired marker let the cartridge name its
/// own launcher. That field existed so a Linux cartridge could point at a
/// different binary; a `cfg` covers the same case without handing an untrusted
/// disk a say in which path gets executed — which is why the old code needed a
/// containment check to stop `..\..\windows\system32\cmd.exe` from being a valid
/// answer. There is now no path to sandbox.
#[cfg(windows)]
pub const LAUNCHER_NAME: &str = "launcher.exe";
#[cfg(not(windows))]
pub const LAUNCHER_NAME: &str = "launcher";

/// A launcher that verified, and the key that vouched for it.
pub struct Trusted {
    pub path: PathBuf,
    pub anchor: &'static str,
}

/// Why a volume is not a cartridge worth launching.
///
/// Every variant ends the same way — the volume is ignored — but they are
/// distinct because the log is the only diagnostic this program has, and
/// "ordinary USB stick" and "someone tampered with a cartridge" should not read
/// alike.
pub enum Refusal {
    /// No launcher at the volume root. Overwhelmingly the common case: an
    /// ordinary drive. Not a problem, and deliberately not alarming.
    NoLauncher,
    Unreadable(String),
    /// A launcher with no signature block — a self-built or stripped binary.
    Unsigned,
    /// A block that is there but is not a minisign signature.
    Malformed(String),
    /// Correctly signed, by a key we do not accept. The interesting one.
    Untrusted,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoLauncher => write!(f, "no {LAUNCHER_NAME} at the volume root"),
            Refusal::Unreadable(e) => write!(f, "{LAUNCHER_NAME} could not be read: {e}"),
            Refusal::Unsigned => write!(f, "{LAUNCHER_NAME} carries no signature"),
            Refusal::Malformed(e) => write!(f, "{LAUNCHER_NAME}'s signature is malformed: {e}"),
            Refusal::Untrusted => write!(
                f,
                "{LAUNCHER_NAME} is signed, but not by a key this listener trusts ({})",
                anchor_names()
            ),
        }
    }
}

/// Verifies `<root>/launcher.exe` against every baked-in anchor.
pub fn verify_launcher(root: &Path) -> Result<Trusted, Refusal> {
    let path = root.join(LAUNCHER_NAME);
    if !path.is_file() {
        return Err(Refusal::NoLauncher);
    }

    // Read the lot. A launcher is a few megabytes and this happens once per
    // volume arrival, against a disk that was just mounted — the alternative,
    // minisign's streaming verification, only works on pre-hashed signatures and
    // would buy nothing here.
    let bytes = fs::read(&path).map_err(|e| Refusal::Unreadable(e.to_string()))?;
    let (payload, signature) = sigblock::split(&bytes);
    let Some(signature) = signature else {
        return Err(Refusal::Unsigned);
    };
    let signature = minisign_verify::Signature::decode(signature)
        .map_err(|e| Refusal::Malformed(e.to_string()))?;

    for anchor in ANCHORS {
        let Ok(key) = minisign_verify::PublicKey::from_base64(anchor.base64) else {
            // build.rs put it there, so this is a broken build rather than
            // anything about the cartridge. Skip it and let the others speak.
            continue;
        };
        if key.verify(payload, &signature, false).is_ok() {
            return Ok(Trusted {
                path,
                anchor: anchor.name,
            });
        }
    }
    Err(Refusal::Untrusted)
}

/// The anchors this build carries, for the log line and `--signature`.
pub fn anchor_names() -> String {
    ANCHORS
        .iter()
        .map(|a| a.name)
        .collect::<Vec<_>>()
        .join(", ")
}
