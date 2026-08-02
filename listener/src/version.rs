// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! `x.y.z` — this listener's own, and a launcher's as its signature states it.
//!
//! **x is shared by every GaCaSy program** and means "the way these programs
//! talk to each other"; y and z belong to each program alone. Two programs are
//! compatible when their x matches, which is a thing the listener can actually
//! check before handing control to a cartridge built against a different
//! generation of the system.
//!
//! # Where a launcher's version comes from
//!
//! Out of its signature, via [`crate::trust`] — `xtask` writes `<role> <version>
//! <date>` into minisign's trusted comment, which is signed alongside the
//! payload and so cannot be edited after the fact.
//!
//! This used to run `launcher.exe --version` and believe the answer, which was
//! defensible only because it happened strictly after the signature check. It is
//! gone, and the reason is worth keeping: an answer that is already inside the
//! thing you verified does not need to be asked for, and asking cost a spawn, a
//! five-second bounded wait on the message-pump thread, and a second and third
//! open of a file on a disk a stranger controls.
//!
//! # Why the output is bare
//!
//! Every GaCaSy exe prints exactly `x.y.z` and nothing else — no program name,
//! no prefix. It is one line for a human and one line to parse, and the two
//! having the same shape is what keeps them from drifting apart.

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// This listener's own version, from its Cargo manifest. There is deliberately
/// no second place to update.
pub fn own() -> Version {
    parse(env!("CARGO_PKG_VERSION")).expect("our own version is a valid x.y.z")
}

/// Prints the `--version` line.
pub fn print() {
    println!("{}", own());
}

/// Parses exactly `x.y.z`.
///
/// Strict on purpose, and it is fed the version field of a signed comment. A
/// launcher whose signature says something else is not one whose compatibility
/// we can reason about, and guessing at "0.2" or "0.2.0-rc1" would be inventing
/// a claim the signature never made.
pub fn parse(text: &str) -> Option<Version> {
    let mut parts = text.trim().split('.');
    let mut next = || parts.next()?.trim().parse::<u64>().ok();
    let version = Version {
        major: next()?,
        minor: next()?,
        patch: next()?,
    };
    parts.next().is_none().then_some(version)
}
