// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Crate root. Declares the four shared modules: log writing, UTC time, Win32
//! UTF-16 conversion, and the `x.y.z` version type.

// Functions are camelCase in this project while variables stay snake_case, which
// is the opposite of what rustc expects. Silenced once, at the root, so no item
// below needs its own attribute.
#![allow(non_snake_case)]

// ########## SHARED PIECES ##########

pub mod log;
pub mod time;
pub mod version;

// Only the Windows builds ever call a `…W` entry point, and on Linux this module
// would be dead code the compiler warns about.
#[cfg(windows)]
pub mod utf16;
