// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Reads and writes the signature block appended past the end of an executable:
//!
//! ```text
//! [ the exe exactly as the linker produced it        ]  <- the signed bytes
//! [ minisig text, N bytes UTF-8 (the 2-line format)  ]
//! [ N            u32 little-endian                   ]  }
//! [ format       u16 little-endian, = 1              ]  } 16-byte footer
//! [ magic        b"ROMZETASIG" (10 bytes)            ]  }
//! ```
//!
//! Finding a block says only where the signature is; verifying it is `trust`.

// Functions are camelCase in this project while variables stay snake_case,
// which rustc's default lints object to. Silenced once, at the crate root.
#![allow(non_snake_case)]

// ########## THE SIGNATURE BLOCK ##########

/// Identifies the footer. Ten bytes exactly, which keeps the whole thing 16.
const MAGIC: &[u8; 10] = b"ROMZETASIG";

/// The only block format that exists. A block declaring anything else is left
/// alone rather than guessed at, since a future format would move these fields.
const FORMAT: u16 = 1;

/// Size of the fixed footer: length (4) + format (2) + magic (10).
pub const FOOTER_LEN: usize = 16;

// ========== Reading ==========

/// Splits a signed binary into `(signed bytes, signature text)`.
/// Anything that is not a well-formed block comes back as `(bytes, None)` —
/// there is no `Result` here, because a truncated file, a bogus length and no
/// block at all all mean the same thing to every caller, and this runs against
/// whatever a stranger just plugged into the machine.
pub fn split(bytes: &[u8]) -> (&[u8], Option<&str>) {
    let unsigned = (bytes, None);

    // `checked_sub` rather than `-`: a file shorter than the footer would
    // underflow and panic, and an empty file is an ordinary thing to be handed.
    let Some(footer_at) = bytes.len().checked_sub(FOOTER_LEN) else {
        return unsigned;
    };
    let footer = &bytes[footer_at..];

    // Magic first — it is the cheapest way to reject the overwhelming majority.
    if &footer[6..] != MAGIC {
        return unsigned;
    }
    if u16::from_le_bytes([footer[4], footer[5]]) != FORMAT {
        return unsigned;
    }

    let len = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]);
    // Compared as u64, because `as usize` narrows on a 32-bit target: a length
    // of 2^32 + 8 would wrap to 8 there and sail through the bounds check.
    if u64::from(len) > footer_at as u64 {
        return unsigned;
    }
    let signature_at = footer_at - len as usize;

    // A minisig is text, so a block whose bytes are not UTF-8 was never ours.
    match str::from_utf8(&bytes[signature_at..footer_at]) {
        Ok(signature) => (&bytes[..signature_at], Some(signature)),
        Err(_) => unsigned,
    }
}

/// True when `bytes` carries a block at all. Says nothing about whether that
/// block verifies against any key.
pub fn isSigned(bytes: &[u8]) -> bool {
    split(bytes).1.is_some()
}

// ========== Writing ==========

/// Builds the file to write from `bytes` and a `signature`.
/// Any block already on `bytes` is stripped first, so re-signing replaces the
/// old signature instead of burying it inside the new signed payload — which
/// matters because `cargo build` and `xtask sign` both write the same file.
pub fn attach(bytes: &[u8], signature: &str) -> Vec<u8> {
    let (payload, _) = split(bytes);
    let len = u32::try_from(signature.len()).expect("a minisig is ~200 bytes, not 4 GB");

    // Exact capacity, so the three extends below never reallocate.
    let mut out = Vec::with_capacity(payload.len() + signature.len() + FOOTER_LEN);
    out.extend_from_slice(payload);
    out.extend_from_slice(signature.as_bytes());
    // Little-endian throughout, matching what `split` reads back.
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(MAGIC);
    out
}

// ========== The Command Line ==========

/// The `--signature` plumbing every Romzeta program shares, kept here rather
/// than copied into each one so the three cannot drift apart.
pub mod cli {
    /// This executable's own signature block, or `None` when it has none or
    /// cannot read itself.
    pub fn ownSignature() -> Option<String> {
        // Chained `?` on Options: any step failing means "no signature", which
        // is the same answer as an unsigned exe and needs no separate branch.
        let bytes = std::fs::read(std::env::current_exe().ok()?).ok()?;
        super::split(&bytes).1.map(str::to_string)
    }

    /// Prints this exe's signature block, or the word `unsigned`.
    /// For a human checking a download by hand — nothing in Romzeta establishes
    /// another program's identity by running it and reading this, because that
    /// would mean executing the very binary you are still deciding about.
    pub fn printSignature() {
        match ownSignature() {
            // `print!`, not `println!`: a minisig block already ends in a newline.
            Some(signature) => print!("{signature}"),
            None => println!("unsigned"),
        }
    }

    /// Reattaches this process to the console that started it, so `println!`
    /// from a `windows_subsystem = "windows"` binary reaches a terminal.
    /// Failure is the ordinary path — nothing launched us from a console — and
    /// is ignored. Being *probed* by another process does not need this: there
    /// the parent supplies a real pipe and stdout is valid either way.
    #[cfg(windows)]
    pub fn attachConsole() {
        // Declared by hand rather than pulling in windows-sys: this crate links
        // into build scripts and into a listener that advertises a two-crate
        // dependency tree, and one FFI line is the better trade.
        unsafe extern "system" {
            // Fixed by the Win32 API, so it keeps Microsoft's spelling.
            fn AttachConsole(process_id: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }

    #[cfg(not(windows))]
    pub fn attachConsole() {}
}

#[cfg(test)]
mod tests;
