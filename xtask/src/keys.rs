// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Locates the secret signing key, loads it (decrypting only when it is
//! encrypted), generates new keypairs, and reads the public keys out of
//! `keys/*.pub`. Refuses a secret key path inside the repository.

// ########## SIGNING KEYS ##########

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Overrides the secret key location. Holds a path, not a key.
const KEY_VAR: &str = "ROMZETA_SIGNING_KEY";
/// The password for it. Absent means the key is unencrypted, or that you will
/// be asked at the prompt.
const PASSWORD_VAR: &str = "ROMZETA_SIGNING_PASSWORD";

// ========== Public Keys ==========

/// A public key a build trusts, and where it came from.
pub struct Anchor {
    /// `romzeta` or `dev` — printed by `xtask verify` so you know which of the
    /// two keys signed the thing in front of you.
    pub name: &'static str,
    /// The bare base64 line, ready for `PublicKey::from_base64`.
    pub base64: String,
}

/// The public keys any listener built from this tree accepts, in the order it
/// tries them. Missing files are skipped rather than being an error.
///
/// Mirrors `listener/build.rs` by reading the same two files — a list of its
/// own here would let `xtask verify` bless a cartridge the listener refuses.
pub fn anchors(root: &Path) -> Vec<Anchor> {
    [("romzeta", "romzeta.pub"), ("dev", "dev.pub")]
        .into_iter()
        // `filter_map` with `?` inside: either the file or the key line being
        // absent drops that anchor and leaves the other one standing.
        .filter_map(|(name, file)| {
            let text = fs::read_to_string(root.join("keys").join(file)).ok()?;
            Some(Anchor {
                name,
                base64: base64Line(&text)?,
            })
        })
        .collect()
}

/// Pulls the key out of a minisign `.pub` file, or `None` if there is no key
/// line in it.
///
/// The format is a comment line then the key, but the comment is free text a
/// human may have edited, so this takes the last line that is neither blank nor
/// a comment rather than trusting the line count.
pub fn base64Line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
        .map(str::to_string)
}

// ========== Settings ==========

/// Reads `.env` at the repo root into a map, or an empty map if there is none.
///
/// Twenty lines rather than a dependency: it handles `KEY=VALUE`, skips blanks
/// and `#` comments, and strips one layer of matching quotes. No interpolation,
/// no multi-line values, no `export` — a build tool that silently misreads the
/// path to a signing key is worse than one that only does the simple case.
pub fn dotenv(root: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(root.join(".env")) else {
        return HashMap::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        // `split_once` so a value containing `=` survives intact.
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| {
            let value = value.trim();
            // Try double quotes, then single; `unwrap_or` leaves an unquoted
            // value exactly as it was.
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            (name.trim().to_string(), value.to_string())
        })
        .collect()
}

/// One setting named `name`, from the real environment first and `env` second.
/// That order round, so an explicit `ROMZETA_SIGNING_KEY=… cargo run` beats a
/// `.env` you set up months ago and forgot.
fn setting(name: &str, env: &HashMap<String, String>) -> Option<String> {
    std::env::var(name)
        .ok()
        // Blank counts as unset on both sides, so an empty variable does not
        // shadow the `.env` entry or become an empty path.
        .filter(|value| !value.trim().is_empty())
        .or_else(|| env.get(name).cloned())
        .filter(|value| !value.trim().is_empty())
}

// ========== The Secret Key ==========

/// Where the secret key is: `$ROMZETA_SIGNING_KEY`, then `.env`, then the
/// default below.
pub fn secretKeyPath(root: &Path) -> PathBuf {
    setting(KEY_VAR, &dotenv(root))
        .map(PathBuf::from)
        .unwrap_or_else(defaultSecretKeyPath)
}

/// `~/.romzeta/romzeta.key` — outside the repo, in the user's profile, on every
/// platform. Chosen so that `git add -A` cannot possibly pick it up.
pub fn defaultSecretKeyPath() -> PathBuf {
    // `USERPROFILE` on Windows, `HOME` elsewhere; the temp dir is a last resort
    // for an environment with neither, where signing will fail loudly anyway.
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".romzeta").join("romzeta.key")
}

/// Loads the secret key, decrypting it only if it is actually encrypted.
/// Prompts for a password as a last resort, and explains what to do when there
/// is no key at all.
///
/// minisign has no generic "open this key" call: `into_secret_key` refuses a key
/// with no KDF outright, and the unencrypted one has its own entry point. So the
/// shape of the file on disk decides which call to make, not whether we hold a
/// password.
pub fn secretKey(root: &Path) -> Result<minisign::SecretKey, String> {
    let path = secretKeyPath(root);
    let text = fs::read_to_string(&path).map_err(|e| {
        format!(
            "no signing key at {} ({e}).\n\
             Generate one with `cargo run -p xtask -- keygen`, or point {KEY_VAR} at an \
             existing key (in the environment or in .env at the repo root).",
            path.display()
        )
    })?;
    // A closure, because `SecretKeyBox` is consumed by every conversion and so
    // has to be re-parsed per attempt. Parsing is only a string wrap.
    let boxed = || {
        minisign::SecretKeyBox::from_string(&text)
            .map_err(|e| format!("{} is not a minisign secret key: {e}", path.display()))
    };

    match boxed()?.into_unencrypted_secret_key() {
        Ok(key) => return Ok(key),
        // The only `Verify` error on this path is a checksum mismatch on a key
        // that really is unencrypted: the file is damaged, not locked, and
        // asking for a password would send you after the wrong problem.
        Err(e) if matches!(e.kind(), minisign::ErrorKind::Verify) => {
            return Err(format!(
                "{} is unencrypted but its checksum does not match — the file is damaged ({e}).",
                path.display()
            ));
        }
        // Anything else means it carries a KDF: it is encrypted and needs one.
        Err(_) => {}
    }

    if let Some(password) = setting(PASSWORD_VAR, &dotenv(root)) {
        return boxed()?.into_secret_key(Some(password)).map_err(|e| {
            format!(
                "could not decrypt {} — wrong {PASSWORD_VAR}? ({e})",
                path.display()
            )
        });
    }

    // stderr, so the prompt is not captured when stdout is being piped.
    eprintln!("{} is password-protected.", path.display());
    eprintln!("(set {PASSWORD_VAR} in the environment or .env to skip this prompt)");
    let password = rpassword::prompt_password("password: ")
        .map_err(|e| format!("could not read the password: {e}"))?;
    boxed()?
        .into_secret_key(Some(password))
        .map_err(|e| format!("could not decrypt {}: {e}", path.display()))
}

