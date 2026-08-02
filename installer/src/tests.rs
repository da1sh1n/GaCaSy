// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every test in this crate, one submodule per source module.
//!
//! Deliberately narrow. The installer is a wizard, and anything you can confirm
//! by clicking through it — the screens, the scan, the copy, the registry
//! writes, the folder layout — is not tested here. What survives is the four
//! things a person cannot check by looking:
//!
//! - [`volume`] — that the drive Windows is on is never offered. Edit mode
//!   *deletes folders* from whichever volume you pick, so this is the one
//!   mistake that costs more than a retry.
//! - [`catalog`] — that a catalog entry cannot name a path outside the
//!   cartridge, and that a game's name cannot become one either.
//! - [`image`] — header parsing of whatever file a user picked as a cover. This
//!   is arbitrary bytes going through a hand-rolled parser.
//! - [`version`] — that `--version` still prints the shape the listener parses.
//! - [`font`] — that the wizard is drawing in the *system's* font. Both it and
//!   the fallback are legible sans-serifs, so a lookup that quietly failed looks
//!   exactly like one that worked.
//! - [`payload`] — that the two programs survive being packed and unpacked with
//!   their signatures intact. A launcher that lost a byte writes a cartridge that
//!   looks right and works nowhere.
//!
//! Run with `cargo test -p installer`. The installer is not in the workspace's
//! `default-members`, so a bare `cargo test` skips it entirely — use
//! `cargo test --workspace`.
//!
//! # One thing to know before running these
//!
//! [`volume`]'s two tests enumerate the machine's real drives and assert against
//! `%SystemRoot%`. They are deliberately not `#[ignore]`d: behind `--ignored`
//! they would simply never run again, and the behaviour they guard is the most
//! destructive thing this program could get wrong. Nothing here writes to the
//! registry or the filesystem.

mod payload {
    use crate::payload::{LAUNCHER_BYTES, LISTENER_BYTES, launcher, listener};

    /// The launcher is carried compressed and written uncompressed, and the
    /// minisign signature riding inside it *is* the cartridge's identity. A
    /// single byte off and the cartridge still looks perfect, still contains a
    /// launcher of the right name and size, and is silently ignored by every
    /// listener — with no symptom but nothing happening.
    ///
    /// So this checks the thing that cannot be checked by looking at the drive:
    /// that what comes out of the payload is still signed.
    #[test]
    fn what_unpacks_is_still_signed() {
        for (name, unpacked, expected) in [
            ("launcher.exe", launcher(), LAUNCHER_BYTES),
            ("listener.exe", listener(), LISTENER_BYTES),
        ] {
            let bytes = unpacked.unwrap_or_else(|e| panic!("{name} did not unpack: {e}"));
            assert_eq!(bytes.len() as u64, expected, "{name} unpacked to the wrong size");
            assert!(
                sigblock::is_signed(&bytes),
                "{name} came out of the payload without its signature — every cartridge \
                 this installer writes would be ignored by every listener"
            );
        }
    }
}

mod font {
    use crate::font::{FALLBACK, SYSTEM, definitions};
    use egui::FontFamily;

    /// epaint panics — `FontFamily::… is not bound to any fonts` — the first time
    /// a family with nothing behind it is used, and it does that lazily. Nothing
    /// in this program asks for monospace today, so the crash would arrive the
    /// day something did.
    #[test]
    fn every_family_has_something_behind_it() {
        let fonts = definitions();
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let chain = fonts
                .families
                .get(&family)
                .unwrap_or_else(|| panic!("{family:?} was not bound at all"));
            assert!(!chain.is_empty(), "{family:?} was bound to an empty list");
            for name in chain {
                assert!(
                    fonts.font_data.contains_key(name),
                    "{family:?} names {name:?}, which has no font data"
                );
            }
        }
    }

    /// The fallback is last, so a glyph the system font lacks still draws.
    #[test]
    fn the_fallback_is_always_there_and_always_last() {
        let fonts = definitions();
        for chain in fonts.families.values() {
            assert_eq!(chain.last().map(String::as_str), Some(FALLBACK));
        }
    }

    /// The one thing looking at the window cannot tell you. If the face lookup,
    /// the registry walk or the file read breaks, the wizard still comes up —
    /// drawn in Ubuntu-Light, which is not what this machine uses anywhere else.
    ///
    /// Like [`super::volume`]'s tests, this one asserts against the real machine.
    #[test]
    #[cfg(windows)]
    fn the_system_font_is_the_one_actually_used() {
        let fonts = definitions();
        let chain = &fonts.families[&FontFamily::Proportional];
        assert_eq!(
            chain.first().map(String::as_str),
            Some(SYSTEM),
            "fell back to {FALLBACK}: this Windows' UI font could not be found or read, \
             so the wizard would draw in a typeface nothing else on the desktop uses"
        );
    }
}

