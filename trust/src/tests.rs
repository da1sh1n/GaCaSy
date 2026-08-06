// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every test in this crate, run against generated keys and real signatures.
//! Run with `cargo test -p trust`.

// ########## TRUST TESTS ##########

use crate::*;

const EXE: &[u8] = b"MZ\x90\x00 pretend this is a launcher";

// ========== Fixtures ==========

/// A generated key, split into the pieces the signing and checking sides need.
struct Key {
    secret: minisign::SecretKey,
    public: String,
}

/// Generates a fresh unencrypted keypair and pulls the bare base64 out of the
/// `.pub` text. Returns both halves.
fn aKey() -> Key {
    let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("keypair");
    let boxed = pair.pk.to_box().expect("public key").to_string();
    // The `.pub` format is a comment line then the key. Same rule as
    // `xtask::keys::base64Line`: take the last line that is neither blank nor
    // the comment, rather than trusting the line count.
    let public = boxed
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
        .expect("a key line")
        .to_string();
    Key {
        secret: pair.sk,
        public,
    }
}

/// Signs `EXE` with `key` the way `xtask sign` does, putting `trusted` in the
/// trusted comment. Returns the signed file. The untrusted comment carries
/// different text, so a tamper test shows which of the two it broke.
fn signedBy(key: &Key, trusted: &str) -> Vec<u8> {
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

/// A one-entry anchor list holding `key`'s public half, as a build's baked-in
/// anchors would look. The lifetime ties the result to `key`, so the borrow
/// cannot outlive the keypair it points into.
fn anchors(key: &Key) -> Vec<Anchor<'_>> {
    vec![Anchor {
        name: "dev",
        base64: &key.public,
    }]
}

// ========== Cases ==========

#[test]
fn a_signed_launcher_is_attested() {
    let key = aKey();
    let signed = signedBy(&key, "romzeta-launcher 0.2.1 2026-07-30");

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
    let key = aKey();
    let signed = signedBy(&key, "romzeta-installer 0.4.0 2026-07-30");

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
    let key = aKey();
    assert_eq!(
        attest(EXE, &anchors(&key), LAUNCHER_ROLE),
        Err(Refusal::Unsigned)
    );
}

#[test]
fn a_block_that_is_not_a_signature_is_malformed() {
    let key = aKey();
    let signed = sigblock::attach(EXE, "not a minisign signature at all\n");
    assert!(matches!(
        attest(&signed, &anchors(&key), LAUNCHER_ROLE),
        Err(Refusal::Malformed(_))
    ));
}

#[test]
fn a_flipped_payload_byte_is_refused() {
    let key = aKey();
    let mut signed = signedBy(&key, "romzeta-launcher 0.2.1 2026-07-30");
    signed[1] ^= 0xff;

    assert_eq!(
        attest(&signed, &anchors(&key), LAUNCHER_ROLE),
        Err(Refusal::Untrusted)
    );
}

#[test]
fn a_stranger_key_is_refused() {
    let ours = aKey();
    let theirs = aKey();
    // Signed correctly, with a role that would otherwise be exactly right.
    let signed = signedBy(&theirs, "romzeta-launcher 0.2.1 2026-07-30");

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
    let key = aKey();
    let signed = signedBy(&key, "romzeta-launcher 0.2.1 2026-07-30");
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
    let ours = aKey();
    let theirs = aKey();
    let signed = signedBy(&theirs, "romzeta-installer 0.4.0 2026-07-30");

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
    let key = aKey();
    let signed = signedBy(&key, "romzeta-launcher 0.2.1 2026-07-30");

    assert_eq!(attest(&signed, &[], LAUNCHER_ROLE), Err(Refusal::Untrusted));
}
