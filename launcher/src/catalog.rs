// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The game list: `catalog.json` as the page needs to see it.
//!
//! The file is an array of `{ name, exe, image }`, with `exe` and `image`
//! relative to the content folder. Unlike config.toml, a broken catalog is
//! fatal — a launcher with no game list has nothing to show.
//!
//! # Why `exe` and `image` are checked here, not just joined
//!
//! `catalog.json` is cartridge content: whoever wrote the cartridge wrote this
//! file, and the launcher's own signature says nothing about it (see
//! `../../SIGNING.md`, §1). `Path::join` with a rooted path (`C:\Windows\...`)
//! or a UNC path (`\\host\share\...`) **discards the base entirely** rather than
//! refusing, so an unchecked `exe` field is a way to name any executable on the
//! machine or the network — and a signed, genuine launcher would run it. The
//! installer already refuses exactly this shape when *removing* a game
//! (`../../installer/src/catalog.rs::game_dir`); this is the same check, applied
//! before a game is ever offered to be launched rather than only when one is
//! deleted.

use std::fs;
use std::path::{Component, Path};

use serde::Deserialize;

use crate::log;

#[derive(Deserialize, Clone)]
pub struct Game {
    pub name: String,
    pub exe: String,
    pub image: String,
}

/// Reads `catalog.json` from the content folder (already seeded by
/// `content::ensure_layout`), dropping any entry whose `exe` or `image` does
/// not stay inside it.
pub fn load(base_dir: &Path) -> Vec<Game> {
    let path = base_dir.join("catalog.json");
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let games: Vec<Game> = serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));

    games
        .into_iter()
        .filter(|game| {
            let contained = is_contained(&game.exe) && is_contained(&game.image);
            if !contained {
                log::line(
                    base_dir,
                    &format!(
                        "REFUSED {}: catalog exe/image path escapes the cartridge \
                         (exe {:?}, image {:?})",
                        game.name, game.exe, game.image
                    ),
                );
            }
            contained
        })
        .collect()
}

/// Whether joining `relative` onto the cartridge root can only ever land
/// somewhere inside it.
///
/// A path that is entirely [`Component::Normal`] or [`Component::CurDir`]
/// stays inside; anything else — a drive prefix, a UNC root, a leading `/`, or
/// a `..` that climbs back out — is refused. `..` is refused outright rather
/// than resolved and range-checked: a symlink inside `games/` could otherwise
/// make an in-range path resolve somewhere it does not point at all, and this
/// crate has no need to support one game linking into another's folder.
pub(crate) fn is_contained(relative: &str) -> bool {
    Path::new(relative)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// The game list as handed to the page.
///
/// Rebuilt rather than passed through as the raw catalog text so each entry can
/// carry `available`: whether its exe is actually on the cartridge. Checked
/// once, at startup — a game whose files never shipped is a state of the
/// cartridge, not of a launch, and the page marks those covers as unplayable
/// instead of letting the player click into a guaranteed failure.
pub fn payload(base_dir: &Path, games: &[Game]) -> serde_json::Value {
    serde_json::Value::Array(
        games
            .iter()
            .map(|game| {
                serde_json::json!({
                    "name": game.name,
                    "exe": game.exe,
                    "image": game.image,
                    "available": base_dir.join(&game.exe).is_file(),
                })
            })
            .collect(),
    )
}
