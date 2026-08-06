// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Resolves the folder holding the cartridge's content, creates the layout on
//! first run, seeds `config.toml` and `catalog.json` when absent, and refreshes
//! the deployed exe during development.

// ########## THE CONTENT FOLDER ##########

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Baked-in defaults so a fresh `output/` can be seeded with no repo around
/// (e.g. on a real cartridge).
const DEFAULT_CONFIG: &str = include_str!("config.toml");
const DEFAULT_CATALOG: &str = include_str!("catalog.json");

/// True when this is the deployed launcher rather than a `cargo run` build.
///
/// Recognised by the exe living under a `target/` directory, not by its parent
/// folder's name: a real cartridge sits wherever its owner put it, often a
/// drive root, which has no folder name at all.
pub fn runningDeployed() -> bool {
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
pub fn resolveBaseDir() -> PathBuf {
    if runningDeployed() {
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
pub fn ensureLayout(base: &Path) {
    // Cover art and WebView2's cache both live under assets/ so the cartridge
    // root holds only what a person put there: the exe, the two data files,
    // their games, and the logs.
    //
    // `images/` is deliberately NOT created any more. An older cartridge that
    // has one keeps it, and `assets::handleRequest` still serves from it —
    // but a fresh cartridge should not be given an empty folder it will never
    // use just because the previous layout had one.
    for sub in ["games", "logs", "assets/images", "assets/EBWebView"] {
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
    //              that does still happen is `config::syncDefaults` appending
    //              (commented, inert) documentation for a setting that didn't
    //              exist when this file was written — see its doc comment.
    if runningDeployed() {
        let config_path = base.join("config.toml");
        seedIfMissing(&config_path, DEFAULT_CONFIG);
        // A no-op for a config.toml that seedIfMissing just wrote fresh (it
        // already has every key); this is what catches up one written before
        // some setting existed.
        crate::config::syncDefaults(&config_path);
    } else {
        mirrorSeedConfig(base);
    }

    seedIfMissing(&base.join("catalog.json"), DEFAULT_CATALOG);
    refreshDeployedExe(base);
}

/// Copies the seed config over `output/config.toml` during development,
/// preferring the live file in the source tree over the compiled-in copy so an
/// edit takes effect without a rebuild.
///
/// The three order keys survive the copy: the seed owns look and feel, but the
/// launcher owns the order and flattening it every run would make that half
/// impossible to try out in the repo. Not fatal on failure — the previous copy
/// is still a usable config.
fn mirrorSeedConfig(base: &Path) {
    // Read before overwriting — this is the only moment the old values exist.
    let carried = crate::config::load(base);

    let live = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("config.toml");
    let contents = fs::read_to_string(&live).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    if fs::write(base.join("config.toml"), contents).is_err() {
        return;
    }

    crate::config::store(base, "order_mode", carried.order_mode.into());
    crate::config::store(
        base,
        "usage_order",
        crate::config::ids(&carried.usage_order),
    );
    crate::config::store(base, "user_order", crate::config::ids(&carried.user_order));
}

fn seedIfMissing(path: &Path, contents: &str) {
    if !path.exists() {
        fs::write(path, contents)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
    }
}

/// Copies the freshly built exe to `output/launcher.exe` so the shippable
/// copy tracks the source. Skipped when we already are that copy; failure
/// (e.g. a deployed instance holding the file open) is non-fatal.
fn refreshDeployedExe(base: &Path) {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let dst = base.join("launcher.exe");
    // Canonicalized before comparing, so a path reached two different ways
    // (a symlink, `..`, a short 8.3 name) is still recognised as the same file.
    if let (Ok(a), Ok(b)) = (exe.canonicalize(), dst.canonicalize())
        && a == b
    {
        return;
    }
    let _ = fs::copy(&exe, &dst);
}
