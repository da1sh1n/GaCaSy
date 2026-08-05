// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The signature block — how a Romzeta binary carries its own minisign
//! signature, with no file beside it.
//!
//! A cartridge is one file. That is the whole point: no `.cartridge` marker to
//! copy, no `.minisig` sidecar to lose, nothing to keep in step with the exe.
//! So the signature lives *in* the exe.
//!
//! # Why it is appended rather than reserved
//!
//! You cannot sign bytes that already contain their own signature — writing the
//! signature in would change what was signed. The two ways out are to reserve a
//! blank region, sign with it blank, then fill it in (and blank it again before
//! verifying), or to append the signature past the end. This crate appends:
//!
//! ```text
//! [ the exe exactly as the linker produced it        ]  <- the signed bytes
//! [ minisig text, N bytes UTF-8 (the 2-line format)  ]
//! [ N            u32 little-endian                   ]  }
//! [ format       u16 little-endian, = 1              ]  } 16-byte footer
//! [ magic        b"ROMZETASIG" (10 bytes)            ]  }
//! ```
//!
//! The signed bytes are then byte-for-byte the linker's output, so verifying is
//! "chop off the footer and check what is left" — there is nothing to zero out
//! first, and no scanning the whole image for a marker that could also occur in
//! the middle of a legitimate `.rdata`. PE and ELF both ignore trailing data: it
//! sits outside every section header and is never mapped, so a signed exe runs
//! exactly as the unsigned one did. Authenticode does structurally the same
//! thing with its certificate table.
//!
//! # Why every failure is "unsigned" and never a panic
//!
//! [`split`] runs against whatever a stranger just plugged into the machine. A
//! truncated file, a length field claiming four gigabytes, sixteen bytes of
//! coincidence — all of them mean "this is not a signed Romzeta binary", which is
//! the same answer as an ordinary drive gets, and none of them are worth taking
//! the listener down over. There is deliberately no `Result` here: the caller
//! has exactly one thing to do about a bad block, and it is what it would do
//! about no block at all.
//!
//! Finding a block still proves nothing. It only says where the signature is;
//! whether it *verifies* is [`minisign_verify`](https://docs.rs/minisign-verify)'s
//! job, and the listener's.

/// Identifies the footer. Ten bytes exactly, which keeps the whole thing 16.
const MAGIC: &[u8; 10] = b"ROMZETASIG";

/// The only block format that exists. A future one would change the layout of
/// everything above the magic, so a block that declares anything else is left
/// alone rather than guessed at.
const FORMAT: u16 = 1;

/// Size of the fixed footer: length (4) + format (2) + magic (10).
pub const FOOTER_LEN: usize = 16;

/// Splits a signed binary into `(signed bytes, signature text)`.
///
/// Returns `(bytes, None)` for anything that is not a well-formed block —
/// see the module docs on why that is not an error.
pub fn split(bytes: &[u8]) -> (&[u8], Option<&str>) {
    let unsigned = (bytes, None);

    let Some(footer_at) = bytes.len().checked_sub(FOOTER_LEN) else {
        return unsigned;
    };
    let footer = &bytes[footer_at..];

    if &footer[6..] != MAGIC {
        return unsigned;
    }
    if u16::from_le_bytes([footer[4], footer[5]]) != FORMAT {
        return unsigned;
    }

    // `as usize` is a narrowing cast on a 32-bit target, so compare in u64 —
    // otherwise a length of 2^32+8 would wrap to 8 and pass the bounds check.
    let len = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]);
    if u64::from(len) > footer_at as u64 {
        return unsigned;
    }
    let signature_at = footer_at - len as usize;

    match str::from_utf8(&bytes[signature_at..footer_at]) {
        Ok(signature) => (&bytes[..signature_at], Some(signature)),
        Err(_) => unsigned,
    }
}

/// True when `bytes` carries a block. Says nothing about whether it verifies.
pub fn is_signed(bytes: &[u8]) -> bool {
    split(bytes).1.is_some()
}

/// Builds the file to write from signed bytes and a signature.
///
/// Any existing block is stripped first, so signing an already-signed binary
/// replaces its signature instead of burying the old one inside the new signed
/// payload. That matters because `cargo build` and `xtask sign` both write to
/// `target/release/`, in either order, as often as you like.
pub fn attach(bytes: &[u8], signature: &str) -> Vec<u8> {
    let (payload, _) = split(bytes);
    let len = u32::try_from(signature.len()).expect("a minisig is ~200 bytes, not 4 GB");

    let mut out = Vec::with_capacity(payload.len() + signature.len() + FOOTER_LEN);
    out.extend_from_slice(payload);
    out.extend_from_slice(signature.as_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(MAGIC);
    out
}

/// The `--version` / `--signature` plumbing every Romzeta program shares.
///
/// All three are GUI-subsystem binaries that nonetheless have to answer two
/// questions on a command line, and all three answer them the same way. Kept
/// here rather than copied into each because the copies would drift, and a
/// launcher whose `--version` output the listener cannot parse is a cartridge
/// that stops working for no visible reason.
pub mod cli {
    /// This executable's own signature block, or `None` when it has none.
    pub fn own_signature() -> Option<String> {
        let bytes = std::fs::read(std::env::current_exe().ok()?).ok()?;
        super::split(&bytes).1.map(str::to_string)
    }

    /// Prints the signature block, or `unsigned`.
    ///
    /// Note what this is *not* for. Nothing in Romzeta establishes another
    /// program's identity by running it and reading this — a binary reporting
    /// its own trustworthiness proves nothing, and asking would mean executing
    /// the very thing you are deciding whether to execute. Signatures are
    /// checked by reading the file. This output is for a human.
    pub fn print_signature() {
        match own_signature() {
            Some(signature) => print!("{signature}"),
            None => println!("unsigned"),
        }
    }

    /// Lets a GUI-subsystem process print to the terminal that started it.
    ///
    /// `windows_subsystem = "windows"` means Windows allocates no console, so a
    /// `println!` from an exe launched at a prompt goes to a handle that leads
    /// nowhere. Attaching to the parent's console fixes the human case.
    ///
    /// It has no bearing on one program *probing* another: there the parent
    /// creates a real pipe and passes it in `STARTUPINFO`, so the child's stdout
    /// is valid either way. Failure here is the ordinary path — nothing launched
    /// us from a console — and is ignored.
    #[cfg(windows)]
    pub fn attach_console() {
        // Declared directly rather than via windows-sys: this crate is linked
        // into a build script and into a listener that advertises a two-crate
        // dependency tree, and one FFI line is a better trade than a dependency.
        unsafe extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }

    #[cfg(not(windows))]
    pub fn attach_console() {}
}

#[cfg(test)]
mod tests;
