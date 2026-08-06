// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Gets and sets clipboard text through Win32. Every failure is silent.

// ########## CLIPBOARD ##########

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
