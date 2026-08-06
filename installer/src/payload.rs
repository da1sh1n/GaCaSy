// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Exposes the bytes `build.rs` embedded — launcher, listener, seed config and
//! catalog — and inflates the two compressed binaries, checking the unpacked
//! length against the size recorded at build time.

// ########## THE EMBEDDED PAYLOAD ##########

// Sizes these unpack back to, written by `build.rs`: `LAUNCHER_BYTES` and
// `LISTENER_BYTES`. The free-space check needs the real size, and a compressed
// length does not answer "will this fit on the drive".
include!(concat!(env!("OUT_DIR"), "/sizes.rs"));

// `LAUNCHER_VERSION`, read by `build.rs` from `../launcher/Cargo.toml`'s
// `[package].version` — the source of truth, not anything the built exe reports.
include!(concat!(env!("OUT_DIR"), "/launcher-version.rs"));

/// The cartridge's app, written to `<volume>/launcher.exe` — packed.
///
/// Unpacked, these bytes carry the minisign signature `xtask sign` appended
/// before this crate was built, and that signature *is* the cartridge's
/// identity. `build.rs` verifies it before packing, so an installer that would
/// produce cartridges its own listener rejects cannot be built.
const LAUNCHER_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/launcher.exe.z"));

/// The PC-side service, written into the listener's install folder — packed.
const LISTENER_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/listener.exe.z"));

/// The launcher, ready to write.
pub fn launcher() -> Result<Vec<u8>, String> {
    unpack("launcher.exe", LAUNCHER_EXE, LAUNCHER_BYTES)
}

/// The listener, ready to write.
pub fn listener() -> Result<Vec<u8>, String> {
    unpack("listener.exe", LISTENER_EXE, LISTENER_BYTES)
}

/// Unpacks one payload binary, refusing anything that is not exactly what went
/// in. Both failures here mean a corrupted installer rather than anything the
/// user did, so they say so instead of suggesting a fix.
fn unpack(name: &str, packed: &[u8], expected: u64) -> Result<Vec<u8>, String> {
    use std::io::Read as _;

    let mut bytes = Vec::with_capacity(expected as usize);
    flate2::read::ZlibDecoder::new(packed)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("This installer's copy of {name} could not be unpacked ({e})."))?;

    if bytes.len() as u64 != expected {
        return Err(format!(
            "This installer's copy of {name} unpacked to {} bytes instead of {expected}.",
            bytes.len()
        ));
    }
    Ok(bytes)
}

/// Seed for a new cartridge's `config.toml` — look and feel only, no key.
pub const LAUNCHER_CONFIG: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/launcher-config.toml"));

/// Seed for a cartridge's `catalog.json`. Never read: job 1 writes a catalog
/// built from the games the user actually chose, and the launcher's seed is an
/// empty list by design — a launcher must never invent games it can't run, so
/// there is no example entry here to read the shape off either. Staged only so
/// the two crates keep pointing at one file.
#[allow(dead_code)]
pub const LAUNCHER_CATALOG: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/launcher-catalog.json"));

/// The payload slots that are empty, by name. Empty in a shipped installer means
/// the build used the escape hatch; every action that would write one of these
/// checks first and refuses, rather than producing a cartridge with a 0-byte
/// `launcher.exe` on it.
pub fn missing() -> Vec<&'static str> {
    // The packed slots, not the unpacked ones: the escape hatch writes an empty
    // file, and an empty file is not a valid zlib stream, so this has to answer
    // before anything tries to unpack it.
    [
        ("launcher.exe", LAUNCHER_EXE),
        ("listener.exe", LISTENER_EXE),
        ("config.toml", LAUNCHER_CONFIG),
    ]
    .into_iter()
    .filter(|(_, bytes)| bytes.is_empty())
    .map(|(name, _)| name)
    .collect()
}

/// One sentence naming what this build cannot do, or `None` when it is whole.
pub fn defect() -> Option<String> {
    let missing = missing();
    (!missing.is_empty()).then(|| {
        format!(
            "This installer was built without its payload ({}) and cannot install anything. \
             Rebuild the workspace with `cargo build --release`.",
            missing.join(", ")
        )
    })
}
