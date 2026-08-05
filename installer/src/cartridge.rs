// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Turning a volume into a cartridge — jobs 1 and 3, which are the same write.
//!
//! Creating a cartridge and editing one differ only in what the plan contains:
//! a create has nothing to keep and nothing to remove. Everything below is
//! shared, which is what keeps "add a game to an existing cartridge" from being
//! a second, subtly different implementation of "add a game".
//!
//! The layout written matches `../../launcher/structure.md` exactly:
//!
//! ```text
//! <volume>/
//!   launcher.exe     <- the app, from the embedded payload
//!   config.toml      <- look and feel only
//!   catalog.json     <- the game list this plan describes
//!   images/          <- one cover per game
//!   games/           <- the copied game installs
//! ```
//!
//! **There is no identity file.** What makes this a cartridge rather than a
//! folder is the minisign signature carried *inside* `launcher.exe`, which the
//! listener reads off the disk and checks against a key compiled into itself.
//! The installer neither writes nor knows any secret: it copies a binary that
//! was already signed at build time, and copying it is the entire act of
//! creating a cartridge's identity. Nothing on the disk can be edited to grant
//! that *file* trust it doesn't have.
//!
//! That is narrower than "trust", though, and worth being precise about:
//! `catalog.json`, `config.toml`, `images/` and `games/` are everything else
//! this function writes, and none of it is signed — there is no secret this
//! installer could sign it with. What stops that from mattering is the
//! launcher itself refusing to run anything those files name outside the
//! cartridge (`../../launcher/src/catalog.rs`), not the signature. See
//! `../../SIGNING.md`, §1.
//!
//! `EBWebView/` and `logs/` are not created — the launcher makes those on first
//! run.
//!
//! One part of a cartridge is not in that layout at all: its **name**, which is
//! the drive's volume label. A plan can carry a new one, and `apply` sets it
//! last — see [`Plan::label`] and `../volume.rs`.

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
    pub exe_relative: PathBuf,
    pub image: PathBuf,
    /// Measured by the scan, for the progress bar and the space check.
    pub bytes: u64,
}

impl PlannedGame {
    fn entry(&self) -> Entry {
        Entry {
            name: self.name.clone(),
            exe: catalog::exe_path(&self.slug, &self.exe_relative),
            image: catalog::image_path(&self.slug, &self.image),
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
    pub fn bytes_to_copy(&self) -> u64 {
        self.add.iter().map(|g| g.bytes).sum()
    }

    /// What the volume must have free. Space that removals will release is
    /// *not* subtracted: removals happen first, so if the estimate is tight the
    /// copy simply proceeds with the room they freed, and if this passes without
    /// them the answer was never in doubt.
    pub fn required_bytes(&self) -> u64 {
        // The size the launcher unpacks to, not the size it is carried at — what
        // lands on the drive is the whole exe.
        self.bytes_to_copy() + payload::LAUNCHER_BYTES + FREE_SPACE_SLACK
    }

    /// True when this plan would change nothing.
    ///
    /// A rename counts. Renaming is the one thing a plan can do without adding
    /// or removing a game, and leaving it out here would let the footer refuse a
    /// plan whose whole point was the new name.
    pub fn is_empty(&self) -> bool {
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

/// Applies a plan to its volume.
///
/// Order matters and is deliberate:
///
/// 1. **Removals first**, so an edit that swaps one game for another has the
///    space for the new one before it starts copying.
/// 2. **Games, then covers**, tracking what was created so a cancel can undo it.
/// 3. **catalog.json last of the content files.** Until it is written the
///    cartridge still describes its previous contents, so a cancel or a failure
///    leaves a cartridge that is *older* than intended rather than one that
///    lists games it doesn't have.
/// 4. `launcher.exe` last, because it is what makes this a cartridge at all —
///    it carries the signature the listener checks, so until it lands the volume
///    is inert, and a run that fails before this point leaves an ordinary folder
///    rather than a cartridge that half works.
/// 5. **The drive's name after even that**, so a cancelled or failed run never
///    reaches it. That is what keeps "the cartridge is as it was" literally true
///    of a cancel, and it means the rename needs no entry in [`unwind`] — the
///    only step that isn't a file is also the only one nothing has to undo.
///
/// Returns the one thing that can go wrong without the cartridge being wrong:
/// `Ok(Some(warning))` means everything was written and only the rename failed.
/// Reporting that as `Err` would tell the user a working cartridge didn't work,
/// and unwinding gigabytes of correctly copied games over a name is worse still.
pub fn apply(
    plan: &Plan,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Progress),
) -> Result<Option<String>, String> {
    if let Some(defect) = payload::defect() {
        return Err(defect);
    }
    let root = &plan.root;
    let total = plan.bytes_to_copy().max(1);
    let mut done = 0u64;

    for dir in [catalog::GAMES_DIR, catalog::IMAGES_DIR] {
        fs::create_dir_all(root.join(dir))
            .map_err(|e| format!("{}/ could not be created: {e}", dir))?;
    }

    for entry in &plan.remove {
        report(Progress {
            done,
            total,
            label: format!("Removing {}", entry.name),
        });
        remove_entry(root, entry)?;
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

        let cover = root.join(catalog::image_path(&game.slug, &game.image));
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

    let Some(label) = &plan.label else {
        return Ok(None);
    };
    report(Progress {
        done: total,
        total,
        label: "Naming the cartridge".into(),
    });
    Ok(crate::volume::set_label(root, label)
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
/// `catalog::game_dir`.
fn remove_entry(root: &Path, entry: &Entry) -> Result<(), String> {
    if let Some(dir) = catalog::game_dir(root, entry)
        && dir.is_dir()
    {
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("{} could not be removed: {e}", dir.display()))?;
    }
    if let Some(cover) = catalog::image_file(root, entry)
        && cover.is_file()
    {
        let _ = fs::remove_file(cover); // a leftover cover is harmless
    }
    Ok(())
}

/// Slugs already in use on the cartridge, so a new game can't be given a
/// folder name that would land on top of an existing one.
pub fn taken_slugs(entries: &[Entry]) -> HashSet<String> {
    crate::detect::taken_slugs(entries)
}
