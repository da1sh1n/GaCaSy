// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Loads and validates `catalog.json` into the `Game` list the page is given,
//! marking each entry present or missing. Rejects any `exe` or `image` path
//! that would resolve outside the cartridge.

// ########## THE GAME LIST ##########

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
/// `content::ensureLayout`), dropping any entry whose `exe` or `image` does
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
            let contained = isContained(&game.exe) && isContained(&game.image);
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
/// somewhere inside it. Everything that is not a plain component — a drive
/// prefix, a UNC root, a leading `/`, a `..` — is refused.
///
/// `..` is refused outright rather than resolved and range-checked, because a
/// symlink inside `games/` could make an in-range path resolve somewhere it
/// does not point at all.
pub(crate) fn isContained(relative: &str) -> bool {
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
