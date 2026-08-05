// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Keeping a second launcher from opening on top of the first.
//!
//! A named mutex rather than a socket, so nothing ever binds a port: the OS
//! releases the name when the process dies, which also means a crashed
//! launcher leaves nothing behind to clean up.
//!
//! Only the deployed exe takes the lock — see `main`.

/// Holds the process-wide single-instance mutex; releasing it (on drop or
/// process exit) frees the name for the next launch.
#[cfg(windows)]
pub struct InstanceGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }
}

/// Returns `Some(guard)` if this is the first instance, `None` if another
/// is already running.
#[cfg(windows)]
pub fn acquire() -> Option<InstanceGuard> {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Local\\Romzeta.CartridgeLauncher\0"
        .encode_utf16()
        .collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if handle.is_null() {
            // Couldn't create the mutex at all; don't block launching.
            return Some(InstanceGuard(handle));
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            windows_sys::Win32::Foundation::CloseHandle(handle);
            return None;
        }
        Some(InstanceGuard(handle))
    }
}

#[cfg(not(windows))]
pub struct InstanceGuard;

#[cfg(not(windows))]
pub fn acquire() -> Option<InstanceGuard> {
    Some(InstanceGuard)
}
