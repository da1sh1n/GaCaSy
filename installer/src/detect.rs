// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Finding the exe inside a game folder — the fiddliest part of job 1.
//!
//! A game install is full of executables that are not the game: uninstallers,
//! redistributable bundles, crash handlers, engine tools. This module walks the
//! folder once, throws out the ones that are never the answer, and scores what
//! is left so the likeliest is preselected.
//!
//! It is a guess, and it is presented as one. The user can always override the
//! pick, and when the scoring finds no clear winner the UI *requires* them to
//! choose rather than quietly taking the top row.
//!
//! The same walk measures the folder, because the copy needs a byte total for
//! the free-space check and walking a multi-gigabyte install twice is a wait the
//! user would feel.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// How much better the top score must be before it is treated as a clear
/// winner. One depth level is worth [`DEPTH_PENALTY`], so this threshold means
/// "shallower than the runner-up, or a better name match" — a rank the runner-up
/// can't be within noise of.
const CLEAR_WINNER_MARGIN: i64 = DEPTH_PENALTY;

/// Cost of each folder level between the game root and the exe. The launcher of
/// a game is near its root; its tools are buried.
const DEPTH_PENALTY: i64 = 120;

/// The exe is named after the folder it is in — by far the strongest signal.
const EXACT_NAME_BONUS: i64 = 500;
/// Weaker version of the same: the name contains the folder name or vice versa.
const PARTIAL_NAME_BONUS: i64 = 200;

/// Size contributes, but only as a tiebreak — capped so a 40 GB packed
/// executable cannot outrank a correctly named one at the root.
const MAX_SIZE_SCORE: i64 = 100;

/// Stop walking below this depth. Nothing this deep in a game folder is the
/// game, and it bounds the scan of a pathological tree.
const MAX_DEPTH: usize = 8;

/// A file this small is a stub or a shim, not a game binary.
const MIN_PLAUSIBLE_BYTES: u64 = 16 * 1024;

/// One executable the walk found, with its path relative to the game folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub relative: PathBuf,
    pub bytes: u64,
    pub score: i64,
}

impl Candidate {
    /// `bin/Game.exe` with forward slashes — the form that goes in the catalog
    /// and the form shown in the picker, so what the user chose and what was
    /// written are visibly the same string.
    pub fn display(&self) -> String {
        crate::catalog::to_relative_string(&self.relative)
    }
}

/// What one walk of a game folder found.
pub struct Scan {
    pub candidates: Vec<Candidate>,
    /// Every file, not just executables — the copy has to move all of it.
    pub total_bytes: u64,
    pub file_count: usize,
    /// True when the walk stopped early because it was cancelled.
    pub cancelled: bool,
}

impl Scan {
    /// The candidate to preselect, or `None` when the user has to decide.
    ///
    /// `None` covers both halves of the spec's rule: nothing survived the reject
    /// list, or the top two are too close to call.
    pub fn clear_winner(&self) -> Option<&Candidate> {
        let best = self.candidates.first()?;
        match self.candidates.get(1) {
            Some(runner_up) if best.score - runner_up.score < CLEAR_WINNER_MARGIN => None,
            _ => Some(best),
        }
    }
}

/// Walks `root` once: collects executables, totals every file's size.
///
/// Runs on a worker thread — a game folder can hold hundreds of thousands of
/// files — and checks `cancel` as it goes so closing the screen doesn't leave a
/// thread grinding through an install.
pub fn scan(root: &Path, cancel: &AtomicBool) -> Scan {
    let folder_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut scan = Scan {
        candidates: Vec::new(),
        total_bytes: 0,
        file_count: 0,
        cancelled: false,
    };
    walk(root, Path::new(""), 0, cancel, &mut scan);

    for candidate in &mut scan.candidates {
        candidate.score = score(&candidate.relative, &folder_name, candidate.bytes);
    }
    // Descending score; the path breaks ties so the order is stable between runs
    // and the "is the top one clearly ahead" test can't flip on a re-scan.
    scan.candidates
        .sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.relative.cmp(&b.relative)));
    scan
}

