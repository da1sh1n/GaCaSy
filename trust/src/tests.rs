// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every test in this crate.
//!
//! These run against real keys and real signatures — generated here, signed with
//! the reference implementation, checked with the verify-only one the shipped
//! programs link. A test that faked either half would prove that this crate
//! agrees with itself, which is not the property anyone needs.
//!
//! Run with `cargo test -p trust`.

use crate::*;

const EXE: &[u8] = b"MZ\x90\x00 pretend this is a launcher";

/// A generated key, and the pieces of it the two sides need.
struct Key {
    secret: minisign::SecretKey,
    public: String,
}

fn a_key() -> Key {
    let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("keypair");
    let boxed = pair.pk.to_box().expect("public key").to_string();
    // The `.pub` format is a comment line then the key. Same rule as
    // `xtask::keys::base64_line`: take the last line that is neither blank nor
    // the comment, rather than trusting the line count.
    let public = boxed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
        .next_back()
        .expect("a key line")
        .to_string();
    Key {
        secret: pair.sk,
        public,
    }
}

/// Signs `EXE` the way `xtask sign` does, trusted comment and all.
///
/// The untrusted comment is deliberately something else: it is not covered by
/// any signature, and a test that put the same text in both could not tell
/// which one the tamper test actually broke.
fn signed_by(key: &Key, trusted: &str) -> Vec<u8> {
    let signature = minisign::sign(
        None,
        &key.secret,
        EXE,
        Some(trusted),
        Some("signature from romzeta"),
    )
    .expect("sign")
    .into_string();
    sigblock::attach(EXE, &signature)
}

fn anchors(key: &Key) -> Vec<Anchor<'_>> {
    vec![Anchor {
        name: "dev",
        base64: &key.public,
    }]
}

#[test]
fn a_signed_launcher_is_attested() {
    let key = a_key();
    let signed = signed_by(&key, "romzeta-launcher 0.2.1 2026-07-30");

    let attested = attest(&signed, &anchors(&key), LAUNCHER_ROLE).expect("attested");
    assert_eq!(attested.anchor, "dev");
    assert_eq!(attested.role, "romzeta-launcher");
    assert_eq!(attested.version, "0.2.1");
}

#[test]
fn a_signed_installer_is_not_a_launcher() {
    // The finding this crate exists for. All three binaries are signed with one
    // key, so "signed by us" was never the same question as "is a launcher" —
    // and renaming installer.exe to launcher.exe used to be enough.
    let key = a_key();
    let signed = signed_by(&key, "romzeta-installer 0.4.0 2026-07-30");

    assert_eq!(
        attest(&signed, &anchors(&key), LAUNCHER_ROLE),
        Err(Refusal::WrongRole {
            expected: LAUNCHER_ROLE.to_string(),
            found: INSTALLER_ROLE.to_string(),
        })
    );
}

#[test]
fn an_unsigned_binary_is_refused() {
    let key = a_key();
    assert_eq!(
        attest(EXE, &anchors(&key), LAUNCHER_ROLE),
        Err(Refusal::Unsigned)
    );
}

#[test]
fn a_block_that_is_not_a_signature_is_malformed() {
    let key = a_key();
    let signed = sigblock::attach(EXE, "not a minisign signature at all\n");
    assert!(matches!(
        attest(&signed, &anchors(&key), LAUNCHER_ROLE),
        Err(Refusal::Malformed(_))
    ));
}

#[test]
fn a_flipped_payload_byte_is_refused() {
    let key = a_key();
    let mut signed = signed_by(&key, "romzeta-launcher 0.2.1 2026-07-30");
    signed[1] ^= 0xff;

    assert_eq!(
        attest(&signed, &anchors(&key), LAUNCHER_ROLE),
        Err(Refusal::Untrusted)
    );
}

#[test]
fn a_stranger_key_is_refused() {
    let ours = a_key();
    let theirs = a_key();
    // Signed correctly, with a role that would otherwise be exactly right.
    let signed = signed_by(&theirs, "romzeta-launcher 0.2.1 2026-07-30");

    assert_eq!(
        attest(&signed, &anchors(&ours), LAUNCHER_ROLE),
        Err(Refusal::Untrusted)
    );
}

#[test]
fn the_trusted_comment_cannot_be_edited_after_signing() {
    // The property everything above rests on. minisign signs the comment with a
    // second signature over `signature ‖ comment`, so rewriting the version (or
    // the role) in a signed file has to invalidate it. If this test ever fails,
    // reading the comment is reading attacker-controlled text and `attest` is
    // worthless.
    let key = a_key();
    let signed = signed_by(&key, "romzeta-launcher 0.2.1 2026-07-30");
    assert!(attest(&signed, &anchors(&key), LAUNCHER_ROLE).is_ok());

    // Edit the comment inside the signature block and put the file back
    // together, leaving the payload and both base64 signatures untouched. Same
    // length, so nothing shifts.
    let (payload, signature) = sigblock::split(&signed);
    let signature = signature.expect("the fixture is signed");
    let edited = signature.replace("romzeta-launcher 0.2.1", "romzeta-launcher 9.9.9");
    assert_ne!(edited, signature, "the fixture did not contain the comment");
    let tampered = sigblock::attach(payload, &edited);

    assert_eq!(
        attest(&tampered, &anchors(&key), LAUNCHER_ROLE),
        Err(Refusal::Untrusted)
    );
}

#[test]
fn the_role_is_not_consulted_before_the_signature() {
    // Order matters: a stranger's binary claiming to be a launcher must come
    // back `Untrusted` and never `WrongRole`, because the second answer would
    // mean the comment had been read and believed on an unverified file.
    let ours = a_key();
    let theirs = a_key();
    let signed = signed_by(&theirs, "romzeta-installer 0.4.0 2026-07-30");

    assert_eq!(
        attest(&signed, &anchors(&ours), LAUNCHER_ROLE),
        Err(Refusal::Untrusted)
    );
}

#[test]
fn no_anchors_means_nothing_is_trusted() {
    // A build that lost its keys must refuse everything rather than accept
    // anything. Both build scripts make this a build error; this makes sure the
    // runtime answer is the safe one regardless.
    let key = a_key();
    let signed = signed_by(&key, "romzeta-launcher 0.2.1 2026-07-30");

    assert_eq!(attest(&signed, &[], LAUNCHER_ROLE), Err(Refusal::Untrusted));
}
