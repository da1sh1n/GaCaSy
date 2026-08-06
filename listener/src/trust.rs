// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Verifies `<root>/launcher.exe` against the anchors compiled into this build,
//! holding the file open while it is checked, and phrases the refusal when it
//! fails. Reads the bytes; runs nothing.

// ########## IS THIS VOLUME A CARTRIDGE ##########

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use trust::Anchor;

// `ANCHORS: &[Anchor]`, written by build.rs from keys/*.pub and pasted in here
// at compile time. Compiled in rather than read from disk: an anchor sitting in
// a writable file beside the exe would let anything able to edit it grant
// itself auto-run.
include!(concat!(env!("OUT_DIR"), "/trust_anchors.rs"));

/// The binary a cartridge is expected to carry, by name. Hardcoded per platform
/// rather than named by the disk, so there is no attacker-supplied path to
/// sandbox in the first place.
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
    /// cannot be swapped between verifying it and running it. Never read; the
    /// leading `_` marks it as held for its lifetime alone.
    _lock: File,
}

/// Why a volume is not a cartridge worth launching. Every variant ends the same
/// way, but they stay distinct because the log is this program's only
/// diagnostic and "ordinary USB stick" must not read like "someone tampered".
pub enum Refusal {
    /// No launcher at the volume root — every ordinary drive, and by far the
    /// most common outcome.
    NoLauncher,
    Unreadable(String),
    /// It is there, and its signature does not make it something we will run.
    Signature(trust::Refusal),
}

impl fmt::Display for Refusal {
    // `fmt` is fixed by the Display trait, so it keeps rustc's spelling.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoLauncher => write!(f, "no {LAUNCHER_NAME} at the volume root"),
            Refusal::Unreadable(e) => write!(f, "{LAUNCHER_NAME} could not be read: {e}"),
            // The anchor list only helps on the one refusal it explains, so it
            // is spelled out here instead of appended to every variant.
            Refusal::Signature(trust::Refusal::Untrusted) => write!(
                f,
                "{LAUNCHER_NAME} is signed, but not by a key this listener trusts ({})",
                anchorNames()
            ),
            Refusal::Signature(reason) => write!(f, "{LAUNCHER_NAME}: {reason}"),
        }
    }
}

/// Verifies `<root>/launcher.exe` against every baked-in anchor, returning the
/// held-open file and what its signature says, or why it was refused.
pub fn verifyLauncher(root: &Path) -> Result<Trusted, Refusal> {
    let path = root.join(LAUNCHER_NAME);
    if !path.is_file() {
        return Err(Refusal::NoLauncher);
    }

    // Read the lot into memory. A launcher is a few megabytes and this happens
    // once per volume arrival; minisign's streaming verification only works on
    // pre-hashed signatures and would buy nothing here.
    let (file, bytes) = readLocked(&path).map_err(|e| Refusal::Unreadable(e.to_string()))?;

    let attested =
        trust::attest(&bytes, ANCHORS, trust::LAUNCHER_ROLE).map_err(Refusal::Signature)?;

    Ok(Trusted {
        path,
        anchor: attested.anchor,
        version: attested.version,
        // Moved into the struct, so the lock is released only when the caller
        // drops it — after the launcher has been started.
        _lock: file,
    })
}

/// Opens `path` so nothing else can write to or delete it while the handle
/// lives, and reads it. Returns the still-open handle alongside the bytes.
///
/// Verifying bytes and then executing a *path* is two different files if
/// anything can change the disk in between, and the disk was plugged in by a
/// stranger. This does not stop hostile USB firmware, which lies below the
/// filesystem; it stops everything going through Windows.
#[cfg(windows)]
fn readLocked(path: &Path) -> std::io::Result<(File, Vec<u8>)> {
    // The extension trait that adds `share_mode` to OpenOptions.
    use std::os::windows::fs::OpenOptionsExt;

    /// `windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ`, spelled out
    /// rather than pulling that crate onto a path this file otherwise shares.
    const FILE_SHARE_READ: u32 = 0x0000_0001;

    // Sharing *reads* and nothing else is the whole trick: the image loader
    // still needs a read handle to start the process, while writers and
    // deleters are refused. Plain `File::open` asks for the permissive default.
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((file, bytes))
}

/// Unix has no share modes — an open handle excludes nothing — so this is a
/// plain read. The handle is still returned and still held, so both builds have
/// one shape rather than two.
#[cfg(not(windows))]
fn readLocked(path: &Path) -> std::io::Result<(File, Vec<u8>)> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((file, bytes))
}

/// The anchors this build carries, comma-separated, for the log line and
/// `--signature`.
pub fn anchorNames() -> String {
    ANCHORS
        .iter()
        .map(|a| a.name)
        .collect::<Vec<_>>()
        .join(", ")
}
