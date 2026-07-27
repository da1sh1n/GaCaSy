// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Where the signing key lives, and which public keys a build trusts.
//!
//! # The one rule
//!
//! **The secret key never enters the repository.** Everything else here is in
//! service of that. The repo is public; anyone can clone it, and the only thing
//! stopping them producing a launcher the shipped listener will run is that they
//! do not have this file. So the default location is outside the working tree
//! entirely, [`keygen`] refuses to write it anywhere inside, and `.gitignore`
//! carries a belt-and-braces rule for the extension.
//!
//! # The two public keys
//!
//! | File | What it is | Committed |
//! |---|---|---|
//! | `keys/gacasy.pub` | the release key | yes — it is public |
//! | `keys/dev.pub` | whatever you generated locally | no |
//!
//! A listener trusts both, which is what makes a clone of this repo useful: you
//! sign with your own key and your own listener accepts your own cartridges,
//! while an official listener still refuses them and yours still accepts an
//! official cartridge. Nothing about that is configurable at runtime — see
//! `listener/build.rs`.
//!
//! `keygen` writes `dev.pub` and never touches `gacasy.pub`, so cloning, keying
//! and building leaves no modification to a tracked file to accidentally commit.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Overrides the secret key location. A path, not a key.
const KEY_VAR: &str = "GACASY_SIGNING_KEY";
/// The password for it. Absent means the key is unencrypted, or that you will
/// be asked.
const PASSWORD_VAR: &str = "GACASY_SIGNING_PASSWORD";

/// A public key a build trusts, and where it came from.
pub struct Anchor {
    /// `gacasy` or `dev` — what `xtask verify` prints so you know which of the
    /// two keys signed the thing in front of you.
    pub name: &'static str,
    /// The bare base64 line, ready for `minisign_verify::PublicKey::from_base64`.
    pub base64: String,
}

/// The public keys any listener built from this tree will accept, in the order
/// it tries them.
///
/// Mirrors `listener/build.rs`. If the two ever disagree, `xtask verify` would
/// bless a cartridge the listener refuses, which is the most confusing possible
/// failure — so both read these same two files and neither has a list of its
/// own.
pub fn anchors(root: &Path) -> Vec<Anchor> {
    [("gacasy", "gacasy.pub"), ("dev", "dev.pub")]
        .into_iter()
        .filter_map(|(name, file)| {
            let text = fs::read_to_string(root.join("keys").join(file)).ok()?;
            Some(Anchor {
                name,
                base64: base64_line(&text)?,
            })
        })
        .collect()
}

/// Pulls the key out of a minisign `.pub` file.
///
/// The format is a comment line then the key, but the comment is free text a
/// human may well have edited, so this takes the last line that is neither
/// blank nor a comment rather than trusting the line count.
pub fn base64_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
        .next_back()
        .map(str::to_string)
}

/// Reads `.env` at the repo root, if there is one.
///
/// Deliberately about twenty lines rather than a dependency: this parses
/// `KEY=VALUE`, ignores blanks and `#` comments, and strips one layer of
/// matching quotes. It does not do interpolation, multi-line values or `export`
/// prefixes, because a build tool that silently misreads the path to a signing
/// key is worse than one that only understands the simple case.
pub fn dotenv(root: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(root.join(".env")) else {
        return HashMap::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            (name.trim().to_string(), value.to_string())
        })
        .collect()
}

/// A setting from the real environment, falling back to `.env`.
///
/// That order round: an explicit `GACASY_SIGNING_KEY=… cargo run` should win
/// over a file you set up months ago and forgot.
fn setting(name: &str, env: &HashMap<String, String>) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| env.get(name).cloned())
        .filter(|value| !value.trim().is_empty())
}

/// Where the secret key is: `$GACASY_SIGNING_KEY`, then `.env`, then
/// `~/.gacasy/gacasy.key`.
pub fn secret_key_path(root: &Path) -> PathBuf {
    setting(KEY_VAR, &dotenv(root))
        .map(PathBuf::from)
        .unwrap_or_else(default_secret_key_path)
}

/// Outside the repo, in the user's profile, on every platform. Chosen so that
/// the obvious thing — `git add -A` — cannot possibly pick it up.
pub fn default_secret_key_path() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".gacasy").join("gacasy.key")
}

