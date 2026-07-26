// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The Linux trigger — **not built yet**.
//!
//! This module exists so the crate compiles on Linux and so the shared core can
//! be exercised there with `--check <mountpoint>`. It is deliberately not a
//! partial implementation: the Linux side is one-shot, and the parts that make
//! it work are the ones missing here, not the parts already written in
//! [`crate::volume`].
//!
//! Still to build (see `../../structure.md`, "Linux — reactive, one-shot"):
//!
//! * The udev rule — `ACTION=="add"`, `SUBSYSTEM=="block"`,
//!   `ENV{ID_FS_USAGE}=="filesystem"` — plus the `RUN+="… systemd-run
//!   --no-block …"` handoff. udev kills `RUN+=` children unconditionally once
//!   the event finishes, so it can neither wait nor parent a GUI launcher.
//! * A bounded wait for the mountpoint. udev fires on **device add**, before
//!   udisks2 mounts anything, so at rule time there is usually no path to hand
//!   to the core yet.
//! * The logind lookup and environment handoff (`--uid`, `DISPLAY` /
//!   `WAYLAND_DISPLAY` / `DBUS_SESSION_BUS_ADDRESS`), because udev runs as root
//!   with no session and the launcher is a GUI app.
//! * The headless decision: no active graphical session means nowhere to put a
//!   window, so log and exit rather than letting `systemd-run` fail obscurely.
//!
//! Installing the rule is the installer's job, not this program's.

use crate::config::Config;
use crate::log::Log;

pub fn run(_config: Config, log: Log) {
    log.line(
        "the Linux trigger is not implemented yet — \
         run with `--check <mountpoint>` to exercise the shared core",
    );
}
