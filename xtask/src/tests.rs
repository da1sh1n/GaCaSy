// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every test in this crate, one submodule per source module. Run with
//! `cargo test -p xtask` — this crate is not in the workspace's
//! `default-members`, so a bare `cargo test` skips it.

// ########## XTASK TESTS ##########

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A temp directory that deletes itself.
///
/// Every fixture that needs a directory goes through this rather than a fixed
/// name under `temp_dir()`: cargo runs tests on threads, and two tests (or two
/// overlapping runs) sharing a path means one test's cleanup deletes another's
/// fixture out from under it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("romzeta-{name}-{}-{unique}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

mod keys {
    use super::Scratch;
    use crate::keys::{base64Line, dotenv, keygen, refuseInsideRepo, secretKey};
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;

    #[test]
    fn reads_the_key_out_of_a_pub_file() {
        let text = "untrusted comment: minisign public key A1B2\nRWQf6LRCGA9i53==\n";
        assert_eq!(base64Line(text), Some("RWQf6LRCGA9i53==".to_string()));
    }

    #[test]
    fn survives_a_pub_file_someone_edited() {
        // No comment line at all, and trailing blank lines.
        assert_eq!(
            base64Line("RWQf6LRCGA9i53==\n\n"),
            Some("RWQf6LRCGA9i53==".to_string())
        );
        assert_eq!(base64Line("untrusted comment: only a comment\n"), None);
        assert_eq!(base64Line(""), None);
    }

    fn envFrom(text: &str) -> HashMap<String, String> {
        let dir = Scratch::new("xtask-dotenv");
        fs::write(dir.join(".env"), text).expect("write .env");
        dotenv(dir.path())
    }

    #[test]
    fn parses_the_env_shapes_that_actually_occur() {
        let env = envFrom(
            "# a comment\n\
             \n\
             ROMZETA_SIGNING_KEY=C:\\keys\\romzeta.key\n\
             ROMZETA_SIGNING_PASSWORD = \"hunter2\"\n\
             QUOTED='single'\n",
        );
        assert_eq!(env["ROMZETA_SIGNING_KEY"], "C:\\keys\\romzeta.key");
        assert_eq!(env["ROMZETA_SIGNING_PASSWORD"], "hunter2");
        assert_eq!(env["QUOTED"], "single");
        assert_eq!(env.len(), 3);
    }

    #[test]
    fn a_missing_env_file_is_not_an_error() {
        assert!(dotenv(Path::new("/nowhere/at/all")).is_empty());
    }

    /// A fake repo root whose `.env` points the signing key somewhere outside
    /// it, which is the arrangement `secretKey` and `keygen` both expect.
    fn rootedAt(case: &str, key: &Path, extra: &str) -> Scratch {
        let root = Scratch::new(&format!("xtask-{case}"));
        fs::write(
            root.join(".env"),
            format!("ROMZETA_SIGNING_KEY={}\n{extra}", key.display()),
        )
        .expect("write .env");
        root
    }

    #[test]
    fn an_unencrypted_key_loads_with_no_password_at_all() {
        // The regression: minisign's `into_secret_key(None)` does not mean "no
        // password", it means "decrypt, asking if you must" — and it *rejects*
        // a key with no KDF. Routing the default key through it turned "needs
        // nothing" into a prompt claiming the key was password-protected.
        let dir = Scratch::new("xtask-plainkey");
        let key_path = dir.join("romzeta.key");

        let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("keypair");
        fs::write(&key_path, pair.sk.to_box(None).expect("box").to_string()).expect("write key");

        let root = rootedAt("plainkey-root", &key_path, "");
        let loaded = secretKey(root.path()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(loaded, pair.sk);
    }

    #[test]
    fn keygen_never_writes_the_password_into_the_key_file() {
        // The other half of the same confusion: `SecretKey::to_box` takes the
        // untrusted *comment*, not a password. Passing the password there put
        // it in cleartext on the line directly above the key it protects.
        let dir = Scratch::new("xtask-encryptedkey");
        let key_path = dir.join("romzeta.key");

        let root = rootedAt(
            "encryptedkey-root",
            &key_path,
            "ROMZETA_SIGNING_PASSWORD=correct-horse-battery-staple\n",
        );
        keygen(root.path(), false).unwrap_or_else(|e| panic!("{e}"));

        let written = fs::read_to_string(&key_path).expect("read key");
        assert!(
            !written.contains("correct-horse-battery-staple"),
            "the password was written into the key file:\n{written}"
        );
        // And it still round-trips, using that password from the same `.env`.
        assert!(secretKey(root.path()).is_ok());
    }

    #[test]
    fn a_secret_key_inside_the_repo_is_refused() {
        // The mistake this exists to stop: putting the key next to its public
        // half, where the next `git add -A` picks it up.
        let root = Scratch::new("xtask-repo");
        fs::create_dir_all(root.join("keys")).expect("temp repo");

        let inside = root.join("keys").join("romzeta.key");
        assert!(refuseInsideRepo(root.path(), &inside).is_err());

        let elsewhere = Scratch::new("xtask-elsewhere");
        let outside = elsewhere.join("romzeta.key");
        assert!(refuseInsideRepo(root.path(), &outside).is_ok());
    }
}

mod manifest {
    use crate::manifest::{check, major, read};
    use std::path::Path;

