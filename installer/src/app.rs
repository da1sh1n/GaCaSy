// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Holds the wizard's state: the current screen, the chosen volume, the games
//! being added or removed, and the running job. `chooseVolume` is where the
//! create-vs-edit routing decision is made.

// ########## WIZARD STATE ##########

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::autoplay;
use crate::cartridge::{self, Plan, PlannedGame};
use crate::catalog::{self, Entry};
use crate::copy;
use crate::detect;
use crate::image;
use crate::listener;
use crate::payload;
use crate::version::{self, Version};
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
            name: detect::defaultName(&source),
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
            .clearWinner()
            .and_then(|winner| scan.candidates.iter().position(|c| c == winner));
        self.scan = Some(scan);
    }

    pub fn scanning(&self) -> bool {
        self.scanning.is_some()
    }

    /// The chosen executable, relative to the game folder.
    pub fn exeRelative(&self) -> Option<PathBuf> {
        if let Some(manual) = &self.manual_exe {
            return Some(manual.clone());
        }
        let scan = self.scan.as_ref()?;
        Some(scan.candidates.get(self.selected?)?.relative.clone())
    }

    /// Accepts a hand-picked exe, which must be inside the game folder — the
    /// copy only moves that folder, so an exe from anywhere else would be a
    /// catalog entry pointing at a file that never shipped.
    pub fn setManualExe(&mut self, chosen: &Path) -> Result<(), String> {
        let relative = chosen
            .strip_prefix(&self.source)
            .map_err(|_| format!("Pick an executable inside {}.", self.source.display()))?;
        self.manual_exe = Some(relative.to_path_buf());
        self.selected = None;
        Ok(())
    }

    pub fn setImage(&mut self, chosen: PathBuf) {
        self.image_warning = image::ratioWarning(&chosen);
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
        if self.exeRelative().is_none() {
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

    /// What the cartridge should be called — the drive's volume label. Seeded
    /// from the drive's current name when one is picked, so leaving it alone
    /// means leaving the name alone.
    pub name: String,

    /// Edit mode: what is already on the cartridge, and which of it to delete.
    pub existing: Vec<Entry>,
    pub remove: Vec<bool>,

    /// Edit mode: set when the cartridge's launcher states a version other than
    /// `version::bundled()`. `None` covers "not a cartridge", "matches" and
    /// "carries no readable version" alike — none is a reason to offer an
    /// update. Read off the verified signature when the volume was picked, so
    /// there is no pending state and nothing to poll.
    pub staleLauncher: Option<Version>,

    pub drafts: Vec<Draft>,
    pub job: Option<Job>,
    pub outcome: Option<Result<Vec<String>, String>>,

    /// Shown at the top of whatever screen is up, until dismissed.
    pub error: Option<String>,

    // ── Job 2 ────────────────────────────────────────────────────────────
    pub listener_installs: Vec<listener::Installed>,
    pub listener_start_now: bool,
    /// Whether installing should also stop Windows opening a folder when a
    /// drive arrives. Opt-in because it is a user-wide setting — see
    /// [`crate::autoplay`].
    pub suppress_autoplay: bool,
}

impl App {
    pub fn new() -> App {
        App {
            screen: Screen::Home,
            mode: Mode::Create,
            volumes: volume::list(),
            target: None,
            name: String::new(),
            existing: Vec::new(),
            remove: Vec::new(),
            staleLauncher: None,
            drafts: Vec::new(),
            job: None,
            outcome: None,
            error: None,
            listener_installs: listener::find(),
            listener_start_now: true,
            // Ticked unless it has already been done, so the common case is one
            // less decision and a PC that is already set up is not offered a
            // change that would do nothing.
            suppress_autoplay: !autoplay::suppressed(),
        }
    }

    pub fn refreshVolumes(&mut self) {
        let previous = self.volume().map(|v| v.root.clone());
        self.volumes = volume::list();
        self.target = previous.and_then(|root| self.volumes.iter().position(|v| v.root == root));
    }

    pub fn refreshListeners(&mut self) {
        self.listener_installs = listener::find();
    }

    pub fn volume(&self) -> Option<&Volume> {
        self.target.and_then(|i| self.volumes.get(i))
    }

    /// Picks `volume` and routes to edit or create, the only place that
    /// decision is made. Everything it needs is already known from the drive
    /// listing, so nothing here is asynchronous.
    pub fn chooseVolume(&mut self, index: usize) {
        self.target = Some(index);
        self.drafts.clear();
        self.error = None;
        self.staleLauncher = None;

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
        // Seeded from the drive rather than left blank: an empty field would
        // read as "this cartridge has no name" and clear a label the user never
        // meant to touch.
        self.name = volume.label.clone();

        if volume.is_cartridge {
            self.mode = Mode::Edit;
            match catalog::read(&root) {
                Ok(entries) => {
                    self.remove = vec![false; entries.len()];
                    self.existing = entries;
                    // Its signature already told us this when the drive was
                    // listed, so there is nothing to start and nothing to wait
                    // for. Any position differing counts — not just the major,
                    // which is all the listener cares about at runtime. This is
                    // "does the cartridge have the newest launcher this
                    // installer knows how to write", not "will these two
                    // programs still talk to each other".
                    self.staleLauncher = match (
                        volume.launcher_version.as_deref().and_then(version::parse),
                        version::bundled(),
                    ) {
                        (Some(theirs), Some(ours)) if theirs != ours => Some(theirs),
                        _ => None,
                    };
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

    pub fn addGame(&mut self, ctx: &egui::Context, folder: PathBuf) {
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
        let slug = catalog::slug(&detect::defaultName(&folder));
        if cartridge::takenSlugs(&self.keptEntries()).contains(&slug) {
            self.error = Some(format!(
                "This cartridge already has a game in games/{slug}. \
                 Remove it below first, or rename the folder you are adding."
            ));
            return;
        }
        self.drafts.push(Draft::new(ctx, folder));
    }

    /// Catalog entries that survive this edit.
    pub fn keptEntries(&self) -> Vec<Entry> {
        self.existing
            .iter()
            .zip(self.remove.iter().chain(std::iter::repeat(&false)))
            .filter(|(_, remove)| !**remove)
            .map(|(entry, _)| entry.clone())
            .collect()
    }

    pub fn removedEntries(&self) -> Vec<Entry> {
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

        // Checked here, where every other blocker is, so a name the drive can't
        // take stops the Review button rather than the last step of a copy that
        // has already run for minutes.
        let name = self.name.trim();
        volume::validateLabel(name, &volume.fs)?;

        let keep = self.keptEntries();
        let mut taken: HashSet<String> = cartridge::takenSlugs(&keep);
        let add = self
            .drafts
            .iter()
            .map(|draft| PlannedGame {
                slug: catalog::uniqueSlug(&draft.name, &mut taken),
                source: draft.source.clone(),
                name: draft.name.trim().to_string(),
                exeRelative: draft.exeRelative().expect("checked by blocker above"),
                image: draft.image.clone().expect("checked by blocker above"),
                bytes: draft.bytes(),
            })
            .collect();

        Ok(Plan {
            root: volume.root.clone(),
            keep,
            remove: self.removedEntries(),
            add,
            label: (name != volume.label).then(|| name.to_string()),
        })
    }

    /// Free space, minus what the plan needs — negative when it won't fit.
    pub fn spaceShortfall(&self, plan: &Plan) -> Option<u64> {
        let free = self.volume()?.free_bytes;
        let needed = plan.requiredBytes();
        (needed > free).then(|| needed - free)
    }

    pub fn start(&mut self, ctx: &egui::Context, plan: Plan) {
        let games = plan.add.len();
        let removed = plan.remove.len();
        let renamed = plan.label.clone();
        self.job = Some(Job::spawn(
            ctx,
            "Writing the cartridge",
            move |cancel, report| {
                cartridge::apply(&plan, cancel, report).map(|warning| {
                    let mut done = Vec::new();
                    if games > 0 {
                        done.push(format!("Copied {games} game(s) onto the cartridge"));
                    }
                    if removed > 0 {
                        done.push(format!("Removed {removed} game(s)"));
                    }
                    done.push("Wrote launcher.exe and catalog.json".into());
                    // The rename is reported whichever way it went: silently
                    // dropping a name the user typed is the one outcome they would
                    // not find out about until they looked at the drive.
                    match (&renamed, warning) {
                        (_, Some(problem)) => done.push(problem),
                        (Some(name), None) if name.is_empty() => {
                            done.push("Cleared the drive's name".into())
                        }
                        (Some(name), None) => done.push(format!("Named the drive {name}")),
                        (None, None) => {}
                    }
                    done.push("Plug it into a PC running the listener to try it".into());
                    done
                })
            },
        ));
        self.screen = Screen::Working;
    }

    pub fn installListener(&mut self, ctx: &egui::Context) {
        let start_now = self.listener_start_now;
        let suppress_autoplay = self.suppress_autoplay;
        self.startListenerJob(ctx, "Installing the listener", move || {
            listener::install(start_now, suppress_autoplay)
        });
    }

    /// Removes the listener at `dir` — which is the one in
    /// `listener::installDir()`, or a folder an earlier build used.
    pub fn uninstallListener(&mut self, ctx: &egui::Context, dir: PathBuf) {
        self.startListenerJob(ctx, "Removing the listener", move || {
            listener::uninstall(&dir)
        });
    }

    /// Both job-2 operations are over in well under a second but still go
    /// through the worker, so the one Working/Done pair of screens reports every
    /// outcome in the program the same way.
    fn startListenerJob<F>(&mut self, ctx: &egui::Context, title: &str, task: F)
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

    /// Rewrites just `launcher.exe` on the current cartridge, independent of
    /// the games plan. `cartridge::apply` also refreshes it, but only as part
    /// of a plan that changes something else — an empty plan cannot reach
    /// Review (`Plan::isEmpty`), so this is the only route for a cartridge
    /// whose games and name are already correct.
    pub fn updateLauncher(&mut self, ctx: &egui::Context) {
        let Some(root) = self.volume().map(|v| v.root.clone()) else {
            return;
        };
        self.startListenerJob(ctx, "Updating the launcher", move || {
            let bytes = payload::launcher()?;
            copy::bytes(&root.join(cartridge::LAUNCHER_NAME), &bytes).map_err(|e| e.message())?;
            Ok(vec!["Updated launcher.exe".into()])
        });
    }

    /// Moves a finished job onto the Done screen.
    pub fn pollJob(&mut self) {
        let Some(job) = &mut self.job else { return };
        job.poll();
        if job.finished() {
            let job = self.job.take().expect("just checked");
            self.outcome = job.outcome;
            self.screen = Screen::Done;
            self.refreshVolumes();
            self.refreshListeners();
        }
    }

    /// Back to the start, keeping nothing but the volume list.
    pub fn reset(&mut self) {
        *self = App::new();
    }
}
