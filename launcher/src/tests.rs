// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every test in this crate, one submodule per source module.
//!
//! Deliberately small. This is a UI-driven binary, and anything you can see by
//! running it — window sizing, config styling, log formatting — is not tested
//! here. What survives are the things a person cannot check by looking: whether
//! the version string the listener parses still has the shape the listener
//! requires, whether a `catalog.json` entry can still name a path outside the
//! cartridge (the field this launcher's own signature has never covered),
//! whether a half-written cover order still comes out usable, and whether
//! writing one back leaves the rest of somebody's config.toml alone.
//!
//! Run with `cargo test -p launcher`.

mod catalog {
    use crate::catalog::is_contained;

    #[test]
    fn ordinary_relative_paths_stay_inside() {
        assert!(is_contained("games/bg3/bg3.exe"));
        assert!(is_contained("images/bg3.png"));
        assert!(is_contained("./games/bg3/bg3.exe"));
    }

    #[test]
    fn a_drive_letter_escapes() {
        // `Path::join` with this discards the base entirely — the whole reason
        // this check exists rather than trusting `join` to contain it.
        assert!(!is_contained(r"C:\Windows\System32\cmd.exe"));
        assert!(!is_contained("C:/Windows/System32/cmd.exe"));
    }

    #[test]
    fn a_unc_path_escapes() {
        assert!(!is_contained(r"\\attacker.example\share\payload.exe"));
    }

    #[test]
    fn a_leading_root_escapes() {
        assert!(!is_contained("/etc/passwd"));
    }

    #[test]
    fn a_parent_dir_component_escapes() {
        assert!(!is_contained("../../evil.exe"));
        assert!(!is_contained("games/../../evil.exe"));
        // Buried in an otherwise-ordinary-looking path — the shape most likely
        // to slip past a reviewer's eye rather than a machine's.
        assert!(!is_contained("games/bg3/../../../evil.exe"));
    }
}

mod order {
    use crate::order::{normalize, promote};

    #[test]
    fn an_empty_list_is_plain_catalog_order() {
        // The state every cartridge starts in: nothing played, nothing
        // arranged, so the covers appear the way their author listed them.
        assert_eq!(normalize(&[], 4), vec![0, 1, 2, 3]);
    }

    #[test]
    fn a_partial_list_keeps_its_order_and_gains_the_rest() {
        // What a cartridge looks like after one game has been played, and what
        // adding a fourth game to a three-game cartridge leaves behind: the
        // newcomer lands at the end rather than invalidating the list.
        assert_eq!(normalize(&[2], 4), vec![2, 0, 1, 3]);
        assert_eq!(normalize(&[3, 1], 4), vec![3, 1, 0, 2]);
    }

    #[test]
    fn out_of_range_ids_are_dropped() {
        // A hand-edited list, or one written when the cartridge held more
        // games than it does now. The survivors keep their order.
        assert_eq!(normalize(&[9, 1, 400], 3), vec![1, 0, 2]);
    }

    #[test]
    fn repeats_count_only_the_first_time() {
        // Without this the result would be longer than the catalog and the
        // duplicate would be drawn twice.
        assert_eq!(normalize(&[1, 1, 0, 1], 3), vec![1, 0, 2]);
    }

    #[test]
    fn an_empty_catalog_normalizes_to_nothing() {
        assert!(normalize(&[0, 1], 0).is_empty());
    }

    #[test]
    fn promoting_moves_one_id_and_disturbs_nothing_else() {
        assert_eq!(promote(&[0, 1, 2, 3], 4, 2), vec![2, 0, 1, 3]);
        // Already first: still a no-op rather than a shuffle.
        assert_eq!(promote(&[2, 0, 1, 3], 4, 2), vec![2, 0, 1, 3]);
    }

    #[test]
    fn promoting_repairs_the_list_it_promotes_into() {
        // The realistic case: a config somebody edited badly, and then a game
        // was played. The write that follows must not carry the mess forward.
        assert_eq!(promote(&[7, 1, 1], 3, 0), vec![0, 1, 2]);
    }