mod catalog {
    use crate::catalog::{Entry, game_dir, image_file, slug};
    use std::path::Path;

    #[test]
    fn slugs_are_safe_on_any_filesystem() {
        // A game's name becomes a folder name, so this is the point where a
        // name someone typed turns into a path.
        assert_eq!(slug("Baldur's Gate 3"), "baldur_s_gate_3");
        assert_eq!(slug("Hollow Knight"), "hollow_knight");
        assert_eq!(slug("  NieR:Automata™  "), "nier_automata");
        assert_eq!(slug("!!!"), "game");
        assert_eq!(slug(""), "game");
    }

    #[test]
    fn removal_paths_stay_inside_the_cartridge() {
        let root = Path::new(r"E:\");
        let escape = Entry {
            name: "evil".into(),
            exe: "../../Windows/System32/cmd.exe".into(),
            image: "../../Windows/x.png".into(),
        };
        assert_eq!(game_dir(root, &escape), None);
        assert_eq!(image_file(root, &escape), None);

        let ok = Entry {
            name: "bg3".into(),
            exe: "games/bg3/bin/bg3.exe".into(),
            image: "images/bg3.png".into(),
        };
        assert_eq!(game_dir(root, &ok), Some(root.join("games").join("bg3")));
        assert_eq!(
            image_file(root, &ok),
            Some(root.join("images").join("bg3.png"))
        );

        // An exe sitting directly in games/ names no folder to delete.
        let shallow = Entry {
            name: "loose".into(),
            exe: "games/loose.exe".into(),
            image: "images/loose.png".into(),
        };
        assert_eq!(game_dir(root, &shallow), None);
    }
}

mod image {
    use crate::image::parse;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    fn riff(chunk: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(chunk);
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn reads_a_png() {
        assert_eq!(parse(&png_header(600, 900)), Some((600, 900)));
    }

    #[test]
    fn reads_an_animated_webp() {
        // VP8X — the shape the covers in this project actually are, and the
        // reason the format is sniffed from bytes rather than trusted from the
        // `.png` name they are usually saved under.
        let mut body = vec![0x10, 0, 0, 0]; // flags: has animation
        body.extend_from_slice(&[0x57, 0x02, 0x00]); // 600-1, 24-bit LE
        body.extend_from_slice(&[0x83, 0x03, 0x00]); // 900-1
        assert_eq!(parse(&riff(b"VP8X", &body)), Some((600, 900)));
    }

    #[test]
    fn reads_a_lossless_webp() {
        let bits: u32 = (599) | (899 << 14);
        let mut body = vec![0x2f];
        body.extend_from_slice(&bits.to_le_bytes());
        body.extend_from_slice(&[0; 8]);
        assert_eq!(parse(&riff(b"VP8L", &body)), Some((600, 900)));
    }

    #[test]
    fn reads_a_lossy_webp() {
        let mut body = vec![0x00, 0x00, 0x00, 0x9d, 0x01, 0x2a];
        body.extend_from_slice(&600u16.to_le_bytes());
        body.extend_from_slice(&900u16.to_le_bytes());
        body.extend_from_slice(&[0; 8]);
        assert_eq!(parse(&riff(b"VP8 ", &body)), Some((600, 900)));
    }

    #[test]
    fn reads_a_gif() {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&600u16.to_le_bytes());
        bytes.extend_from_slice(&900u16.to_le_bytes());
        assert_eq!(parse(&bytes), Some((600, 900)));
    }

    #[test]
    fn reads_a_jpeg_behind_a_metadata_segment() {
        let mut bytes = vec![0xff, 0xd8];
        // An APP1 (EXIF) segment the size walk has to step over.
        bytes.extend_from_slice(&[0xff, 0xe1, 0x00, 0x10]);
        bytes.extend_from_slice(&[0u8; 14]);
        // SOF0: length, precision, height, width.
        bytes.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&900u16.to_be_bytes());
        bytes.extend_from_slice(&600u16.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        assert_eq!(parse(&bytes), Some((600, 900)));
    }

