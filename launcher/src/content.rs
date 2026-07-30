// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Where the cartridge's content lives, and putting it there on first run.
//!
//! Everything the exe reads from disk sits in one folder beside it — config,
//! catalog, cover art, games, logs, WebView2's own cache. This module answers
//! *which* folder that is, creates what's missing, and keeps the shippable
//! `output/launcher.exe` in step with the source during development.
//!
//! Cartridge content — covers, games, catalog — is never overwritten once
//! present, so hand-dropped files survive every build. `config.toml` is the one
//! exception; see [`ensure_layout`].

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Baked-in defaults so a fresh `output/` can be seeded with no repo around
/// (e.g. on a real cartridge).
const DEFAULT_CONFIG: &str = include_str!("config.toml");
const DEFAULT_CATALOG: &str = include_str!("catalog.json");

/// True when this is the deployed launcher rather than a `cargo run` build out
/// of `target/`.
///
/// A real cartridge can sit anywhere the installer or its owner puts it —
/// often a drive root, which has no folder name at all — so "deployed" cannot
/// be recognized by the name of the exe's parent folder. What's actually
/// distinctive about a `cargo run` build is that it lives under a `target/`
/// directory; nothing else does.
pub fn running_deployed() -> bool {
    let Ok(exe) = env::current_exe() else {
        return true;
    };
    !exe.components().any(|c| c.as_os_str() == "target")
}

/// The folder holding `launcher.exe` and all cartridge content.
///
/// When the deployed exe runs (its parent folder is named `output`), that
/// folder is the base. Under `cargo run` the exe lives in target/, so the
/// base is the repo's own `output/`.
pub fn resolve_base_dir() -> PathBuf {
    if running_deployed() {
        let exe = env::current_exe().expect("failed to resolve current exe path");
        exe.parent()
            .expect("current exe has no parent directory")
            .to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("output")
    }
}

/// Creates the content folders, puts config.toml/catalog.json in place, and
/// refreshes the deployed exe. Cartridge content — covers, games, catalog —
/// is never touched once present, so hand-dropped files survive every build.
pub fn ensure_layout(base: &Path) {
    for sub in ["games", "images", "logs", "EBWebView"] {
        fs::create_dir_all(base.join(sub))
            .unwrap_or_else(|e| panic!("failed to create output/{sub}/: {e}"));
    }

    // config.toml is the one file with two different rules, because in the repo
    // it has a master and on a cartridge it doesn't:
    //
    //   dev      — src/config.toml is the master and output/'s copy is exactly
    //              that, rewritten every run. Edit the one in src/.
    //   deployed — written once if missing, then never rewritten. The
    //              cartridge's owner owns its config, and an update must not
    //              restyle their launcher out from under them. The one thing
    //              that does still happen is `config::sync_defaults` appending
    //              (commented, inert) documentation for a setting that didn't
    //              exist when this file was written — see its doc comment.
    if running_deployed() {
        let config_path = base.join("config.toml");
        seed_if_missing(&config_path, DEFAULT_CONFIG);
        // A no-op for a config.toml that seed_if_missing just wrote fresh (it
        // already has every key); this is what catches up one written before
        // some setting existed.
        crate::config::sync_defaults(&config_path);
    } else {
        mirror_seed_config(base);
    }

    seed_if_missing(&base.join("catalog.json"), DEFAULT_CATALOG);
    refresh_deployed_exe(base);
}

/// Copies the seed config over `output/config.toml`. Prefers the live file in
/// the source tree over the copy baked in at compile time — same reasoning as
/// the live UI assets in `assets::handle_request`, so editing src/config.toml
/// and re-running takes effect even if the binary itself wasn't rebuilt.
///
/// A failure here is not fatal: the previous copy is still a usable config.
fn mirror_seed_config(base: &Path) {
    let live = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("config.toml");
    let contents = fs::read_to_string(&live).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let _ = fs::write(base.join("config.toml"), contents);
}

fn seed_if_missing(path: &Path, contents: &str) {
    if !path.exists() {
        fs::write(path, contents)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
    }
}

/// Copies the freshly built exe to `output/launcher.exe` so the shippable
/// copy tracks the source. Skipped when we already are that copy; failure
/// (e.g. a deployed instance holding the file open) is non-fatal.
fn refresh_deployed_exe(base: &Path) {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let dst = base.join("launcher.exe");
    if let (Ok(a), Ok(b)) = (exe.canonicalize(), dst.canonicalize()) {
        if a == b {
            return;
        }
    }
    let _ = fs::copy(&exe, &dst);
}