/// Loads and decrypts the secret key.
///
/// The password comes from the environment or `.env` when set. When it is not,
/// an unencrypted key is tried first — that is what `keygen` produces when you
/// give it no password, and prompting for a password that does not exist would
/// be a small daily lie. Only if that fails are you asked.
pub fn secret_key(root: &Path) -> Result<minisign::SecretKey, String> {
    let path = secret_key_path(root);
    let text = fs::read_to_string(&path).map_err(|e| {
        format!(
            "no signing key at {} ({e}).\n\
             Generate one with `cargo run -p xtask -- keygen`, or point {KEY_VAR} at an \
             existing key (in the environment or in .env at the repo root).",
            path.display()
        )
    })?;
    let boxed = minisign::SecretKeyBox::from_string(&text)
        .map_err(|e| format!("{} is not a minisign secret key: {e}", path.display()))?;

    if let Some(password) = setting(PASSWORD_VAR, &dotenv(root)) {
        return boxed
            .into_secret_key(Some(password))
            .map_err(|e| format!("could not decrypt {} — wrong {PASSWORD_VAR}? ({e})", path.display()));
    }

    let boxed_again = minisign::SecretKeyBox::from_string(&text).expect("parsed once already");
    match boxed.into_secret_key(None) {
        Ok(key) => Ok(key),
        Err(_) => {
            eprintln!("{} is password-protected.", path.display());
            eprintln!("(set {PASSWORD_VAR} in the environment or .env to skip this prompt)");
            let password = rpassword::prompt_password("password: ")
                .map_err(|e| format!("could not read the password: {e}"))?;
            boxed_again
                .into_secret_key(Some(password))
                .map_err(|e| format!("could not decrypt {}: {e}", path.display()))
        }
    }
}

/// Creates a signing key, writing the secret outside the repo and its public
/// half inside it.
///
/// `release` picks which public key file it becomes. The default, `dev.pub`, is
/// gitignored and is what anyone cloning this repo wants: their listener trusts
/// their cartridges and nobody else's build changes. `--release` writes
/// `gacasy.pub`, the committed key every published listener is built against —
/// there should be exactly one of those, ever, so it refuses to overwrite.
pub fn keygen(root: &Path, release: bool) -> Result<(), String> {
    let secret_path = secret_key_path(root);
    refuse_inside_repo(root, &secret_path)?;

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

    // Two different calls, not one with an Option: the encrypted path runs
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
    // `to_box` is what encrypts. Passing None here would write the key to disk
    // in the clear no matter what password was asked for — the generated
    // KeyPair's `sk` is already decrypted in memory.
    let secret_text = pair
        .sk
        .to_box(password.as_deref())
        .map_err(|e| format!("could not serialise the secret key: {e}"))?
        .to_string();
    fs::write(&secret_path, secret_text)
        .map_err(|e| format!("could not write {}: {e}", secret_path.display()))?;

    let public_path = root
        .join("keys")
        .join(if release { "gacasy.pub" } else { "dev.pub" });
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
    println!("{}", base64_line(&public_text).unwrap_or_default());
    println!();
    println!("Rebuild for it to take effect: cargo run -p xtask -- release");
    Ok(())
}

/// Refuses a secret key path anywhere under the working tree.
///
/// The check is on the *parent* directory, because the key itself does not
/// exist yet and `canonicalize` needs something real to resolve. A repo root
/// that will not canonicalize is not a repo we can reason about, so that fails
/// closed too.
fn refuse_inside_repo(root: &Path, secret: &Path) -> Result<(), String> {
    let parent = secret.parent().unwrap_or(Path::new("."));
    let _ = fs::create_dir_all(parent);

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
            default_secret_key_path().display(),
            root.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_key_out_of_a_pub_file() {
        let text = "untrusted comment: minisign public key A1B2\nRWQf6LRCGA9i53==\n";
        assert_eq!(base64_line(text), Some("RWQf6LRCGA9i53==".to_string()));
    }

    #[test]
    fn survives_a_pub_file_someone_edited() {
        // No comment line at all, and trailing blank lines.
        assert_eq!(
            base64_line("RWQf6LRCGA9i53==\n\n"),
            Some("RWQf6LRCGA9i53==".to_string())
        );
        assert_eq!(base64_line("untrusted comment: only a comment\n"), None);
        assert_eq!(base64_line(""), None);
    }

    fn env_from(text: &str) -> HashMap<String, String> {
        let dir = std::env::temp_dir().join("gacasy-xtask-dotenv");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join(".env"), text).expect("write .env");
        dotenv(&dir)
    }

    #[test]
    fn parses_the_env_shapes_that_actually_occur() {
        let env = env_from(
            "# a comment\n\
             \n\
             GACASY_SIGNING_KEY=C:\\keys\\gacasy.key\n\
             GACASY_SIGNING_PASSWORD = \"hunter2\"\n\
             QUOTED='single'\n",
        );
        assert_eq!(env["GACASY_SIGNING_KEY"], "C:\\keys\\gacasy.key");
        assert_eq!(env["GACASY_SIGNING_PASSWORD"], "hunter2");
        assert_eq!(env["QUOTED"], "single");
        assert_eq!(env.len(), 3);
    }

    #[test]
    fn a_missing_env_file_is_not_an_error() {
        assert!(dotenv(Path::new("/nowhere/at/all")).is_empty());
    }

    #[test]
    fn a_secret_key_inside_the_repo_is_refused() {
        // The mistake this exists to stop: putting the key next to its public
        // half, where the next `git add -A` picks it up.
        let root = std::env::temp_dir().join("gacasy-xtask-repo");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("keys")).expect("temp repo");

        let inside = root.join("keys").join("gacasy.key");
        assert!(refuse_inside_repo(&root, &inside).is_err());

        let outside = std::env::temp_dir().join("gacasy-xtask-elsewhere").join("gacasy.key");
        assert!(refuse_inside_repo(&root, &outside).is_ok());
    }
}
