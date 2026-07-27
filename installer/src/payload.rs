// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Everything the installer writes, carried inside it.
//!
//! One self-contained exe: no downloads, no prerequisites, no side-by-side
//! files. `build.rs` stages each of these in `OUT_DIR` and fails the build with
//! a clear message when one is missing, so the bytes below are always either the
//! real artifact or — under the documented `GACASY_PAYLOAD_OPTIONAL` escape
//! hatch — deliberately empty.

/// The cartridge's app, written to `<volume>/launcher.exe`.
///
/// These bytes carry their own minisign signature, appended past the end of the
/// image by `xtask sign` before this crate was built (see the sigblock crate).
/// That signature *is* the cartridge's identity — the listener reads it off the
/// disk and refuses a launcher it cannot verify — so copying this array onto a
/// volume is the entire act of making that volume trusted. `build.rs` verifies
/// the signature before staging, so an installer that would produce cartridges
/// its own listener rejects cannot be built.
pub const LAUNCHER_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/launcher.exe"));

/// The PC-side service, written into the listener's install folder.
pub const LISTENER_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/listener.exe"));

/// Seed for a new cartridge's `config.toml` — look and feel only, no key.
pub const LAUNCHER_CONFIG: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/launcher-config.toml"));

/// Seed for a cartridge's `catalog.json`. Only ever used as a shape reference;
/// job 1 writes a catalog built from the games the user actually chose.
#[allow(dead_code)]
pub const LAUNCHER_CATALOG: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/launcher-catalog.json"));

/// The payload slots that are empty, by name. Empty in a shipped installer means
/// the build used the escape hatch; every action that would write one of these
/// checks first and refuses, rather than producing a cartridge with a 0-byte
/// `launcher.exe` on it.
pub fn missing() -> Vec<&'static str> {
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
