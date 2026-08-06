// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Wraps the registry calls this crate uses: open, create, read and write
//! string values, enumerate names, delete.
//!
//! Two Win32 conventions: sizes are **bytes** while lengths are `u16` counts
//! (`RegEnumValueW` is the exception), and a `None` name addresses a key's
//! unnamed `(Default)` value with a null pointer.

// ########## THE REGISTRY ##########

#![cfg(windows)]

use std::ptr;

use common::utf16::wide;

use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};

pub use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

/// Access mask for a key that is only going to be read.
pub const READ: u32 = KEY_QUERY_VALUE;
/// Access mask for a key that is going to be written or have values deleted.
pub const WRITE: u32 = KEY_SET_VALUE;

/// An open key. RAII so that every early return closes it.
pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { RegCloseKey(self.0) };
        }
    }
}

/// Opens an existing key. `None` when it isn't there or isn't ours to open,
/// which for every caller here means "nothing has been set" rather than an
/// error worth reporting.
pub fn open(root: HKEY, path: &str, access: u32) -> Option<Key> {
    let mut handle: HKEY = ptr::null_mut();
    let ok = unsafe { RegOpenKeyExW(root, wide(path).as_ptr(), 0, access, &mut handle) };
    (ok == ERROR_SUCCESS).then_some(Key(handle))
}

/// Opens a key, creating it and any missing parents. Needed because some of the
/// AutoPlay keys only exist on a profile where the setting has been touched
/// before, and because our own backup key never exists the first time.
pub fn create(root: HKEY, path: &str) -> Result<Key, String> {
    let mut handle: HKEY = ptr::null_mut();
    let ok = unsafe {
        RegCreateKeyExW(
            root,
            wide(path).as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            ptr::null(),
            &mut handle,
            ptr::null_mut(),
        )
    };
    if ok == ERROR_SUCCESS {
        Ok(Key(handle))
    } else {
        Err(format!(r"HKCU\{path} could not be opened (error {ok})."))
    }
}

/// Writes a string value. `name` of `None` writes the key's `(Default)` value.
pub fn setSz(key: &Key, name: Option<&str>, value: &str) -> Result<(), String> {
    let label = name.unwrap_or("(Default)").to_owned();
    let name = name.map(wide);
    let value = wide(value);
    let ok = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ref().map_or(ptr::null(), |n| n.as_ptr()),
            0,
            REG_SZ,
            value.as_ptr() as *const u8,
            // Bytes, and the terminator counts — a REG_SZ written without it
            // reads back with whatever followed it in the hive attached.
            (value.len() * 2) as u32,
        )
    };
    if ok == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "The registry value {label} could not be written (error {ok})."
        ))
    }
}

/// Reads a string value. `name` of `None` reads the key's `(Default)` value.
///
/// `None` for a value that isn't there, isn't a string, or is longer than a
/// registry value has any business being — all of which mean "not something we
/// wrote" to every caller.
pub fn querySz(key: &Key, name: Option<&str>) -> Option<String> {
    let name = name.map(wide);
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    let ok = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ref().map_or(ptr::null(), |n| n.as_ptr()),
            ptr::null(),
            ptr::null_mut(),
            buffer.as_mut_ptr() as *mut u8,
            &mut size,
        )
    };
    if ok != ERROR_SUCCESS {
        return None;
    }
    // A REG_SZ is *usually* terminated in the hive and occasionally isn't, so
    // the terminator is a stopping point rather than something to rely on.
    let chars = (size as usize / 2).min(buffer.len());
    let end = buffer[..chars]
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(chars);
    Some(String::from_utf16_lossy(&buffer[..end]))
}

/// Every value name under `key`, in the order the hive hands them over.
///
/// Only `crate::font` needs this, because the font list names its values after
/// the *faces* inside a file rather than after the file — a `.ttc` holding
/// three is one value named after all three — so there is no name to ask for.
/// A name too long for the buffer is skipped rather than truncated, since a
/// truncated one would match the wrong font.
pub fn enumValueNames(key: &Key) -> Vec<String> {
    let mut names = Vec::new();
    let mut buffer = [0u16; 512];

    for index in 0.. {
        // Characters, not bytes, and reset every time round: the call writes the
        // length it used back into this same variable.
        let mut len = buffer.len() as u32;
        let ok = unsafe {
            RegEnumValueW(
                key.0,
                index,
                buffer.as_mut_ptr(),
                &mut len,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        match ok {
            ERROR_SUCCESS => names.push(String::from_utf16_lossy(&buffer[..len as usize])),
            // The name alone was too long for 512 characters. Keep going: the
            // index still advances, so this skips one value rather than ending
            // the walk early.
            ERROR_MORE_DATA => continue,
            // ERROR_NO_MORE_ITEMS, or a key that went away underneath us.
            _ => break,
        }
    }
    names
}

/// Removes a value. A value that was already absent is the desired end state,
/// so nothing is reported either way.
pub fn deleteValue(key: &Key, name: Option<&str>) {
    let name = name.map(wide);
    unsafe {
        RegDeleteValueW(key.0, name.as_ref().map_or(ptr::null(), |n| n.as_ptr()));
    }
}

/// Removes a key that has no subkeys. Called on the AutoPlay backup key once
/// its contents have been restored.
pub fn deleteKey(root: HKEY, path: &str) {
    unsafe {
        RegDeleteKeyW(root, wide(path).as_ptr());
    }
}
