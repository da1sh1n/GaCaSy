// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The Linux trigger — not implemented. Exists so the crate compiles on Linux;
//! `--check <mountpoint>` exercises the core in the meantime. What is still to
//! build is listed in `../../structure.md`, "Linux — reactive, one-shot".

// ########## THE LINUX TRIGGER ##########

use crate::log::Log;

/// Takes ownership of `log`, records that there is no trigger here, and
/// returns. Signature-compatible with the Windows `run` so `trigger::run` is
/// one call either way.
pub fn run(log: Log) {
    log.line(
        "the Linux trigger is not implemented yet — \
         run with `--check <mountpoint>` to exercise the shared core",
    );
}
