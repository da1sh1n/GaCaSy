// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Selects the platform trigger at compile time. Both halves export `run`.

// ########## PLATFORM TRIGGERS ##########

// Exactly one of these two is compiled, and both export `run`, so the rest of
// the crate calls `trigger::run` without knowing which platform it is on.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::run;

#[cfg(not(windows))]
mod linux;
#[cfg(not(windows))]
pub use linux::run;