    fn repoRoot() -> &'static Path {
        // xtask/ -> the workspace root.
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask has a parent")
    }

    #[test]
    fn this_workspace_agrees_with_itself() {
        // The whole point, asserted against the real manifests: if someone bumps
        // one crate's major and not the others, this fails in `cargo test`
        // rather than at release time.
        check(repoRoot()).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn every_crate_is_accounted_for() {
        let (_, crates) = read(repoRoot()).expect("read the manifests");
        let names: Vec<_> = crates.iter().map(|c| c.name.as_str()).collect();
        for expected in [
            "launcher",
            "listener",
            "installer",
            "common",
            "sigblock",
            "xtask",
        ] {
            assert!(names.contains(&expected), "{expected} is not a member");
        }
    }

    #[test]
    fn reads_the_leading_number() {
        assert_eq!(major("0.2.0"), Some(0));
        assert_eq!(major("12.0.1"), Some(12));
        assert_eq!(major(" 1.0.0 "), Some(1));
        assert_eq!(major("not-a-version"), None);
        assert_eq!(major(""), None);
    }
}

mod sign {
    use super::Scratch;
    use crate::keys::{Anchor, base64Line};
    use crate::sign::{sign, verify};
    use std::fs;

    /// The full round trip against the real libraries: generate a key, sign a
    /// buffer, verify it, then break it and watch verification fail.
    #[test]
    fn signs_and_verifies_a_binary() {
        let dir = Scratch::new("xtask-sign");

        let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("keypair");
        let anchors = vec![Anchor {
            name: "dev",
            base64: base64Line(&pair.pk.to_box().expect("pk").to_string()).expect("base64 line"),
        }];
        let key = pair.sk;

        let exe = dir.join("fake.exe");
        fs::write(&exe, b"MZ pretend this is a launcher").expect("write");

        sign(&exe, &key, "romzeta-launcher 0.2.0").expect("sign");
        let verified = verify(&exe, &anchors).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(verified.anchor, "dev");
        assert!(verified.comment.starts_with("romzeta-launcher 0.2.0 "));

        // Signing twice must replace, not nest — and must still verify.
        let once = fs::metadata(&exe).expect("stat").len();
        sign(&exe, &key, "romzeta-launcher 0.2.1").expect("re-sign");
        assert!(fs::metadata(&exe).expect("stat").len() <= once + 8);
        assert!(verify(&exe, &anchors).is_ok());

        // One flipped byte in the payload, and it is no longer ours.
        let mut bytes = fs::read(&exe).expect("read");
        bytes[1] ^= 0xff;
        fs::write(&exe, &bytes).expect("write");
        assert!(verify(&exe, &anchors).is_err());
    }

    #[test]
    fn an_unsigned_binary_is_reported_as_such() {
        let dir = Scratch::new("xtask-unsigned");
        let exe = dir.join("bare.exe");
        fs::write(&exe, b"MZ and nothing else").expect("write");

        let error = verify(&exe, &[]).expect_err("unsigned");
        assert!(error.contains("no signature block"), "{error}");
    }

    #[test]
    fn a_signature_from_another_key_is_refused() {
        let dir = Scratch::new("xtask-otherkey");

        let ours = minisign::KeyPair::generate_unencrypted_keypair().expect("keypair");
        let theirs = minisign::KeyPair::generate_unencrypted_keypair().expect("keypair");
        let anchors = vec![Anchor {
            name: "romzeta",
            base64: base64Line(&ours.pk.to_box().expect("pk").to_string()).expect("base64 line"),
        }];

        let exe = dir.join("theirs.exe");
        fs::write(&exe, b"MZ signed by someone else").expect("write");
        sign(&exe, &theirs.sk, "romzeta-launcher 0.2.0").expect("sign");

        let error = verify(&exe, &anchors).expect_err("not our key");
        assert!(error.contains("not by any key this tree trusts"), "{error}");
    }
}
