// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! What makes a volume a cartridge: a `launcher.exe` at its root carrying a
//! signature from a key this listener was built to trust, for the job of being
//! a launcher.
//!
//! That is the whole definition. There is no marker file, no key to copy off the
//! disk, and nothing on the PC to edit — the previous scheme had all three, and
//! its own docs called it a recognition handshake rather than a security
//! boundary, because anyone who could read a cartridge could clone its secret.
//!
//! The decision itself lives in [`trust`], shared with the installer. What is
//! here is the part that is about a *volume*: which file to look at, holding it
//! still, and saying why not in a way the log can be read back from.
//!
//! This is a check on `launcher.exe` alone. `catalog.json`, `config.toml` and
//! everything under `games/` and `images/` sit on the same disk unsigned, and
//! nothing here vouches for them — see `SIGNING.md`, §1, and
//! `crate::volume`'s module doc for what starting the launcher actually spawns
//! next.
//!
//! # Reading the file, not asking the program
//!
//! The signature is verified by reading `launcher.exe`'s bytes off the disk,
//! never by running it and asking. A binary that reports its own trustworthiness
//! tells you nothing — a hostile one prints whatever makes it look legitimate —
//! and to ask, you would first have to execute the very thing you are deciding
//! whether to permit. So nothing here spawns anything, and neither does anything
//! downstream of it: the launcher's version comes out of the signature too (see
//! [`crate::volume`]), so by the time this listener runs a launcher at all, it
//! has already asked every question it has.
//!
//! # Holding the file still
//!
//! Verifying bytes and then executing a *path* is two different files if
//! anything can change the disk in between — and the disk in question was
//! plugged in by a stranger. So the file is opened once, denying writes and
//! deletes to everyone else for as long as the handle lives, and that handle
//! rides along in [`Trusted`] until the launcher has been started. It does not
//! stop hostile USB *firmware*, which lies below the filesystem and can serve
//! different bytes on a second read; it does stop anything going through
//! Windows.

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use trust::Anchor;

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

/// A launcher that verified, what vouched for it, and what it says it is.
pub struct Trusted {
    pub path: PathBuf,
    /// Which baked-in key accepted it — `release` or `dev`.
    pub anchor: String,
    /// The `x.y.z` from the signed comment. Authenticated, so it is the
    /// launcher's version without the launcher having been asked.
    pub version: String,
    /// The open handle the bytes were read through, kept alive so the file
    /// cannot be swapped between verifying it and running it. Never touched
    /// again — its lifetime *is* its purpose.
    _lock: File,
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
    /// It is there, and its signature does not make it something we will run.
    Signature(trust::Refusal),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoLauncher => write!(f, "no {LAUNCHER_NAME} at the volume root"),
            Refusal::Unreadable(e) => write!(f, "{LAUNCHER_NAME} could not be read: {e}"),
            // The trailing anchor list only helps on the one refusal it explains,
            // and would be noise on the rest.
            Refusal::Signature(trust::Refusal::Untrusted) => write!(
                f,
                "{LAUNCHER_NAME} is signed, but not by a key this listener trusts ({})",
                anchor_names()
            ),
            Refusal::Signature(reason) => write!(f, "{LAUNCHER_NAME}: {reason}"),
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
    let (file, bytes) = read_locked(&path).map_err(|e| Refusal::Unreadable(e.to_string()))?;

    let attested =
        trust::attest(&bytes, ANCHORS, trust::LAUNCHER_ROLE).map_err(Refusal::Signature)?;

    Ok(Trusted {
        path,
        anchor: attested.anchor,
        version: attested.version,
        _lock: file,
    })
}

/// Opens `path` so that nothing else can write to or delete it while the handle
/// lives, and reads it.
///
/// `share_mode(FILE_SHARE_READ)` is the whole trick: other readers are still
/// allowed — the image loader needs one to start the process — while writers and
/// deleters are refused for as long as the returned handle is held. Plain
/// `File::open` asks for the permissive default and would leave the file free to
/// change between here and the spawn.
#[cfg(windows)]
fn read_locked(path: &Path) -> std::io::Result<(File, Vec<u8>)> {
    use std::os::windows::fs::OpenOptionsExt;

    /// `windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ`, spelled out
    /// rather than pulling the crate in on a non-Windows-only path.
    const FILE_SHARE_READ: u32 = 0x0000_0001;

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((file, bytes))
}

/// Unix has no share modes — an open handle excludes nothing — so this is a
/// plain read. The handle is still returned and still held, so the two builds
/// have one shape rather than two.
#[cfg(not(windows))]
fn read_locked(path: &Path) -> std::io::Result<(File, Vec<u8>)> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((file, bytes))
}

/// The anchors this build carries, for the log line and `--signature`.
pub fn anchor_names() -> String {
    ANCHORS
        .iter()
        .map(|a| a.name)
        .collect::<Vec<_>>()
        .join(", ")
}
