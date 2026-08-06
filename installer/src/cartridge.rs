// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Writes a plan onto a volume: removals first, then game folders, covers,
//! `catalog.json`, `config.toml`, `launcher.exe`, and the drive's label last.
//! Unwinds what it created if a step fails or the user cancels.
//!
//! ```text
//! <volume>/
//!   launcher.exe     <- the app, from the embedded payload
//!   config.toml      <- look and feel only
//!   catalog.json     <- the game list this plan describes
//!   images/          <- one cover per game
//!   games/           <- the copied game installs
//! ```

// ########## WRITING A CARTRIDGE ##########

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::catalog::{self, Entry};
use crate::copy;
use crate::payload;

pub const CONFIG_FILE: &str = "config.toml";

/// The launcher's filename on a Windows cartridge, and the file the listener
/// looks for. Both sides hardcode it — see `../../listener/src/trust.rs`, which
/// explains why letting the cartridge name its own binary was a liability
/// rather than a feature.
pub const LAUNCHER_NAME: &str = "launcher.exe";

/// Headroom demanded on top of the measured bytes before a copy is offered.
///
/// The measurement is a sum of file sizes, and what a filesystem actually
/// consumes is that plus per-file slack, directory entries and whatever the
/// volume's cluster size rounds each file up to. Filling a cartridge to the last
/// byte also leaves the launcher no room for its log and WebView2 cache.
pub const FREE_SPACE_SLACK: u64 = 256 * 1024 * 1024;

/// A game being added, with everything already resolved: which folder, which exe
/// inside it, which cover, and what it will be called on the cartridge.
#[derive(Clone)]
pub struct PlannedGame {
    pub source: PathBuf,
    pub name: String,
    pub slug: String,
    /// The chosen executable, relative to `source`.
    pub exeRelative: PathBuf,
    pub image: PathBuf,
    /// Measured by the scan, for the progress bar and the space check.
    pub bytes: u64,
}

impl PlannedGame {
    fn entry(&self) -> Entry {
        Entry {
            name: self.name.clone(),
            exe: catalog::exePath(&self.slug, &self.exeRelative),
            image: catalog::imagePath(&self.slug, &self.image),
        }
    }
}

/// Everything one run of the installer will do to one volume.
pub struct Plan {
    pub root: PathBuf,
    /// Catalog entries already on the cartridge that stay untouched.
    pub keep: Vec<Entry>,
    /// Entries to delete, with their files.
    pub remove: Vec<Entry>,
    pub add: Vec<PlannedGame>,
    /// The name to give the drive, when it differs from the one it has now.
    /// `Some("")` clears the label; `None` leaves it alone.
    pub label: Option<String>,
}

impl Plan {
    /// The catalog this plan results in — kept games first, in their existing
    /// order, then the new ones.
    pub fn entries(&self) -> Vec<Entry> {
        let mut entries = self.keep.clone();
        entries.extend(self.add.iter().map(PlannedGame::entry));
        entries
    }

    /// Bytes the copy has to move. Cover images are rounding error next to game
    /// folders and are not counted.
    pub fn bytesToCopy(&self) -> u64 {
        self.add.iter().map(|g| g.bytes).sum()
    }

    /// What the volume must have free. Space that removals will release is
    /// *not* subtracted: removals happen first, so if the estimate is tight the
    /// copy simply proceeds with the room they freed, and if this passes without
    /// them the answer was never in doubt.
    pub fn requiredBytes(&self) -> u64 {
        // The size the launcher unpacks to, not the size it is carried at — what
        // lands on the drive is the whole exe.
        self.bytesToCopy() + payload::LAUNCHER_BYTES + FREE_SPACE_SLACK
    }

    /// True when this plan would change nothing.
    ///
    /// A rename counts. Renaming is the one thing a plan can do without adding
    /// or removing a game, and leaving it out here would let the footer refuse a
    /// plan whose whole point was the new name.
    pub fn isEmpty(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty() && self.label.is_none()
    }
}

/// How far along `apply` is, for the progress bar.
pub struct Progress {
    pub done: u64,
    pub total: u64,
    /// What is happening right now, in the user's words.
    pub label: String,
}

