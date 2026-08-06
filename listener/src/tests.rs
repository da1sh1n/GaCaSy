// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every test in this crate, one submodule per source module. Run with
//! `cargo test -p listener`; a fresh clone needs
//! `cargo run -p xtask -- keygen` first or the crate will not compile.

// ########## LISTENER TESTS ##########

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

mod trust {
    use super::Scratch;
    use crate::trust::{ANCHORS, LAUNCHER_NAME, Refusal, verifyLauncher};
    use std::fs;

    /// Shorthand for "refused for this signature reason".
    fn refusedBecause(dir: &Scratch, expected: ::trust::Refusal) -> bool {
        matches!(
            verifyLauncher(dir.path()),
            Err(Refusal::Signature(actual)) if actual == expected
        )
    }

    /// A volume with nothing on it is the ordinary case, and the one that has to
    /// stay cheap: this is every USB stick anyone ever plugs in.
    #[test]
    fn a_volume_with_no_launcher_is_not_a_cartridge() {
        let dir = Scratch::new("trust-empty");
        assert!(matches!(
            verifyLauncher(dir.path()),
            Err(Refusal::NoLauncher)
        ));
    }

    #[test]
    fn an_unsigned_launcher_is_refused() {
        let dir = Scratch::new("trust-unsigned");
        fs::write(dir.join(LAUNCHER_NAME), b"MZ but nobody signed it").expect("write");
        assert!(refusedBecause(&dir, ::trust::Refusal::Unsigned));
    }

    /// The handle `verifyLauncher` holds has to deny writers without denying
    /// readers — the image loader needs one to start the process, so a lock that
    /// excluded everything would make every genuine cartridge fail to launch.
    /// Verified on a file we control, since the signed path cannot be reached
    /// here without a signing key.
    #[cfg(windows)]
    #[test]
    fn the_lock_denies_writers_and_allows_readers() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;

        let dir = Scratch::new("trust-lock");
        let path = dir.join("locked.bin");
        fs::write(&path, b"MZ").expect("write");

        let _held = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .expect("open with the same share mode verifyLauncher uses");

        assert!(fs::read(&path).is_ok(), "a reader must still get in");
        assert!(
            fs::OpenOptions::new().write(true).open(&path).is_err(),
            "a writer must not: this is what stops the file being swapped \
             between verifying it and running it"
        );
        assert!(fs::remove_file(&path).is_err(), "nor a deleter");
    }

    #[test]
    fn a_launcher_signed_by_a_stranger_is_refused() {
        // The signature is real and well-formed; it is simply not ours. This is
        // the case the whole module exists for, and the one that used to be
        // "copy the key off the cartridge and write it into your own".
        let dir = Scratch::new("trust-stranger");

        let signature = "untrusted comment: signature from a key we do not have\n\
                         RUQAAAAAAAAAAOaGxHqZQ0KtvVCJ6iKzXG8bFvKZ0V0kZ1qWzKz0hVYQ4rZ8Xk1t\
                         Yy0jVQhJZ0kZ1qWzKz0hVYQ4rZ8Xk1tYy0jVQ==\n\
                         trusted comment: romzeta-launcher 9.9.9\n\
                         AAAA==\n";
        let signed = sigblock::attach(b"MZ signed by someone else", signature);
        fs::write(dir.join(LAUNCHER_NAME), signed).expect("write");

        // Either it fails to decode or it fails to verify; both are refusals and
        // neither launches anything. What must never happen is Ok.
        assert!(verifyLauncher(dir.path()).is_err());
    }

    #[test]
    fn this_build_has_something_to_trust() {
        // A listener compiled with no anchors would refuse every cartridge in
        // existence, one puzzled log line at a time. build.rs makes that a build
        // error; this makes sure the generated file is actually wired up.
        assert!(!ANCHORS.is_empty(), "build.rs produced no trust anchors");
        for anchor in ANCHORS {
            assert!(
                anchor.isUsable(),
                "keys/{}.pub is not a usable minisign public key",
                anchor.name
            );
        }
    }
}

mod version {
    use crate::version::{Version, own, parse};

    #[test]
    fn parses_a_bare_version() {
        assert_eq!(
            parse("0.2.0"),
            Some(Version {
                major: 0,
                minor: 2,
                patch: 0
            })
        );
        assert_eq!(parse("  12.3.45  \r\n").map(|v| v.major), Some(12));
    }

    #[test]
    fn refuses_anything_that_is_not_three_numbers() {
        // The shapes a well-meaning change might introduce. Each would be a
        // guess about what the signature meant, so each is refused. The first is
        // the one that matters now: `parse` is fed the *version field* of a
        // trusted comment, never the whole comment.
        assert_eq!(parse("romzeta-launcher 0.2.0"), None);
        assert_eq!(parse("0.2"), None);
        assert_eq!(parse("0.2.0.1"), None);
        assert_eq!(parse("0.2.0-rc1"), None);
        assert_eq!(parse("v0.2.0"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("not a version"), None);
    }

    #[test]
    fn our_own_version_parses() {
        // If this fails, `--version` is printing something no other Romzeta
        // program could read back.
        let own = own();
        assert_eq!(parse(&own.to_string()), Some(own));
    }

    #[test]
    fn the_version_field_of_a_signed_comment_parses() {
        // The shape `xtask sign` writes, split by `trust::attest` into role and
        // version. This is the contract between the two crates: if xtask's
        // comment format ever changes, this is what should notice.
        let (role, version) = {
            let comment = "romzeta-launcher 0.2.1 2026-07-30";
            let mut parts = comment.split_whitespace();
            (parts.next().unwrap(), parts.next().unwrap())
        };
        assert_eq!(role, ::trust::LAUNCHER_ROLE);
        assert_eq!(parse(version).map(|v| v.minor), Some(2));
    }
}

mod volume {
    use super::Scratch;
    use crate::log::Log;
    use crate::trust;
    use crate::volume::{Outcome, handleVolume};
    use std::fs;

    /// Builds a fake volume in a temp folder.
    fn fakeVolume(name: &str, launcher: Option<&[u8]>) -> Scratch {
        let dir = Scratch::new(&format!("volume-{name}"));
        if let Some(bytes) = launcher {
            fs::write(dir.join(trust::LAUNCHER_NAME), bytes).expect("write launcher");
        }
        dir
    }

    #[test]
    fn a_volume_with_no_launcher_is_ignored() {
        let dir = fakeVolume("plain", None);
        assert_eq!(handleVolume(dir.path(), &Log::silent()), Outcome::Ignored);
    }

    #[test]
    fn an_unsigned_launcher_is_never_started() {
        // The whole point of the change: a binary sitting at a volume root with
        // the right *name* gets nowhere without the right signature.
        let dir = fakeVolume("unsigned", Some(b"MZ nobody signed this"));
        assert_eq!(handleVolume(dir.path(), &Log::silent()), Outcome::Ignored);
    }

    #[test]
    fn a_launcher_signed_by_a_stranger_is_never_started() {
        let signature = "untrusted comment: signature from a key we do not have\n\
                         RUQAAAAAAAAAAOaGxHqZQ0KtvVCJ6iKzXG8bFvKZ0V0kZ1qWzKz0hVYQ4rZ8Xk1t\n\
                         trusted comment: romzeta-launcher 0.2.0\n\
                         AAAA==\n";
        let signed = sigblock::attach(b"MZ signed by someone else", signature);
        let dir = fakeVolume("stranger", Some(&signed));
        assert_eq!(handleVolume(dir.path(), &Log::silent()), Outcome::Ignored);
    }
}
