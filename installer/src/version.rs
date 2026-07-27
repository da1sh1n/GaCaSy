// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! `--version` and `--signature`, answered before the window opens.
//!
//! Nothing probes the installer the way the listener probes a launcher — it is
//! the one program in the system no other program runs. It answers anyway,
//! because the person who just downloaded a setup exe from the internet is
//! exactly the person entitled to ask what it is and who signed it, and because
//! three programs that answer the same two questions the same way are easier to
//! trust than two that do and one that doesn't.

use std::env;

/// Handles `--version` / `--signature` / `--help` if asked. `true` means the
/// program has said what it was asked and should exit now.
pub fn handled() -> bool {
    let mut version = false;
    let mut signature = false;
    let mut help = false;
    for arg in env::args_os().skip(1) {
        version |= arg == "--version";
        signature |= arg == "--signature";
        help |= arg == "--help" || arg == "-h";
    }
    if !version && !signature && !help {
        return false;
    }

    sigblock::cli::attach_console();
    if version {
        println!("{}", env!("CARGO_PKG_VERSION"));
    }
    if signature {
        sigblock::cli::print_signature();
    }
    if help {
        println!("GaCaSy installer {}", env!("CARGO_PKG_VERSION"));
        println!();
        println!("Run it with no arguments to open the installer.");
        println!("  --version    print x.y.z");
        println!("  --signature  print this exe's signature");
    }
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn our_version_is_a_bare_three_part_number() {
        // The same shape the launcher and listener print. Nothing parses the
        // installer's, but three programs answering one question three ways is
        // how the one that *is* parsed eventually drifts.
        let version = env!("CARGO_PKG_VERSION");
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(parts.len(), 3, "{version}");
        for part in parts {
            assert!(part.parse::<u64>().is_ok(), "{version} has a non-numeric part");
        }
    }
}
