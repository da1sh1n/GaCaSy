// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Platform triggers — the only part of the listener that differs per OS.
//!
//! What is `cfg`-gated here is **the process lifetime**, not just which API
//! notices a volume. The Windows trigger is resident from login to logout and
//! blocks in `GetMessage`; the Linux trigger does not exist until udev starts
//! it, and exits once it has acted. See `../../structure.md`
//! ("Execution models").
//!
//! Both end at the same place: [`crate::volume::handle_volume`]. Nothing about
//! signatures, versions or launching lives in this folder.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::run;

#[cfg(not(windows))]
mod linux;
#[cfg(not(windows))]
pub use linux::run;
