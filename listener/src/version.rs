// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! This listener's own `x.y.z` and printing it. The type and parser come from
//! `common::version`.

// ########## THIS LISTENER'S VERSION ##########

// Re-exported rather than wrapped, so the rest of the crate says `version::parse`
// and does not have to know the definition lives elsewhere.
pub use common::version::{Version, parse};

/// This listener's own version, read from its Cargo manifest at compile time.
/// `Cargo.toml` is the only place it is written.
pub fn own() -> Version {
    // `env!` fails the build if the manifest has no version, so the only way
    // this panics is a version Cargo accepted and we cannot parse.
    parse(env!("CARGO_PKG_VERSION")).expect("our own version is a valid x.y.z")
}

/// Prints the `--version` line: exactly `x.y.z`, no program name and no `v`.
/// Every Romzeta exe prints the same shape, which is what keeps the human
/// reading it and the parser reading it from drifting apart.
pub fn print() {
    println!("{}", own());
}
