// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Converts between Rust strings and the NUL-terminated UTF-16 that Win32 `…W`
//! entry points take and return.

// ########## UTF-16 FOR WIN32 ##########

/// `text` as a NUL-terminated UTF-16 buffer, ready to pass to a `…W` function.
/// The returned `Vec` owns the bytes, so it has to stay alive for the whole
/// call — `f(wide(s).as_ptr())` is fine, but binding the pointer alone dangles.
pub fn wide(text: &str) -> Vec<u16> {
    // Rust strings carry a length instead of a terminator, so the NUL is never
    // already there and `chain` has to append it.
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A wide buffer Windows filled in, as a `String`, stopping at the first NUL.
/// Win32 writes into a fixed-size buffer and leaves the tail as it found it, so
/// the whole slice is almost never the string.
pub fn fromWide(buffer: &[u16]) -> String {
    // No NUL at all means the API used every slot; take the lot.
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    // `_lossy` rather than the checked form: an unpaired surrogate coming back
    // from the OS is a filename we should still be able to show, not an error.
    String::from_utf16_lossy(&buffer[..end])
}