    #[test]
    fn promoting_an_id_that_isnt_there_still_normalizes() {
        // `id` out of range can only come from a bug, and the answer is the
        // order unchanged — not a panic, and not an id in the file that names
        // no game.
        assert_eq!(promote(&[2, 0, 1], 3, 9), vec![2, 0, 1]);
    }
}

mod config {
    use std::fs;

    /// A scratch directory that cleans itself up, so the round-trip below can
    /// use a real file — which is the whole point of it, since what's being
    /// checked is what ends up on disk.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("romzeta-launcher-test-{name}"));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("failed to create the scratch directory");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const HAND_WRITTEN: &str = "\
# A comment somebody wrote.
show_captions = true

# Another one, about the order.
usage_order = [1, 0]

# And a trailing note.
border_gap = 36
";

    #[test]
    fn storing_a_key_leaves_the_rest_of_the_file_alone() {
        // The property the whole persistence design rests on. config.toml is
        // mostly prose written for a person; a launcher that reformatted it as
        // a side effect of somebody starting a game would be answering a
        // question nobody asked.
        let scratch = Scratch::new("store-preserves");
        fs::write(scratch.0.join("config.toml"), HAND_WRITTEN).unwrap();

        crate::config::store(&scratch.0, "usage_order", crate::config::ids(&[2, 1, 0]));

        let after = fs::read_to_string(scratch.0.join("config.toml")).unwrap();
        assert!(after.contains("usage_order = [2, 1, 0]"), "{after}");
        assert!(after.contains("# A comment somebody wrote."), "{after}");
        assert!(after.contains("# Another one, about the order."), "{after}");
        assert!(after.contains("# And a trailing note."), "{after}");
        assert!(after.contains("show_captions = true"), "{after}");
        assert!(after.contains("border_gap = 36"), "{after}");

        // And it is still readable — the check the assertions above can't make
        // on their own, since they only look for text.
        let config = crate::config::load(&scratch.0);
        assert_eq!(config.usage_order, vec![2, 1, 0]);
        assert!(config.show_captions);
    }

    #[test]
    fn storing_a_key_the_file_never_had_appends_it() {
        // A cartridge set up before the setting existed. It has to arrive
        // documented, the same way `sync_defaults` would have introduced it.
        let scratch = Scratch::new("store-appends");
        fs::write(scratch.0.join("config.toml"), "border_gap = 36\n").unwrap();

        crate::config::store(&scratch.0, "order_mode", "alphabetic".into());

        let after = fs::read_to_string(scratch.0.join("config.toml")).unwrap();
        assert!(after.contains("order_mode = \"alphabetic\""), "{after}");
        assert!(after.contains("# Cover order:"), "{after}");
        assert_eq!(crate::config::load(&scratch.0).order_mode, "alphabetic");
    }

    #[test]
    fn a_bad_value_costs_only_that_setting() {
        // The rule `load` is built around, asserted for the two new readers:
        // an unknown mode and a list holding things that aren't ids leave the
        // default in place without taking the rest of the file down with them.
        let scratch = Scratch::new("bad-values");
        fs::write(
            scratch.0.join("config.toml"),
            "order_mode = \"nonsense\"\nusage_order = [0, \"x\", 2]\nshow_captions = true\n",
        )
        .unwrap();

        let config = crate::config::load(&scratch.0);
        assert_eq!(config.order_mode, crate::constants::DEFAULT_ORDER_MODE);
        assert_eq!(config.usage_order, vec![0, 2]);
        assert!(config.show_captions);
    }
}

mod version {
    #[test]
    fn our_version_is_a_bare_three_part_number() {
        // What the listener's parser accepts, asserted from this side too: if
        // this ever became "v0.2.0" or "0.2", every cartridge built from it
        // would stop being launchable and the log would say only that the
        // version was unreadable.
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
