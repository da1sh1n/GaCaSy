// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every test in this crate, one submodule per source module.
//!
//! Deliberately small. This is a UI-driven binary, and anything you can see by
//! running it — window sizing, config styling, log formatting — is not tested
//! here. What survives is the one thing a person cannot check by looking:
//! whether the version string the listener parses still has the shape the
//! listener requires.
//!
//! Run with `cargo test -p launcher`.

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