// ========== Generating ==========

/// Creates a signing key, writing the secret outside the repo and its public
/// half into `keys/`. `release` picks `romzeta.pub` over the default `dev.pub`.
///
/// Refuses to overwrite either an existing secret key or an existing
/// `romzeta.pub`: replacing one orphans every cartridge already signed with it,
/// with no way back.
pub fn keygen(root: &Path, release: bool) -> Result<(), String> {
    let secret_path = secretKeyPath(root);
    refuseInsideRepo(root, &secret_path)?;

    if secret_path.exists() {
        return Err(format!(
            "{} already exists.\n\
             Refusing to overwrite it: every cartridge already signed with that key would \
             stop being accepted by every listener built against it, with no way back. \
             Delete it by hand if that is really what you want.",
            secret_path.display()
        ));
    }

    let password = setting(PASSWORD_VAR, &dotenv(root));
    if password.is_none() {
        eprintln!(
            "No {PASSWORD_VAR} set — generating an unencrypted key.\n\
             That is a reasonable choice for a key whose only job is signing your own \
             local builds; set {PASSWORD_VAR} first if you want one you have to unlock."
        );
    }

    // Two calls rather than one taking an Option: the encrypted path runs
    // scrypt, which is the point when there is a password and pure cost when
    // there is not.
    let pair = match &password {
        Some(password) => minisign::KeyPair::generate_encrypted_keypair(Some(password.clone())),
        None => minisign::KeyPair::generate_unencrypted_keypair(),
    }
    .map_err(|e| format!("could not generate a keypair: {e}"))?;

    if let Some(parent) = secret_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    // `to_box`'s argument is the *untrusted comment* line, not a password.
    // Encryption already happened above, and passing the password here would
    // write it to disk in the clear, right above the key it protects.
    let secret_text = pair
        .sk
        .to_box(None)
        .map_err(|e| format!("could not serialise the secret key: {e}"))?
        .to_string();
    fs::write(&secret_path, secret_text)
        .map_err(|e| format!("could not write {}: {e}", secret_path.display()))?;

    let public_path = root
        .join("keys")
        .join(if release { "romzeta.pub" } else { "dev.pub" });
    if release && public_path.exists() {
        return Err(format!(
            "{} already exists.\n\
             There is only ever one release key: replacing it would orphan every cartridge \
             already signed with the old one. Generate a dev key instead (drop --release), \
             or delete it by hand if you truly mean to start over.",
            public_path.display()
        ));
    }
    fs::create_dir_all(root.join("keys")).map_err(|e| format!("could not create keys/: {e}"))?;
    let public_text = pair
        .pk
        .to_box()
        .map_err(|e| format!("could not serialise the public key: {e}"))?
        .to_string();
    fs::write(&public_path, &public_text)
        .map_err(|e| format!("could not write {}: {e}", public_path.display()))?;

    println!("secret key  {}", secret_path.display());
    println!("            never commit this, never share it, and back it up somewhere");
    println!("            safe — losing it means no future release can be signed.");
    println!("public key  {}", public_path.display());
    println!(
        "            {}",
        if release {
            "commit this; every published listener is built to trust it"
        } else {
            "gitignored; listeners you build here will trust it"
        }
    );
    println!();
    println!("{}", base64Line(&public_text).unwrap_or_default());
    println!();
    println!("Rebuild for it to take effect: cargo run -p xtask -- release");
    Ok(())
}

/// Refuses a secret key path anywhere under the working tree.
///
/// The check is on the *parent* directory, because the key does not exist yet
/// and `canonicalize` needs something real to resolve. A repo root that will
/// not canonicalize fails closed too.
pub(crate) fn refuseInsideRepo(root: &Path, secret: &Path) -> Result<(), String> {
    let parent = secret.parent().unwrap_or(Path::new("."));
    // Created first so `canonicalize` below has a real directory to resolve.
    let _ = fs::create_dir_all(parent);

    // Both paths canonicalized before comparing: `starts_with` is textual, so
    // a `..` or a symlink would otherwise walk straight out of the check.
    let (Ok(root), Ok(parent)) = (root.canonicalize(), parent.canonicalize()) else {
        return Err(format!(
            "could not resolve {} against the repo root to check it is outside the working tree",
            secret.display()
        ));
    };

    if parent.starts_with(&root) {
        return Err(format!(
            "{} is inside the repository.\n\
             The signing key must live outside the working tree — this repo is public, and a \
             key in it is a key that gets pushed. Unset {KEY_VAR} to use the default \
             ({}), or point it somewhere outside {}.",
            secret.display(),
            defaultSecretKeyPath().display(),
            root.display()
        ));
    }
    Ok(())
}
