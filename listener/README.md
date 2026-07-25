<!--
  SPDX-License-Identifier: GPL-3.0-or-later
  Copyright (C) 2026 da1sh1n
  This file is part of GaCaSy, licensed under the GNU General Public License
  v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
  or <https://www.gnu.org/licenses/> for details.
-->

# GaCaSy Listener (PC side)

**Not built yet.** This folder will hold the PC-side component that auto-detects a
connected cartridge, verifies it, and launches the cartridge's launcher.

Planned as one Rust codebase, `cfg`-gated per OS, producing a Windows build and a
Linux build. A cartridge is **any mounted storage volume** (NVMe / SSD / HDD /
USB) — detection is "a new volume showed up", not a specific USB device id.

The two builds share their logic but **not their shape**: on Windows the listener
is a resident background process waiting on `WM_DEVICECHANGE` from login to logout,
while on Linux nothing runs at all until udev starts it on connect — it verifies,
launches, and exits. See
[Execution models](structure.md#execution-models) for why, and what each one costs.

See [`structure.md`](structure.md) for the full specification, including the
**Cartridge identification system** — the `.cartridge` key handshake this component
performs before launching anything.
