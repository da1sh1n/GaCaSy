// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Wizard state: which screen, and everything the screens read and write.
//!
//! The routing rule from `../structure.md` lives in [`App::choose_volume`] and
//! nowhere else: a volume that already carries a launcher goes to edit, one
//! without goes to create. Both then use the same games screen and the same
//! write, because they *are* the same job with a different starting catalog.
//!
//! There is no key screen and no pairing step. A cartridge's identity is the
//! signature inside the `launcher.exe` this installer carries, so creating one
//! is just writing files — there is nothing for the user to choose, copy down,
//! or keep in step between two machines.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use eframe::egui;

use crate::cartridge::{self, Plan, PlannedGame};
use crate::catalog::{self, Entry};
use crate::detect;
use crate::image;
use crate::listener;
use crate::volume::{self, Volume};
use crate::work::{Job, Scanning};

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Screen {
    Home,
    /// Pick the volume; routes to create or edit.
    Volume,
    Games,
    Review,
    /// A job is running. The only screen with no way back.
    Working,
    Done,
    /// Job 2, reachable from Home and independent of the cartridge flow.
    Listener,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Mode {
    Create,
    Edit,
}

/// One game on its way onto the cartridge.
pub struct Draft {
    pub source: PathBuf,
    pub name: String,
    /// Running until the folder walk finishes.
    pub scanning: Option<Scanning>,
    pub scan: Option<detect::Scan>,
    /// Index into `scan.candidates`, or `None` when the exe came from Browse.
    pub selected: Option<usize>,
    /// Set only by the manual override; relative to `source`.
    pub manual_exe: Option<PathBuf>,
    pub image: Option<PathBuf>,
    /// The 2:3 note, if the chosen cover isn't that shape.
    pub image_warning: Option<String>,
}

impl Draft {
    fn new(ctx: &egui::Context, source: PathBuf) -> Draft {
        Draft {
            name: detect::default_name(&source),
            scanning: Some(Scanning::start(ctx, source.clone())),
            source,
            scan: None,
            selected: None,
            manual_exe: None,
            image: None,
            image_warning: None,
        }
    }

    /// Picks up the finished scan and preselects a clear winner.
    pub fn poll(&mut self) {
        let Some(scanning) = &self.scanning else {
            return;
        };
        let Some(scan) = scanning.take() else { return };
        self.scanning = None;
        // Only a *clear* winner is preselected. When the top two are too close
        // to call, the field is left empty and the user has to choose — a guess
        // presented as a decision is worse than no guess.
        self.selected = scan
            .clear_winner()
            .and_then(|winner| scan.candidates.iter().position(|c| c == winner));
        self.scan = Some(scan);
    }

    pub fn scanning(&self) -> bool {
        self.scanning.is_some()
    }

    /// The chosen executable, relative to the game folder.
    pub fn exe_relative(&self) -> Option<PathBuf> {
        if let Some(manual) = &self.manual_exe {
            return Some(manual.clone());
        }
        let scan = self.scan.as_ref()?;
        Some(scan.candidates.get(self.selected?)?.relative.clone())
    }

    /// Accepts a hand-picked exe, which must be inside the game folder — the
    /// copy only moves that folder, so an exe from anywhere else would be a
    /// catalog entry pointing at a file that never shipped.
    pub fn set_manual_exe(&mut self, chosen: &Path) -> Result<(), String> {
        let relative = chosen
            .strip_prefix(&self.source)
            .map_err(|_| format!("Pick an executable inside {}.", self.source.display()))?;
        self.manual_exe = Some(relative.to_path_buf());
        self.selected = None;
        Ok(())
    }

    pub fn set_image(&mut self, chosen: PathBuf) {
        self.image_warning = image::ratio_warning(&chosen);
        self.image = Some(chosen);
    }

    /// What still has to be filled in, in one sentence, or `None` when ready.
    pub fn blocker(&self) -> Option<String> {
        if self.scanning() {
            return Some("still being read".into());
        }
        if self.name.trim().is_empty() {
            return Some("needs a name".into());
        }
        if self.exe_relative().is_none() {
            return Some(match self.scan.as_ref().map(|s| s.candidates.is_empty()) {
                Some(true) => "no executable was found — pick one".into(),
                _ => "needs an executable".into(),
            });
        }
        if self.image.is_none() {
            return Some("needs a cover image".into());
        }
        None
    }

    pub fn bytes(&self) -> u64 {
        self.scan.as_ref().map(|s| s.total_bytes).unwrap_or(0)
    }
}

pub struct App {
    pub screen: Screen,
    pub mode: Mode,

    pub volumes: Vec<Volume>,
    pub target: Option<usize>,

    /// Edit mode: what is already on the cartridge, and which of it to delete.
    pub existing: Vec<Entry>,
    pub remove: Vec<bool>,

    pub drafts: Vec<Draft>,
    pub job: Option<Job>,
    pub outcome: Option<Result<Vec<String>, String>>,

    /// Shown at the top of whatever screen is up, until dismissed.
    pub error: Option<String>,

    // ── Job 2 ────────────────────────────────────────────────────────────
    pub listener_installs: Vec<listener::Installed>,
    pub listener_start_now: bool,
}

impl App {
    pub fn new() -> App {
        App {
            screen: Screen::Home,
            mode: Mode::Create,
            volumes: volume::list(),
            target: None,
            existing: Vec::new(),
            remove: Vec::new(),
            drafts: Vec::new(),
            job: None,
            outcome: None,
            error: None,
            listener_installs: listener::find(),
            listener_start_now: true,
        }
    }

    pub fn refresh_volumes(&mut self) {
        let previous = self.volume().map(|v| v.root.clone());
        self.volumes = volume::list();
        self.target = previous.and_then(|root| self.volumes.iter().position(|v| v.root == root));
    }

    pub fn refresh_listeners(&mut self) {
        self.listener_installs = listener::find();
    }

    pub fn volume(&self) -> Option<&Volume> {
        self.target.and_then(|i| self.volumes.get(i))
    }

    /// The routing decision, and the only place it is made.
    pub fn choose_volume(&mut self, index: usize) {
        self.target = Some(index);
        self.drafts.clear();
        self.error = None;

        let Some(volume) = self.volumes.get(index) else {
            return;
        };

        // The picker doesn't offer a refused drive, so reaching here means the
        // list went stale under a click — a drive unplugged and its letter
        // reused, most plausibly. Checked again rather than trusted, because the
        // thing on the other side of this is a multi-gigabyte write.
        if !volume.allowed() {
            self.error = Some(format!(
                "{} cannot be used: {}",
                volume.root.display(),
                volume.eligibility.reason()
            ));
            self.target = None;
            return;
        }
        let root = volume.root.clone();

        if volume.is_cartridge {
            self.mode = Mode::Edit;
            match catalog::read(&root) {
                Ok(entries) => {
                    self.remove = vec![false; entries.len()];
                    self.existing = entries;
                }
                Err(e) => {
                    // Refusing here is the point: writing a new catalog over one
                    // we couldn't parse would silently drop games that are on
                    // the cartridge right now.
                    self.error = Some(format!("{e}\n\nFix or delete that file and try again."));
                    self.target = None;
                    return;
                }
            }
            self.screen = Screen::Games;
        } else {
            self.mode = Mode::Create;
            self.existing.clear();
            self.remove.clear();
            // Straight to the games screen. Creating a cartridge used to stop
            // here to choose a key; there is nothing left to ask.
            self.screen = Screen::Games;
        }
    }

    pub fn add_game(&mut self, ctx: &egui::Context, folder: PathBuf) {
        if self.drafts.iter().any(|d| d.source == folder) {
            self.error = Some(format!(
                "{} is already in this list.",
                folder.file_name().unwrap_or_default().to_string_lossy()
            ));
            return;
        }
        // The other half of the duplicate rule: a folder whose name matches a
        // game already on the cartridge is refused rather than renamed or
        // merged. Renaming produces two entries the user can't tell apart;
        // overwriting destroys an install that may be many gigabytes and may be
        // the only copy. Refusing costs one click — remove the old one first.
        let slug = catalog::slug(&detect::default_name(&folder));
        if cartridge::taken_slugs(&self.kept_entries()).contains(&slug) {
            self.error = Some(format!(
                "This cartridge already has a game in games/{slug}. \
                 Remove it below first, or rename the folder you are adding."
            ));
            return;
        }
        self.drafts.push(Draft::new(ctx, folder));
    }

    /// Catalog entries that survive this edit.
    pub fn kept_entries(&self) -> Vec<Entry> {
        self.existing
            .iter()
            .zip(self.remove.iter().chain(std::iter::repeat(&false)))
            .filter(|(_, remove)| !**remove)
            .map(|(entry, _)| entry.clone())
            .collect()
    }

    pub fn removed_entries(&self) -> Vec<Entry> {
        self.existing
            .iter()
            .zip(self.remove.iter().chain(std::iter::repeat(&false)))
            .filter(|(_, remove)| **remove)
            .map(|(entry, _)| entry.clone())
            .collect()
    }

    /// Builds the plan, or says what is stopping it.
    pub fn plan(&self) -> Result<Plan, String> {
        let volume = self.volume().ok_or("No volume is selected.")?;

        // The last gate before anything is written, and the reason it is here
        // rather than only in the picker: `plan()` is what the Review screen
        // shows and what the Write button consumes, so a selection that became
        // invalid while the user was adding games — an external drive unplugged,
        // its letter picked up by something internal — cannot get past it.
        if !volume.allowed() {
            return Err(format!(
                "{} cannot be used: {}",
                volume.root.display(),
                volume.eligibility.reason()
            ));
        }
        for draft in &self.drafts {
            if let Some(blocker) = draft.blocker() {
                return Err(format!("{} {blocker}.", draft.name));
            }
        }

        let keep = self.kept_entries();
        let mut taken: HashSet<String> = cartridge::taken_slugs(&keep);
        let add = self
            .drafts
            .iter()
            .map(|draft| PlannedGame {
                slug: catalog::unique_slug(&draft.name, &mut taken),
                source: draft.source.clone(),
                name: draft.name.trim().to_string(),
                exe_relative: draft.exe_relative().expect("checked by blocker above"),
                image: draft.image.clone().expect("checked by blocker above"),
                bytes: draft.bytes(),
            })
            .collect();

        Ok(Plan {
            root: volume.root.clone(),
            keep,
            remove: self.removed_entries(),
            add,
        })
    }

    /// Free space, minus what the plan needs — negative when it won't fit.
    pub fn space_shortfall(&self, plan: &Plan) -> Option<u64> {
        let free = self.volume()?.free_bytes;
        let needed = plan.required_bytes();
        (needed > free).then(|| needed - free)
    }

    pub fn start(&mut self, ctx: &egui::Context, plan: Plan) {
        let games = plan.add.len();
        let removed = plan.remove.len();
        self.job = Some(Job::spawn(ctx, "Writing the cartridge", move |cancel, report| {
            cartridge::apply(&plan, cancel, report).map(|()| {
                let mut done = Vec::new();
                if games > 0 {
                    done.push(format!("Copied {games} game(s) onto the cartridge"));
                }
                if removed > 0 {
                    done.push(format!("Removed {removed} game(s)"));
                }
                done.push("Wrote launcher.exe and catalog.json".into());
                done.push("Plug it into a PC running the listener to try it".into());
                done
            })
        }));
        self.screen = Screen::Working;
    }

    pub fn install_listener(&mut self, ctx: &egui::Context) {
        let start_now = self.listener_start_now;
        self.start_listener_job(ctx, "Installing the listener", move || {
            listener::install(start_now)
        });
    }

    /// Removes the listener at `dir` — which is the one in
    /// `listener::install_dir()`, or a folder an earlier build used.
    pub fn uninstall_listener(&mut self, ctx: &egui::Context, dir: PathBuf) {
        self.start_listener_job(ctx, "Removing the listener", move || {
            listener::uninstall(&dir)
        });
    }

    /// Both job-2 operations are over in well under a second but still go
    /// through the worker, so the one Working/Done pair of screens reports every
    /// outcome in the program the same way.
    fn start_listener_job<F>(&mut self, ctx: &egui::Context, title: &str, task: F)
    where
        F: FnOnce() -> Result<Vec<String>, String> + Send + 'static,
    {
        self.job = Some(
            Job::spawn(ctx, title, move |_cancel, report| {
                report(cartridge::Progress {
                    done: 0,
                    total: 1,
                    label: "Working…".into(),
                });
                task()
            })
            .uncancellable(),
        );
        self.screen = Screen::Working;
    }

    /// Moves a finished job onto the Done screen.
    pub fn poll_job(&mut self) {
        let Some(job) = &mut self.job else { return };
        job.poll();
        if job.finished() {
            let job = self.job.take().expect("just checked");
            self.outcome = job.outcome;
            self.screen = Screen::Done;
            self.refresh_volumes();
            self.refresh_listeners();
        }
    }

    /// Back to the start, keeping nothing but the volume list.
    pub fn reset(&mut self) {
        *self = App::new();
    }
}
