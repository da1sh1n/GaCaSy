// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Verifies a binary against a set of public keys and checks the role its
//! signed comment declares, returning what the signature says or why it was
//! refused. Covers that file only; anything beside it on disk is unsigned.

// Functions are camelCase in this project while variables stay snake_case,
// which rustc's default lints object to. Silenced once, at the crate root.
#![allow(non_snake_case)]

// ########## THE TRUST DECISION ##########

// ========== What A Build Accepts ==========

/// One public key a build will accept, plus a name for it so the log can say
/// which one let a binary through.
///
/// The fields are borrowed: the shipped programs get theirs from a `const`
/// their build script generated (see `listener/build.rs`). There is no runtime
/// path for loading an anchor.
pub struct Anchor<'a> {
    pub name: &'a str,
    pub base64: &'a str,
}

impl Anchor<'_> {
    /// Whether this anchor parses as a key `attest` could actually use.
    /// Lets a program assert its build script produced something usable without
    /// linking a verifier itself — anchors that do not parse would otherwise
    /// refuse every cartridge in existence, one puzzled log line at a time.
    pub fn isUsable(&self) -> bool {
        minisign_verify::PublicKey::from_base64(self.base64).is_ok()
    }
}

/// The role names `xtask` writes into the trusted comment and `attest` matches
/// against. Defined here, where both the signer and the checker can see them: a
/// disagreement about this string makes a cartridge every listener ignores.
pub const LAUNCHER_ROLE: &str = "romzeta-launcher";
pub const LISTENER_ROLE: &str = "romzeta-listener";
pub const INSTALLER_ROLE: &str = "romzeta-installer";

// ========== The Two Outcomes ==========

/// What a verified signature says about the binary it came from. Every field is
/// covered by a signature that has already been checked, so unlike anything else
/// about a file off a stranger's disk it is safe to believe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attested {
    /// Which anchor accepted it — `release` or `dev`, for the log line.
    pub anchor: String,
    /// The role from the trusted comment, always equal to the `expected_role`
    /// that was asked for. Carried anyway so a caller logging the result does
    /// not have to remember what it passed in.
    pub role: String,
    /// The `x.y.z` from the trusted comment, left unparsed — each program has
    /// its own strictness about what it accepts, and this crate does not pick.
    pub version: String,
}

/// Why a binary is not one we will run. Separate variants because the log is
/// the only diagnostic these programs have, and "ordinary USB stick" must not
/// read like "someone renamed our installer".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No signature block at all — a self-built or stripped binary, or an
    /// unrelated file that happens to have the right name.
    Unsigned,
    /// A block is there, but it is not a minisign signature.
    Malformed(String),
    /// Correctly signed, by a key this build does not accept.
    Untrusted,
    /// Signed by a key we do accept, for a different job — someone put a
    /// genuine Romzeta binary where a launcher goes.
    WrongRole { expected: String, found: String },
}

impl std::fmt::Display for Refusal {
    // `fmt` is fixed by the Display trait, so it keeps rustc's spelling.
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

// ========== Attesting ==========

/// Verifies the whole file `bytes` against `anchors`, and requires the result
/// to declare `expected_role`. Returns what the signature says on success, or
/// the reason it was refused.
///
/// The three questions are answered in this order and the order is the point:
/// is it signed by a key we trust, what does it say it is, and is that what we
/// asked for. Nothing in the file means anything until the first one passes.
pub fn attest(
    bytes: &[u8],
    anchors: &[Anchor<'_>],
    expected_role: &str,
) -> Result<Attested, Refusal> {
    // The signed region is everything before the block sigblock finds.
    let (payload, signature) = sigblock::split(bytes);
    let Some(signature) = signature else {
        return Err(Refusal::Unsigned);
    };
    // `?` propagates as a Refusal because of the `map_err` right before it.
    let signature = minisign_verify::Signature::decode(signature)
        .map_err(|e| Refusal::Malformed(e.to_string()))?;

    for anchor in anchors {
        let Ok(key) = minisign_verify::PublicKey::from_base64(anchor.base64) else {
            // A build script put this here, so a bad anchor is a broken build
            // rather than anything about the file. Let the other anchors speak.
            continue;
        };
        // `false` refuses minisign's pre-1.0 signature format. Everything we
        // have ever produced is the current one, and accepting the legacy shape
        // would widen what verifies for no living cartridge's benefit.
        if key.verify(payload, &signature, false).is_err() {
            continue;
        }

        // Only past the verify, never before: until that call returns Ok, the
        // trusted comment is just bytes off a disk someone else wrote. minisign
        // covers it with a second signature over `signature ‖ comment`, which
        // the same `verify` call above already checked.
        let (role, version) = splitComment(signature.trusted_comment());
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
    // Fell off the end: a valid signature, but from nobody we know.
    Err(Refusal::Untrusted)
}

/// Splits a trusted comment into `(role, version)`. `xtask` writes
/// `<role> <version> <date>`; the date is provenance for a human and nothing
/// reads it.
///
/// Missing fields come back empty rather than as an error — an empty role
/// matches no `expected_role`, which is already the right outcome for a comment
/// we cannot make sense of.
fn splitComment(comment: &str) -> (&str, &str) {
    let mut parts = comment.split_whitespace();
    // `unwrap_or("")` rather than `?`: see the doc comment above.
    (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
}

#[cfg(test)]
mod tests;