fn walk(dir: &Path, relative: &Path, depth: usize, cancel: &AtomicBool, scan: &mut Scan) {
    if depth > MAX_DEPTH || cancel.load(Ordering::Relaxed) {
        scan.cancelled |= cancel.load(Ordering::Relaxed);
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        if cancel.load(Ordering::Relaxed) {
            scan.cancelled = true;
            return;
        }
        let Ok(kind) = entry.file_type() else { continue };
        let child = relative.join(entry.file_name());

        // Symlinks are neither followed nor counted: a link loop would make the
        // walk unbounded, and the copy doesn't follow them either, so counting
        // one would inflate the free-space estimate.
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            walk(&entry.path(), &child, depth + 1, cancel, scan);
            continue;
        }

        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        scan.total_bytes += bytes;
        scan.file_count += 1;

        if is_executable(&child) && !is_rejected(&child) && bytes >= MIN_PLAUSIBLE_BYTES {
            scan.candidates.push(Candidate {
                relative: child,
                bytes,
                score: 0,
            });
        }
    }
}

fn is_executable(relative: &Path) -> bool {
    relative
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

/// Executables that are never the game.
///
/// Two kinds of rule, both from `../structure.md`: names that give the file away
/// (`unins000.exe`, `vcredist_x64.exe`, `UnityCrashHandler64.exe`) and folders
/// whose entire contents are somebody else's binaries shipped alongside the game.
pub fn is_rejected(relative: &Path) -> bool {
    const REJECTED_DIRS: [&str; 3] = ["redist", "_commonredist", "directx"];

    let name = relative
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if name.starts_with("unins")
        || name.starts_with("vcredist")
        || name.starts_with("directx")
        || name.starts_with("dxsetup")
        || name.starts_with("oalinst")
        || name.contains("setup")
        || name.contains("crashhandler")
        || name.contains("crashreport")
        || name.contains("uninstall")
    {
        return true;
    }

    let parts: Vec<String> = relative
        .parent()
        .into_iter()
        .flat_map(|p| p.components())
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();

    if parts.iter().any(|p| REJECTED_DIRS.contains(&p.as_str())) {
        return true;
    }
    // Unreal ships its third-party runtimes here, several of which are exes with
    // plausible names. Matched as a sequence rather than as three separate
    // folder names, which would reject far too much.
    parts
        .windows(3)
        .any(|w| w == ["engine", "binaries", "thirdparty"])
}

/// Ranks a surviving executable. Higher is likelier to be the game.
///
/// Shallow beats deep, a name matching the folder beats one that doesn't, and
/// size only breaks ties — in that order of importance, which is why the name
/// bonus is worth several depth levels and the size score is capped below one.
fn score(relative: &Path, folder_name: &str, bytes: u64) -> i64 {
    let depth = relative.components().count().saturating_sub(1) as i64;
    let mut score = -depth * DEPTH_PENALTY;

    let stem = relative
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let folder = folder_name.to_ascii_lowercase();

    if !stem.is_empty() && !folder.is_empty() {
        if squash(&stem) == squash(&folder) {
            score += EXACT_NAME_BONUS;
        } else if squash(&folder).contains(&squash(&stem)) || squash(&stem).contains(&squash(&folder))
        {
            score += PARTIAL_NAME_BONUS;
        }
    }

    // Megabytes, capped. A launcher shim and the real binary are usually orders
    // of magnitude apart, and this separates them without letting a big blob win
    // on size alone.
    score + ((bytes / (1024 * 1024)) as i64).min(MAX_SIZE_SCORE)
}

/// Folder and file names for the same game rarely agree on punctuation —
/// `Hollow Knight` / `hollow_knight.exe` / `HollowKnight.exe` are one game. Only
/// alphanumerics survive the comparison.
fn squash(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// A name for the game, defaulting to the folder's own — editable afterwards.
///
/// Trailing version noise is left alone: guessing wrong about `Game v1.2` costs
/// the user an edit either way, and stripping it wrongly is the one that loses
/// information.
pub fn default_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| folder.display().to_string())
}

