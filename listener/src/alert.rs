// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The one thing the listener says out loud.
//!
//! This program is silent by design — no console, no window, everything in the
//! log. That is right for the cases the user cannot act on: an ordinary drive
//! with no launcher, a debounced repeat arrival, a cartridge signed by a
//! stranger. Popping a box for any of those would mean interrupting someone for
//! plugging in a USB stick.
//!
//! A version mismatch is the exception. The cartridge *is* one of ours, it *is*
//! correctly signed, and the person holding it has every reason to expect it to
//! work — so "nothing happened" is the one outcome they would be right to read
//! as the software being broken. There is also something they can do about it,
//! which is what makes telling them worth the interruption.

/// Shows a warning the user can dismiss, and returns immediately.
///
/// **Never blocks the caller.** On Windows the listener spends its whole life in
/// `GetMessage` on one thread; a modal box shown from there would hold up every
/// later device arrival for as long as the box stayed open, so this hands it to
/// a thread of its own and lets it outlive the call.
#[cfg(windows)]
pub fn warn(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_SYSTEMMODAL, MessageBoxW,
    };

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

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// No portable way to raise a dialog from a headless process, and the Linux
/// trigger is one-shot from udev with no session to show it in anyway (see
/// `trigger/linux.rs`). The log carries the same sentence.
#[cfg(not(windows))]
pub fn warn(_title: &str, _message: &str) {}
