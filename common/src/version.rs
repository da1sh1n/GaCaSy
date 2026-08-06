// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The `x.y.z` type and its parser, plus the shared `--version` / `--signature`
//! / `--help` command line. `x` is the compatibility generation; `y` and `z`
//! are per-program.

// ########## VERSION NUMBERS ##########

use std::env;

// ========== The Number ==========

/// A parsed `x.y.z`. `Copy` because it is three integers and gets passed around
/// by value everywhere it is compared.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl std::fmt::Display for Version {
    // `fmt` is fixed by the Display trait, so it keeps rustc's spelling rather
    // than the project's — an external contract we do not get to rename.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Parses exactly three dot-separated numbers. `None` for anything else,
/// including `0.2` and `0.2.0-rc1`. Input is the version field of a signed
/// trusted comment.
pub fn parse(text: &str) -> Option<Version> {
    let mut parts = text.trim().split('.');
    // A closure so all three fields are read identically. `?` inside a closure
    // returns from the *closure*, so each call hands back an Option that the
    // outer `?` then unwraps — that is why this is not just `parts.next()`.
    let mut next = || parts.next()?.trim().parse::<u64>().ok();
    let version = Version {
        major: next()?,
        minor: next()?,
        patch: next()?,
    };
    // The borrow `next` held on `parts` ends at its last use above, which is
    // what lets us touch `parts` again here. A fourth part means this was never
    // an x.y.z; `then_some` turns that bool into the Option we return.
    parts.next().is_none().then_some(version)
}

// ========== The Command Line ==========

/// Answers `--version`, `--signature` and `--help` if any of them were passed,
/// returning true when the program has said its piece and should exit now.
///
/// `own_version` is the caller's own `env!("CARGO_PKG_VERSION")` — it has to be
/// passed in, because that macro expands to whichever crate *writes* it and
/// would otherwise report this one. `help` is the text `--help` prints, or
/// `None` for a program that does not take `--help` at all.
pub fn handled(own_version: &str, help: Option<&str>) -> bool {
    let mut version = false;
    let mut signature = false;
    let mut wants_help = false;

    // `args_os`, not `args`: the latter panics on an argument that is not valid
    // UTF-8, and a program whose only job right now is to print one line should
    // not die over a stray byte in someone's shell history.
    for arg in env::args_os().skip(1) {
        // `|=` rather than `=` so a flag repeated on the line stays true.
        version |= arg == "--version";
        signature |= arg == "--signature";
        wants_help |= help.is_some() && (arg == "--help" || arg == "-h");
    }
    if !version && !signature && !wants_help {
        return false;
    }

    // These are all `windows_subsystem = "windows"` binaries, so Windows gives
    // them no console and `println!` goes nowhere until this reattaches to the
    // terminal that started us.
    sigblock::cli::attachConsole();
    if version {
        println!("{own_version}");
    }
    if signature {
        sigblock::cli::printSignature();
    }
    // A let-chain: `wants_help` is only ever true when `help` is Some, but
    // binding it here is what gets the text out of the Option.
    if wants_help && let Some(text) = help {
        println!("{text}");
    }
    true
}
