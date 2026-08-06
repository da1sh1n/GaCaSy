// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Answers `--version`, `--signature` and `--help`, and reports the version of
//! the launcher embedded in this exe.

// ########## VERSION AND COMMAND LINE ##########

// Re-exported so the rest of the crate says `version::Version` without knowing
// the definition is shared with the listener and the launcher.
pub use common::version::{Version, parse};

use crate::payload;

/// The version of the launcher this installer carries, from a constant
/// `build.rs` reads out of `../launcher/Cargo.toml`.
///
/// `None` only under the `ROMZETA_PAYLOAD_OPTIONAL` escape hatch, where there
/// is no real launcher to compare against anyway.
pub fn bundled() -> Option<Version> {
    parse(payload::LAUNCHER_VERSION)
}

/// Answers `--version` / `--signature` / `--help` if any were passed. `true`
/// means the program has said its piece and should exit now.
///
/// Nothing probes the installer the way the listener probes a launcher — it is
/// the one program no other program runs. It answers anyway, because the person
/// who just downloaded a setup exe is entitled to ask what it is and who signed
/// it.
pub fn handled() -> bool {
    // The version is passed in rather than read inside `common`, because
    // `env!` expands to whichever crate *writes* it.
    let own = env!("CARGO_PKG_VERSION");
    common::version::handled(
        own,
        Some(&format!(
            "Romzeta installer {own}\n\n\
             Run it with no arguments to open the installer.\n  \
             --version    print x.y.z\n  \
             --signature  print this exe's signature"
        )),
    )
}
