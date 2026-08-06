// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Shows a modal warning box on a thread of its own, so the caller never
//! blocks. A no-op off Windows.

// ########## THE ONE WARNING ##########

/// Shows a warning the user can dismiss, and returns immediately.
///
/// **Never blocks the caller.** On Windows the listener spends its whole life
/// in `GetMessage` on one thread, so a modal box shown from there would hold up
/// every later device arrival until it was closed. This hands it to a thread of
/// its own and lets that thread outlive the call.
#[cfg(windows)]
pub fn warn(title: &str, message: &str) {
    use common::utf16::wide;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_SYSTEMMODAL, MessageBoxW,
    };

    // Converted before the spawn: the buffers must be owned by the closure, and
    // `&str` borrowed from the caller could not be moved into a thread that
    // outlives this call.
    let title = wide(title);
    let message = wide(message);
    std::thread::spawn(move || {
        // MB_SETFOREGROUND and MB_SYSTEMMODAL because this box has no owner
        // window to sit in front of — without them it can open behind whatever
        // is fullscreen, which for a games launcher is most of the time.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_SYSTEMMODAL,
            );
        }
    });
}

/// No portable way to raise a dialog from a headless process, and the Linux
/// trigger is one-shot from udev with no session to show one in. The log
/// carries the same sentence.
#[cfg(not(windows))]
pub fn warn(_title: &str, _message: &str) {}
