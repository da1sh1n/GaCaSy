// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! `--version` and `--signature`, answered before the launcher does anything
//! else.
//!
//! # Why the launcher of all things has a command line
//!
//! Because the listener asks. Having verified this exe's signature, it runs it
//! with `--version` and compares the first number against its own — that is how
//! "these two programs are the same generation of the system" is checked at the
//! moment it matters, rather than assumed. See `../../listener/src/version.rs`.
//!
//! # The two rules that make it work
//!
//! **Print `x.y.z` and nothing else.** No program name, no prefix, no `v`. The
//! listener parses this, and every decoration is something to get wrong on one
//! side and not the other.
//!
//! **Answer before touching the disk.** [`handled`] is the first thing `main`
//! calls, ahead of resolving the content folder, seeding it, or taking the
//! single-instance mutex. A launcher sitting on a cartridge would otherwise
//! create folders and rewrite its own exe as a side effect of being asked what
//! version it is — a write to a stranger's disk in response to a question.

use std::env;

/// Handles `--version` / `--signature` if asked. `true` means the program has
/// said what it was asked and should exit now.
pub fn handled() -> bool {
    let mut version = false;
    let mut signature = false;
    for arg in env::args_os().skip(1) {
        version |= arg == "--version";
        signature |= arg == "--signature";
    }
    if !version && !signature {
        return false;
    }

    sigblock::cli::attach_console();
    if version {
        println!("{}", env!("CARGO_PKG_VERSION"));
    }
    if signature {
        sigblock::cli::print_signature();
    }
    true
}
