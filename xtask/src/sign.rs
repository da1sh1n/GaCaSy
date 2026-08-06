// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Signs a binary in place and verifies one, both through `sigblock`. Signs
//! with `minisign`, verifies with `minisign-verify`.

// ########## SIGNING AND VERIFYING ##########

use std::fs;
use std::path::Path;

use crate::keys::Anchor;

// ========== Signing ==========

/// Signs the file at `path` in place with `key`, writing `comment` and today's
/// date into the trusted comment.
///
/// Idempotent: `sigblock::attach` strips any block already there, so signing an
/// exe twice replaces its signature instead of burying the old one inside the
/// new signed payload.
pub fn sign(path: &Path, key: &minisign::SecretKey, comment: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    // Sign the payload, not the file: an already-signed exe must not have its
    // old block folded into what the new signature covers.
    let (payload, _) = sigblock::split(&bytes);

    let trusted = format!("{comment} {}", common::time::today());
    // The trusted comment is signed alongside the payload; the untrusted one
    // (the last argument) is free text nothing checks.
    let signature = minisign::sign(None, key, payload, Some(&trusted), Some(comment))
        .map_err(|e| format!("could not sign {}: {e}", path.display()))?
        .into_string();

    fs::write(path, sigblock::attach(payload, &signature))
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

// ========== Verifying ==========

/// What a signed binary turned out to be.
#[derive(Debug)]
pub struct Verified {
    /// Which trust anchor accepted it — `romzeta` for a release build, `dev`
    /// for one of yours.
    pub anchor: String,
    /// The signer's trusted comment, which minisign authenticates along with
    /// the file. Free text; nothing depends on its shape.
    pub comment: String,
}

/// Verifies the file at `path` against `anchors`, the way the listener will.
/// Returns which anchor accepted it, or a sentence explaining the refusal.
pub fn verify(path: &Path, anchors: &[Anchor]) -> Result<Verified, String> {
    let bytes = fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let (payload, signature) = sigblock::split(&bytes);

    let Some(signature) = signature else {
        return Err(format!(
            "{} carries no signature block — was it rebuilt after being signed?",
            path.display()
        ));
    };
    let signature = minisign_verify::Signature::decode(signature)
        .map_err(|e| format!("{} has a malformed signature: {e}", path.display()))?;

    // Distinct from "signed by a stranger": no keys at all is a broken checkout,
    // and saying so beats reporting every binary as untrusted.
    if anchors.is_empty() {
        return Err("no public keys to check against — keys/romzeta.pub is missing".to_string());
    }

    for anchor in anchors {
        let Ok(key) = minisign_verify::PublicKey::from_base64(&anchor.base64) else {
            // A key file we control, so a bad one is an error worth stopping on
            // rather than something to skip past like `trust::attest` does.
            return Err(format!(
                "keys/{}.pub is not a minisign public key",
                anchor.name
            ));
        };
        // `false` refuses minisign's pre-1.0 signature format, matching the
        // listener exactly — this check has to reach the same conclusion it will.
        if key.verify(payload, &signature, false).is_ok() {
            return Ok(Verified {
                anchor: anchor.name.to_string(),
                comment: signature.trusted_comment().to_string(),
            });
        }
    }

    Err(format!(
        "{} is signed, but not by any key this tree trusts ({}).\n\
         A listener built from this tree would refuse it.",
        path.display(),
        anchors
            .iter()
            .map(|a| a.name)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}
