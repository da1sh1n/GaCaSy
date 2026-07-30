// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every test in this crate.
//!
//! Kept inside the crate rather than in `tests/` so that the tests can reach
//! internals like [`MAGIC`] without those becoming public API. The signature
//! block's whole job is to be a tight, auditable format definition; widening it
//! for a test's convenience would be the wrong trade.
//!
//! Run with `cargo test -p sigblock`.

use crate::*;

const EXE: &[u8] = b"MZ\x90\x00 not really a PE, but neither is anything else here";
const SIG: &str = "untrusted comment: signature from gacasy\nRUQf6LRCGA9i53==\n\
                   trusted comment: gacasy-launcher 0.2.0\nAbCd==\n";

#[test]
fn round_trips() {
    let signed = attach(EXE, SIG);
    let (payload, signature) = split(&signed);
    assert_eq!(payload, EXE);
    assert_eq!(signature, Some(SIG));
}

#[test]
fn an_ordinary_binary_is_unsigned() {
    let (payload, signature) = split(EXE);
    assert_eq!(payload, EXE);
    assert_eq!(signature, None);
    assert!(!is_signed(EXE));
}

#[test]
fn re_signing_replaces_instead_of_nesting() {
    // The trap: if `attach` appended to the already-signed file, the second
    // block's payload would contain the first block, the exe would grow
    // without bound, and verification would be against bytes no linker ever
    // produced.
    let once = attach(EXE, SIG);
    let twice = attach(&once, "untrusted comment: a different one\nZZ==\n");
    let (payload, signature) = split(&twice);
    assert_eq!(payload, EXE);
    assert_eq!(
        signature,
        Some("untrusted comment: a different one\nZZ==\n")
    );
    assert!(twice.len() < once.len() + SIG.len());
}

#[test]
fn a_truncated_block_reads_as_unsigned() {
    let signed = attach(EXE, SIG);
    // Losing the footer is exactly what "truncate the last 16 bytes" does.
    assert!(!is_signed(&signed[..signed.len() - FOOTER_LEN]));
    // Losing one byte of it is subtler and must fail just as quietly.
    assert!(!is_signed(&signed[..signed.len() - 1]));
}

#[test]
fn a_short_file_cannot_panic() {
    for len in 0..=FOOTER_LEN {
        let (payload, signature) = split(&EXE[..len]);
        assert_eq!(payload.len(), len);
        assert_eq!(signature, None);
    }
}

#[test]
fn a_length_larger_than_the_file_is_refused() {
    // The interesting hostile input: a real magic and a length that would
    // slice before the start of the buffer.
    let mut signed = attach(EXE, SIG);
    let at = signed.len() - FOOTER_LEN;
    signed[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(!is_signed(&signed));
}

#[test]
fn a_future_format_is_left_alone() {
    let mut signed = attach(EXE, SIG);
    let at = signed.len() - FOOTER_LEN;
    signed[at + 4..at + 6].copy_from_slice(&2u16.to_le_bytes());
    assert!(!is_signed(&signed));
}

#[test]
fn a_non_utf8_signature_is_refused() {
    let mut signed = attach(EXE, SIG);
    let at = signed.len() - FOOTER_LEN - SIG.len();
    signed[at] = 0xff;
    assert!(!is_signed(&signed));
}

#[test]
fn the_magic_alone_is_not_a_block() {
    // Sixteen bytes of coincidence at the end of some unrelated file.
    let mut coincidence = EXE.to_vec();
    coincidence.extend_from_slice(MAGIC);
    assert!(!is_signed(&coincidence));
}

#[test]
fn an_empty_payload_is_still_addressable() {
    let signed = attach(b"", SIG);
    let (payload, signature) = split(&signed);
    assert!(payload.is_empty());
    assert_eq!(signature, Some(SIG));
}
