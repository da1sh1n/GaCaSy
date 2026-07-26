// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The game list: `catalog.json` as the page needs to see it.
//!
//! The file is an array of `{ name, exe, image }`, with `exe` and `image`
//! relative to the content folder. Unlike config.toml, a broken catalog is
//! fatal — a launcher with no game list has nothing to show.

use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Game {
    pub name: String,
    pub exe: String,
    pub image: String,
}

/// Reads `catalog.json` from the content folder (already seeded by
/// `content::ensure_layout`).
pub fn load(base_dir: &Path) -> Vec<Game> {
    let path = base_dir.join("catalog.json");
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&json).unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
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
