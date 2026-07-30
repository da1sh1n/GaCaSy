// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Putting a signature into a binary, and reading one back out.
//!
//! Both directions go through [`sigblock`], so the bytes this crate signs are
//! exactly the bytes the listener will verify. The asymmetry worth knowing: we
//! sign with `minisign` (the reference implementation) and verify with
//! `minisign-verify` (what the listener links). Verifying with the same library
//! that just signed would prove only that it agrees with itself.

use std::fs;
use std::path::Path;

use crate::keys::Anchor;

/// Signs `path` in place.
///
/// In place, and idempotent: [`sigblock::attach`] strips any block already
/// there, so signing an exe twice replaces its signature rather than burying
/// the old one inside the new signed payload. `cargo build` and this function
/// both write to `target/release/`, in whatever order you happen to run them.
pub fn sign(path: &Path, key: &minisign::SecretKey, comment: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let (payload, _) = sigblock::split(&bytes);

    let trusted = format!("{comment} {}", today());
    let signature = minisign::sign(None, key, payload, Some(&trusted), Some(comment))
        .map_err(|e| format!("could not sign {}: {e}", path.display()))?
        .into_string();

    fs::write(path, sigblock::attach(payload, &signature))
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// What a signed binary turned out to be.
#[derive(Debug)]
pub struct Verified {
    /// Which trust anchor accepted it — `gacasy` for a release build, `dev` for
    /// one of yours.
    pub anchor: String,
    /// The signer's trusted comment, which minisign authenticates along with the
    /// file. Free text; nothing depends on its shape.
    pub comment: String,
}

/// Verifies `path` against `anchors`, the way the listener will.
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

    if anchors.is_empty() {
        return Err("no public keys to check against — keys/gacasy.pub is missing".to_string());
    }

    for anchor in anchors {
        let Ok(key) = minisign_verify::PublicKey::from_base64(&anchor.base64) else {
            return Err(format!("keys/{}.pub is not a minisign public key", anchor.name));
        };
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

/// `YYYY-MM-DD`, for the trusted comment.
///
/// Hand-rolled from the system clock rather than pulling in a date library for
/// one line of provenance text that nothing parses. Civil-from-days, after
/// Howard Hinnant's algorithm; correct for any date this project will see.
fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}")
}