    #[test]
    fn an_unrecognised_file_is_not_a_failure() {
        assert_eq!(parse(b"this is not a picture at all"), None);
        assert_eq!(parse(&[]), None);
    }
}

mod version {
    #[test]
    fn our_version_is_a_bare_three_part_number() {
        // The same shape the launcher and listener print. Nothing parses the
        // installer's, but three programs answering one question three ways is
        // how the one that *is* parsed eventually drifts.
        let version = env!("CARGO_PKG_VERSION");
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(parts.len(), 3, "{version}");
        for part in parts {
            assert!(
                part.parse::<u64>().is_ok(),
                "{version} has a non-numeric part"
            );
        }
    }
}

mod volume {
    /// The build carries at least one usable anchor. Same assertion as the
    /// listener's `this_build_has_something_to_trust` — the two must agree, or a
    /// cartridge one of them would accept the other silently would not.
    #[test]
    fn this_build_has_something_to_trust() {
        use crate::volume::ANCHORS;

        assert!(!ANCHORS.is_empty(), "build.rs produced no trust anchors");
        for anchor in ANCHORS {
            assert!(
                anchor.is_usable(),
                "keys/{}.pub is not a usable minisign public key",
                anchor.name
            );
        }
    }

    /// The finding this change exists to fix: a `launcher.exe` at a drive root is
    /// not believed just because it has the right name. Without a signing key
    /// this cannot construct something that *does* verify — that round trip is
    /// `trust`'s own suite — but every one of these must come back `None`
    /// rather than "close enough".
    #[test]
    fn only_a_verified_signature_makes_a_cartridge() {
        use crate::volume::attested_launcher;

        let dir = std::env::temp_dir().join(format!(
            "gacasy-installer-attest-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        // No file at all.
        assert_eq!(attested_launcher(&dir), None);

        // A file with the right name and nothing else — what running it used to
        // accept.
        std::fs::write(dir.join(crate::cartridge::LAUNCHER_NAME), b"MZ not signed")
            .expect("write");
        assert_eq!(attested_launcher(&dir), None);

        // A well-formed signature block from a key this build does not carry.
        let signature = "untrusted comment: signature from a key we do not have\n\
                         RUQAAAAAAAAAAOaGxHqZQ0KtvVCJ6iKzXG8bFvKZ0V0kZ1qWzKz0hVYQ4rZ8Xk1t\n\
                         trusted comment: gacasy-launcher 9.9.9 2026-07-30\n\
                         AAAA==\n";
        let signed = sigblock::attach(b"MZ signed by someone else", signature);
        std::fs::write(dir.join(crate::cartridge::LAUNCHER_NAME), signed).expect("write");
        assert_eq!(attested_launcher(&dir), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn the_drive_windows_is_on_is_refused() {
        use crate::volume::{Eligibility, drive_letter, is_system_drive, list};
        use std::path::Path;

        // The most important behaviour in this module, asserted against the
        // machine running the test rather than a fixture.
        let system = std::env::var_os("SystemRoot").expect("Windows sets SystemRoot");
        let letter = drive_letter(Path::new(&system)).expect("a drive letter");

        assert!(is_system_drive(Path::new(&format!("{letter}:\\"))));
        assert!(is_system_drive(Path::new(&format!(
            "{}:\\",
            letter.to_ascii_lowercase()
        ))));
        assert!(is_system_drive(Path::new(&format!("{letter}:\\games"))));

        for volume in list() {
            if drive_letter(&volume.root) == Some(letter) {
                assert_eq!(
                    volume.eligibility,
                    Eligibility::SystemDrive,
                    "{} must never be offered",
                    volume.root.display()
                );
                assert!(!volume.allowed());
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn nothing_internal_is_ever_allowed() {
        use crate::volume::{is_system_drive, list};

        for volume in list() {
            if volume.allowed() {
                assert!(
                    !is_system_drive(&volume.root),
                    "{} is the system drive",
                    volume.root.display()
                );
                assert!(
                    matches!(volume.bus, "USB" | "FireWire" | "SD" | "MMC" | "removable"),
                    "{} was allowed on a {} bus",
                    volume.root.display(),
                    volume.bus
                );
            }
        }
    }
}