/// Applies `plan` to its volume, reporting progress and honouring a cancel.
///
/// The order is the safety property: removals, then games, then covers, then
/// `catalog.json`, then `launcher.exe`, then the drive's name. Every stage
/// leaves the cartridge either as it was or older than intended, never
/// describing games it does not have.
///
/// `Ok(Some(warning))` is the one thing that can go wrong without the cartridge
/// being wrong: everything landed and only the rename failed.
pub fn apply(
    plan: &Plan,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Progress),
) -> Result<Option<String>, String> {
    if let Some(defect) = payload::defect() {
        return Err(defect);
    }
    let root = &plan.root;
    let total = plan.bytesToCopy().max(1);
    let mut done = 0u64;

    for dir in [catalog::GAMES_DIR, catalog::IMAGES_DIR] {
        fs::create_dir_all(root.join(dir))
            .map_err(|e| format!("{}/ could not be created: {e}", dir))?;
    }

    // Removals first, so an edit that swaps one game for another has the space
    // for the new one before any copying starts.
    for entry in &plan.remove {
        report(Progress {
            done,
            total,
            label: format!("Removing {}", entry.name),
        });
        removeEntry(root, entry)?;
    }

    // Everything this run created, newest last. Only these are undone on
    // failure — content that was already on the cartridge is never touched.
    let mut created: Vec<PathBuf> = Vec::new();

    for game in &plan.add {
        let destination = root.join(catalog::GAMES_DIR).join(&game.slug);
        created.push(destination.clone());

        let result = copy::directory(&game.source, &destination, cancel, &mut |file, bytes| {
            done += bytes;
            report(Progress {
                done,
                total,
                label: format!(
                    "Copying {} — {}",
                    game.name,
                    file.file_name().unwrap_or_default().to_string_lossy()
                ),
            });
        });
        if let Err(e) = result {
            return Err(unwind(&created, e.message()));
        }

        let cover = root.join(catalog::imagePath(&game.slug, &game.image));
        created.push(cover.clone());
        if let Err(e) = copy::single(&game.image, &cover, cancel) {
            return Err(unwind(&created, e.message()));
        }
    }

    report(Progress {
        done: total,
        total,
        label: "Writing the cartridge".into(),
    });

    // catalog.json after the files it names: until this lands the cartridge
    // still describes its previous contents, so a cancel leaves one that is
    // older than intended rather than one listing games it does not have.
    if let Err(e) = catalog::write(root, &plan.entries()) {
        return Err(unwind(&created, format!("catalog.json: {e}")));
    }

    // config.toml is look and feel, and it belongs to whoever owns the
    // cartridge: seeded when absent, never overwritten. The same rule the
    // launcher applies to it on a real cartridge (../../launcher/src/content.rs).
    let config = root.join(CONFIG_FILE);
    if !config.exists()
        && let Err(e) = copy::bytes(&config, payload::LAUNCHER_CONFIG)
    {
        return Err(unwind(&created, e.message()));
    }

    // The launcher *is* refreshed, unlike the config: it is program, not
    // preference, and an edit pass is the natural moment for a cartridge to pick
    // up a newer one. Refreshing it also re-establishes identity — the signature
    // rides inside these bytes — so an old cartridge edited by a new installer
    // comes away trusted by whatever listener that installer ships with.
    let launcher = match payload::launcher() {
        Ok(bytes) => bytes,
        Err(problem) => return Err(unwind(&created, problem)),
    };
    if let Err(e) = copy::bytes(&root.join(LAUNCHER_NAME), &launcher) {
        return Err(unwind(&created, e.message()));
    }

    // The name comes after even the launcher, so a cancelled or failed run
    // never reaches it — which is what makes "the cartridge is as it was"
    // literally true of a cancel, and why the rename needs no unwind entry.
    let Some(label) = &plan.label else {
        return Ok(None);
    };
    report(Progress {
        done: total,
        total,
        label: "Naming the cartridge".into(),
    });
    Ok(crate::volume::setLabel(root, label)
        .err()
        .map(|e| format!("The drive could not be renamed: {e}. Everything else was written.")))
}

/// Deletes what this run created and returns the message to show.
///
/// Half a game folder is worse than none: the launcher would list a game whose
/// files are incomplete, and the user would have no way to tell which. Failures
/// during the cleanup are swallowed — the original problem is the one worth
/// reporting, and a second error about it would only bury it.
fn unwind(created: &[PathBuf], reason: String) -> String {
    for path in created.iter().rev() {
        if path.is_dir() {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
    reason
}

/// Deletes one catalog entry's files. Paths that don't resolve to somewhere
/// inside the cartridge are skipped rather than followed — see
/// `catalog::gameDir`.
fn removeEntry(root: &Path, entry: &Entry) -> Result<(), String> {
    if let Some(dir) = catalog::gameDir(root, entry)
        && dir.is_dir()
    {
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("{} could not be removed: {e}", dir.display()))?;
    }
    if let Some(cover) = catalog::imageFile(root, entry)
        && cover.is_file()
    {
        let _ = fs::remove_file(cover); // a leftover cover is harmless
    }
    Ok(())
}

/// Slugs already in use on the cartridge, so a new game can't be given a
/// folder name that would land on top of an existing one.
pub fn takenSlugs(entries: &[Entry]) -> HashSet<String> {
    crate::detect::takenSlugs(entries)
}
