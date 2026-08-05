// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Whether a binary may be run, and what it turned out to be.
//!
//! Two programs decide this: the listener, before starting a launcher off a
//! volume that just arrived, and the installer, before believing anything about
//! a `launcher.exe` already sitting on a cartridge. They used to decide it
//! differently — the listener verified a signature, and the installer executed
//! the file and asked. That is not a difference of opinion, it is the second
//! one being wrong, and the reason it was wrong is that the check lived in the
//! listener rather than anywhere both could reach.
//!
//! So it lives here, and there is exactly one of it.
//!
//! # What a signature actually establishes
//!
//! [`attest`] answers three questions, in this order, and the order is the
//! point:
//!
//! 1. **Is this signed by a key we trust?** Ed25519 over the whole binary, via
//!    [`minisign_verify`]. Until this passes, nothing else on the file means
//!    anything.
//! 2. **What does it say it is?** minisign signs a *trusted comment* alongside
//!    the payload — a second signature over `signature ‖ comment`, checked by
//!    the same `verify` call as step 1. `xtask` writes `<role> <version> <date>`
//!    there. Because it is covered by that second signature, it is as
//!    unforgeable as the payload; because it is *only* covered once step 1 has
//!    passed, reading it before then would be reading attacker-controlled text.
//! 3. **Is that the thing we asked for?** A signed `installer.exe` is a
//!    perfectly genuine Romzeta binary and is not a launcher. Without this step
//!    the trust decision is "signed by us", which is not the same as "is the
//!    program the caller is about to run" — see [`Refusal::WrongRole`].
//!
//! # What it does not establish
//!
//! Only that *this file* is ours and unmodified. The signature says nothing
//! about the `catalog.json`, `config.toml`, `images/` or `games/` sitting
//! beside it on the same disk — those are written by whoever made the cartridge,
//! on a machine that holds no signing key, so there is nothing that could have
//! signed them. A launcher must therefore treat the content next to it as
//! untrusted input no matter how it was started. See `SIGNING.md`, §1.

/// One public key a build accepts, and a name for it so the log can say which.
///
/// Borrowed rather than owned because the shipped programs get theirs from a
/// `const` their build script generated — see `listener/build.rs`. A trust
/// anchor in a writable file beside the exe would let anything that could edit
/// that file grant itself auto-run, so there is deliberately no way to load one
/// at runtime.
pub struct Anchor<'a> {
    pub name: &'a str,
    pub base64: &'a str,
}

impl Anchor<'_> {
    /// Whether this anchor is a key [`attest`] could actually use.
    ///
    /// Exists so that the programs carrying baked-in anchors can assert their
    /// build script produced something usable without linking a verifier
    /// themselves — a listener whose generated anchors do not parse would
    /// refuse every cartridge in existence, one puzzled log line at a time.
    pub fn is_usable(&self) -> bool {
        minisign_verify::PublicKey::from_base64(self.base64).is_ok()
    }
}

/// The role names `xtask` writes into the trusted comment, and the ones
/// [`attest`] matches against.
///
/// Here rather than in `xtask` alone because a signer and a checker that
/// disagree about this string produce a cartridge that is silently ignored by
/// every listener on earth. One definition, two users, no drift.
pub const LAUNCHER_ROLE: &str = "romzeta-launcher";
pub const LISTENER_ROLE: &str = "romzeta-listener";
pub const INSTALLER_ROLE: &str = "romzeta-installer";

/// What a verified signature says about the binary it came from.
///
/// Every field here is covered by a signature that has already been checked, so
/// unlike anything else about a file off a stranger's disk, it is safe to
/// believe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attested {
    /// Which anchor accepted it — `release` or `dev`, for the log line.
    pub anchor: String,
    /// The role from the trusted comment. Always equal to the `expected_role`
    /// passed to [`attest`]; carried anyway so a caller logging the result does
    /// not have to remember what it asked for.
    pub role: String,
    /// The `x.y.z` from the trusted comment, unparsed — each program has its own
    /// `Version` type and its own strictness about what it will accept, and this
    /// crate has no business picking one.
    pub version: String,
}

/// Why a binary is not one we will run.
///
/// Distinct variants because the log is the only diagnostic these programs
/// have, and "ordinary USB stick" and "someone renamed our installer" must not
/// read alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No signature block at all — a self-built or stripped binary, or any
    /// unrelated file that happens to have the right name.
    Unsigned,
    /// A block is there but is not a minisign signature.
    Malformed(String),
    /// Correctly signed, by a key we do not accept.
    Untrusted,
    /// Signed by a key we *do* accept, for a different job. The interesting
    /// one: it means someone took a genuine Romzeta binary and put it where a
    /// launcher goes.
    WrongRole { expected: String, found: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Unsigned => write!(f, "it carries no signature"),
            Refusal::Malformed(e) => write!(f, "its signature is malformed: {e}"),
            Refusal::Untrusted => write!(f, "it is signed, but not by a key this build trusts"),
            Refusal::WrongRole { expected, found } => {
                write!(f, "it is a signed {found}, and a {expected} was expected")
            }
        }
    }
}

/// Verifies `bytes` against `anchors` and requires it to declare `expected_role`.
///
/// `bytes` must be the whole file. The signed region is everything before the
/// signature block, which [`sigblock::split`] finds; passing a truncated read
/// would fail rather than pass something unverified, but it would fail for the
/// wrong reason.
pub fn attest(
    bytes: &[u8],
    anchors: &[Anchor<'_>],
    expected_role: &str,
) -> Result<Attested, Refusal> {
    let (payload, signature) = sigblock::split(bytes);
    let Some(signature) = signature else {
        return Err(Refusal::Unsigned);
    };
    let signature = minisign_verify::Signature::decode(signature)
        .map_err(|e| Refusal::Malformed(e.to_string()))?;

    for anchor in anchors {
        let Ok(key) = minisign_verify::PublicKey::from_base64(anchor.base64) else {
            // A build script put it there, so this is a broken build rather than
            // anything about the file in front of us. Skip it and let the others
            // speak.
            continue;
        };
        // `false` refuses minisign's pre-1.0 signature format. Everything we
        // have ever produced is the current one, and accepting the legacy shape
        // would widen what verifies for no living cartridge's benefit.
        if key.verify(payload, &signature, false).is_err() {
            continue;
        }

        // Only past the verify, never before it: until that call returns Ok the
        // trusted comment is just bytes off a disk someone else wrote.
        let (role, version) = split_comment(signature.trusted_comment());
        if role != expected_role {
            return Err(Refusal::WrongRole {
                expected: expected_role.to_string(),
                found: role.to_string(),
            });
        }
        return Ok(Attested {
            anchor: anchor.name.to_string(),
            role: role.to_string(),
            version: version.to_string(),
        });
    }
    Err(Refusal::Untrusted)
}

/// Splits a trusted comment into `(role, version)`.
///
/// The comment `xtask` writes is `<role> <version> <date>`; the date is
/// provenance for a human and nothing reads it. Missing fields come back empty
/// rather than as an error — a signature that verified is ours regardless of
/// what we wrote in the comment years ago, and the empty role simply will not
/// match any `expected_role`, which is the correct outcome for a comment we
/// cannot make sense of.
fn split_comment(comment: &str) -> (&str, &str) {
    let mut parts = comment.split_whitespace();
    (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
}

#[cfg(test)]
mod tests;