/// Names already used on the cartridge, for the duplicate check the games screen
/// runs before accepting a folder.
pub fn taken_slugs(entries: &[crate::catalog::Entry]) -> HashSet<String> {
    entries
        .iter()
        .filter_map(|e| {
            Path::new(&e.exe)
                .components()
                .nth(1)
                .map(|c| c.as_os_str().to_string_lossy().to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn rejected(path: &str) -> bool {
        is_rejected(Path::new(path))
    }

    #[test]
    fn throws_out_the_executables_that_are_never_the_game() {
        assert!(rejected("unins000.exe"));
        assert!(rejected("Uninstall.exe"));
        assert!(rejected("setup.exe"));
        assert!(rejected("game_setup_helper.exe"));
        assert!(rejected("vcredist_x64.exe"));
        assert!(rejected("DXSETUP.exe"));
        assert!(rejected("directx_jun2010_redist.exe"));
        assert!(rejected("UnityCrashHandler64.exe"));
        assert!(rejected("_CommonRedist/vc/2019/vc.exe"));
        assert!(rejected("redist/anything.exe"));
        assert!(rejected("Engine/Binaries/ThirdParty/Steam/steam.exe"));
    }

    #[test]
    fn keeps_ordinary_game_executables() {
        assert!(!rejected("bg3.exe"));
        assert!(!rejected("bin/Game-Win64-Shipping.exe"));
        assert!(!rejected("Hollow Knight.exe"));
        // "engine/binaries" alone is where Unreal games actually put their
        // shipping binary — only the ThirdParty run underneath it is rejected.
        assert!(!rejected("Engine/Binaries/Win64/Game.exe"));
    }

    /// Builds a fake game folder: `(relative path, bytes)` pairs.
    fn fake_game(name: &str, files: &[(&str, u64)]) -> PathBuf {
        let dir = std::env::temp_dir().join("gacasy-detect").join(name);
        let _ = fs::remove_dir_all(&dir);
        for (path, bytes) in files {
            let full = dir.join(path);
            fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
            fs::write(&full, vec![0u8; *bytes as usize]).expect("write");
        }
        dir
    }

    fn scan_of(dir: &Path) -> Scan {
        scan(dir, &AtomicBool::new(false))
    }

    #[test]
    fn preselects_the_exe_named_after_its_folder() {
        let dir = fake_game(
            "hollow_knight",
            &[
                ("hollow_knight.exe", 40 * 1024),
                ("unins000.exe", 900 * 1024),
                ("bin/tools/editor.exe", 5 * 1024 * 1024),
            ],
        );
        let scan = scan_of(&dir);
        let winner = scan.clear_winner().expect("a clear winner");
        assert_eq!(winner.relative, PathBuf::from("hollow_knight.exe"));
        // The uninstaller never even became a candidate.
        assert!(!scan.candidates.iter().any(|c| c.display().contains("unins")));
    }

    #[test]
    fn two_equally_plausible_executables_are_left_to_the_user() {
        let dir = fake_game(
            "Some Game",
            &[("play.exe", 60 * 1024), ("launch.exe", 60 * 1024)],
        );
        let scan = scan_of(&dir);
        assert_eq!(scan.candidates.len(), 2);
        assert!(
            scan.clear_winner().is_none(),
            "a coin flip must not be presented as a decision"
        );
    }

    #[test]
    fn a_folder_with_nothing_left_after_the_reject_list_has_no_winner() {
        let dir = fake_game("Empty", &[("unins000.exe", 900 * 1024), ("readme.txt", 10)]);
        let scan = scan_of(&dir);
        assert!(scan.candidates.is_empty());
        assert!(scan.clear_winner().is_none());
        // The walk still measured the folder — that is what the copy needs.
        assert_eq!(scan.file_count, 2);
        assert_eq!(scan.total_bytes, 900 * 1024 + 10);
    }

    #[test]
    fn a_shallow_exe_beats_a_buried_one_of_the_same_name() {
        let dir = fake_game(
            "bg3",
            &[
                ("bg3.exe", 20 * 1024),
                ("bin/dx11/bg3.exe", 80 * 1024 * 1024),
            ],
        );
        let scan = scan_of(&dir);
        let winner = scan.clear_winner().expect("a clear winner");
        assert_eq!(winner.relative, PathBuf::from("bg3.exe"));
    }

    #[test]
    fn size_only_breaks_ties() {
        // Same depth, neither matches the folder name: the bigger one wins, but
        // not by enough to count as clear.
        let dir = fake_game(
            "Untitled",
            &[("a.exe", 20 * 1024), ("b.exe", 200 * 1024 * 1024)],
        );
        let scan = scan_of(&dir);
        assert_eq!(scan.candidates[0].relative, PathBuf::from("b.exe"));
        assert!(scan.clear_winner().is_none());
    }

    #[test]
    fn measures_every_file_not_just_executables() {
        let dir = fake_game(
            "Sized",
            &[("game.exe", 100 * 1024), ("data/pak0.bin", 3 * 1024 * 1024)],
        );
        let scan = scan_of(&dir);
        assert_eq!(scan.total_bytes, 100 * 1024 + 3 * 1024 * 1024);
        assert_eq!(scan.file_count, 2);
    }

    #[test]
    fn a_cancelled_scan_says_so() {
        let dir = fake_game("Cancelled", &[("game.exe", 100 * 1024)]);
        let scan = scan(&dir, &AtomicBool::new(true));
        assert!(scan.cancelled);
        assert!(scan.candidates.is_empty());
    }
}
