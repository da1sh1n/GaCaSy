// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! `catalog.json` — the game list, as the launcher deserializes it.
//!
//! The producer side of `../../launcher/src/catalog.rs`: an array of
//! `{ name, exe, image }` with both paths **relative to the cartridge root**
//! (`games/bg3/bg3.exe`, `assets/images/bg3.png`). The launcher joins them onto
//! its own folder, so an absolute path here would produce a cartridge that only
//! works on the machine that made it.
//!
//! Separators are always `/`. Windows joins either kind, and a cartridge with
//! backslashes in its catalog would stop working the day a Linux launcher reads
//! it.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CATALOG_FILE: &str = "catalog.json";
pub const GAMES_DIR: &str = "games";
/// Cover art, under the cartridge's `assets/` folder alongside WebView2's own
/// cache — so the root holds only what a person put there.
///
/// This was a bare `images/` and a cartridge written by an older installer
/// still says so in its catalog. Nothing needs migrating: the launcher serves
/// whatever path the catalog names, and accepts both prefixes.
pub const IMAGES_DIR: &str = "assets/images";

/// One row of the catalog. Field names and types match the launcher's `Game`
/// exactly; it is a hard error there if they don't.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub exe: String,
    pub image: String,
}

/// Reads an existing cartridge's catalog.
///
/// A missing file is an empty list, not an error: a volume can carry a
/// `.cartridge` marker and no catalog yet, and edit mode should let you add the
/// first game rather than refusing to open. A file that is *there* but unparsable
/// does fail, because overwriting it would throw away a list we couldn't read.
pub fn read(root: &Path) -> Result<Vec<Entry>, String> {
    let path = root.join(CATALOG_FILE);
    let json = match fs::read_to_string(&path) {
        Ok(json) => json,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{} could not be read: {e}", path.display())),
    };
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&json)
        .map_err(|e| format!("{} is not a valid catalog: {e}", path.display()))
}

/// Writes the catalog, pretty-printed — it is a file people open and edit by
/// hand on a cartridge that is already made.
pub fn write(root: &Path, entries: &[Entry]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(entries).expect("catalog entries always serialize");
    fs::write(root.join(CATALOG_FILE), json + "\n")
}

/// `games/<slug>/<exe>` for the catalog's `exe` field.
pub fn exe_path(slug: &str, exe_relative: &Path) -> String {
    format!("{GAMES_DIR}/{slug}/{}", to_relative_string(exe_relative))
}

/// `images/<slug>.<ext>` for the catalog's `image` field.
///
/// The extension is kept from the file the user picked rather than forced to
/// `.png`: the launcher hands the path to the webview, which goes by content and
/// not by name, and renaming a `.jpg` to `.png` only makes the cartridge harder
/// to understand later.
pub fn image_path(slug: &str, source: &Path) -> String {
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "png".into());
    format!("{IMAGES_DIR}/{slug}.{ext}")
}

/// A path relative to something, as the catalog spells it: `/` separators, no
/// leading `./`.
pub fn to_relative_string(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// A folder- and URL-safe name derived from the game's, used for both
/// `games/<slug>/` and `images/<slug>.png`.
///
/// The alternative — keeping the source folder's name — puts whatever the user's
/// disk happened to hold (spaces, `™`, a trailing dot) into a path that has to
/// survive a JSON file, a `file://`-ish webview fetch and a FAT32 volume. This
/// is the one place worth normalising.
pub fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_').to_string();
    if slug.is_empty() { "game".into() } else { slug }
}

/// `slug`, with a numeric suffix if that name is already taken on the cartridge.
///
/// Only ever reached for two *differently named* games that squash to the same
/// slug (`Game: II` and `Game II`). Two adds of the same folder are refused up
/// in the games screen, where the user can still do something about it — see
/// `../structure.md`, "Adding the same game twice".
pub fn unique_slug(name: &str, taken: &mut std::collections::HashSet<String>) -> String {
    let base = slug(name);
    let mut candidate = base.clone();
    let mut n = 2;
    while !taken.insert(candidate.clone()) {
        candidate = format!("{base}_{n}");
        n += 1;
    }
    candidate
}

/// The folder on the cartridge holding one entry's game files, or `None` if the
/// entry's `exe` doesn't name one inside `games/`.
///
/// Used by remove, which deletes that folder — so a path escaping the cartridge
/// (`../../Windows`) must resolve to nothing rather than to a directory tree.
pub fn game_dir(root: &Path, entry: &Entry) -> Option<PathBuf> {
    let mut parts = Vec::new();
    for component in Path::new(&entry.exe).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    // games/<slug>/… — anything shallower names no folder of its own.
    if parts.len() < 3 || parts[0] != GAMES_DIR {
        return None;
    }
    Some(root.join(&parts[0]).join(&parts[1]))
}

/// The cover file on the cartridge for one entry, with the same escape check.
pub fn image_file(root: &Path, entry: &Entry) -> Option<PathBuf> {
    let mut resolved = root.to_path_buf();
    let mut parts = 0;
    for component in Path::new(&entry.image).components() {
        match component {
            Component::Normal(part) => {
                resolved.push(part);
                parts += 1;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (parts > 0).then_some(resolved)
}
