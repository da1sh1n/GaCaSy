// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Copy and paste, for the one text field in this program.
//!
//! There is exactly one place to type in the whole wizard — a game's name, on the
//! games screen — and a name is precisely the kind of thing someone pastes. So
//! this exists.
//!
//! **Why it is hand-rolled.** egui's winit integration has clipboard support
//! already, behind a feature flag; turning that flag on pulls in `arboard`, which
//! egui asks for with `image-data`, which drags the whole `image` and `png` stack
//! into the binary so that someone can paste a *picture* into a field that holds
//! a game's name. That is around a megabyte for a feature this program does not
//! have. Two Win32 calls in each direction is the cheaper trade — see
//! [`crate::shell`] for where they are wired in.
//!
//! Every failure is silent. A clipboard that is locked by another process, or
//! holds a bitmap, means the keystroke does nothing — which is what the user
//! would see from any other program in the same moment.

#![cfg(windows)]

use std::ptr;

use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

/// Closes the clipboard however the block it guards is left.
///
/// Every path out of the two functions below has to close it — an early return
/// that forgets leaves the clipboard locked against every other program on the
/// desktop until this process exits.
struct Session;

impl Session {
    /// `None` when another process is holding it. Nothing to do but let the
    /// keystroke go nowhere.
    fn open() -> Option<Session> {
        (unsafe { OpenClipboard(ptr::null_mut()) } != 0).then_some(Session)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

/// The clipboard as text, if it holds any.
pub fn get() -> Option<String> {
    let _session = Session::open()?;

    // Not ours to free, and only valid until the clipboard is closed — hence the
    // copy into a `String` before `_session` drops.
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT as u32) };
    if handle.is_null() {
        return None;
    }
    let text = unsafe { GlobalLock(handle) }.cast::<u16>();
    if text.is_null() {
        return None;
    }

    // The API gives no length, only a terminator. The cap is not a limit anyone
    // is expected to reach; it is what stops a clipboard without a terminator
    // from reading the rest of the address space.
    const CAP: usize = 64 * 1024;
    let mut units = Vec::new();
    for offset in 0..CAP {
        let unit = unsafe { *text.add(offset) };
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    unsafe { GlobalUnlock(handle) };

    Some(String::from_utf16_lossy(&units))
}

/// Puts text on the clipboard, replacing whatever was there.
pub fn set(text: &str) {
    let Some(_session) = Session::open() else {
        return;
    };

    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // Bytes, and the terminator is part of what gets copied.
    let bytes = std::mem::size_of_val(wide.as_slice());
    let block = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    if block.is_null() {
        return;
    }

    let target = unsafe { GlobalLock(block) }.cast::<u16>();
    if target.is_null() {
        unsafe { GlobalFree(block) };
        return;
    }
    unsafe { ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len()) };
    unsafe { GlobalUnlock(block) };

    // The clipboard must be emptied before it will take ownership of the block,
    // and it only takes ownership if this succeeds — so a failure here is the one
    // case where freeing the block is still ours to do.
    unsafe { EmptyClipboard() };
    if unsafe { SetClipboardData(CF_UNICODETEXT as u32, block as HANDLE) }.is_null() {
        unsafe { GlobalFree(block) };
    }
}
