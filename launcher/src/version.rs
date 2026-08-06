// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Answers `--version` and `--signature`. Prints `x.y.z` and nothing else.

// ########## THE COMMAND LINE ##########

/// Answers `--version` / `--signature` if either was passed. `true` means the
/// program has said its piece and should exit now.
///
/// Called first thing in `main`, ahead of resolving the content folder or
/// taking the single-instance mutex: a launcher on a cartridge that created
/// folders as a side effect of being asked its version would be writing to a
/// stranger's disk in answer to a question.
pub fn handled() -> bool {
    // The version is passed in because `env!` expands to whichever crate
    // *writes* it, and the shared implementation lives in `common`. `None`
    // because the launcher takes no `--help`.
    common::version::handled(env!("CARGO_PKG_VERSION"), None)
}
