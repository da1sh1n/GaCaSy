// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The wizard: a header, one screen, a footer of actions.
//!
//! Every screen is a function of `&mut App` and an `egui::Ui`, and the footer is
//! the only thing that moves between them. That shape is deliberate — the back
//! button, the primary action and the "what is stopping this" text all live in
//! one place, so no screen can quietly grow a different way forward.
//!
//! `Screen::Working` is the one screen with no way back, because the thing it is
//! showing is a copy that is already underway. It offers cancel instead.

mod games;
mod listener;

use crate::app::{App, Mode, Screen};
use crate::cartridge;
use crate::payload;
use crate::volume::human_bytes;

/// Warnings and blockers. Read against both the light and dark egui themes.
pub const WARN: egui::Color32 = egui::Color32::from_rgb(0xe0, 0xb1, 0x3a);
pub const BAD: egui::Color32 = egui::Color32::from_rgb(0xd1, 0x3a, 0x3a);
pub const GOOD: egui::Color32 = egui::Color32::from_rgb(0x5c, 0xb8, 0x5c);

/// Look and feel, applied once before the first frame.
///
/// A shade larger than egui's default. This is a wizard read once by someone who
/// has never seen it, not a tool used daily.
pub fn configure(ctx: &egui::Context) {
    ctx.set_fonts(crate::font::definitions());
    ctx.all_styles_mut(|style| {
        for (_, font) in style.text_styles.iter_mut() {
            font.size *= 1.15;
        }
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    });
}

impl App {
    /// One frame. Called by [`crate::shell`] with the whole window to draw into.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll_job();
        for draft in &mut self.drafts {
            draft.poll();
        }

        egui::Panel::top("header").show(ui, |ui| header(self, ui));
        egui::Panel::bottom("footer").show(ui, |ui| footer(self, &ctx, ui));
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    match self.screen {
                        Screen::Home => home(self, ui),
                        Screen::Volume => volume_screen(self, ui),
                        Screen::Games => games::screen(self, &ctx, ui),
                        Screen::Review => review(self, ui),
                        Screen::Working => working(self, ui),
                        Screen::Done => done(self, ui),
                        Screen::Listener => listener::screen(self, ui),
                    }
                });
        });
    }
}

fn header(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.heading(title(app));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(volume) = app.volume()
                && !matches!(app.screen, Screen::Home | Screen::Listener)
            {
                ui.label(egui::RichText::new(volume.summary()).weak());
            }
        });
    });
    if let Some(subtitle) = subtitle(app) {
        ui.label(egui::RichText::new(subtitle).weak());
    }
    ui.add_space(6.0);

    // A payload-less build says so before the user picks anything, rather than
    // failing at the moment it would have written a file.
    if let Some(defect) = payload::defect() {
        ui.colored_label(BAD, defect);
        ui.add_space(4.0);
    }
    if let Some(error) = app.error.clone() {
        ui.horizontal_top(|ui| {
            ui.colored_label(BAD, error);
            if ui.small_button("Dismiss").clicked() {
                app.error = None;
            }
        });
        ui.add_space(4.0);
    }
}

fn title(app: &App) -> &'static str {
    match app.screen {
        Screen::Home => "GaCaSy",
        Screen::Volume => "Choose the drive",
        Screen::Games => match app.mode {
            Mode::Create => "Add games",
            Mode::Edit => "Edit this cartridge",
        },
        Screen::Review => "Review",
        Screen::Working => "Working",
        Screen::Done => "Done",
        Screen::Listener => "The PC listener",
    }
}

fn subtitle(app: &App) -> Option<&'static str> {
    Some(match app.screen {
        Screen::Home => "Make a game cartridge, or set this PC up to notice one.",
        Screen::Volume => {
            "External drives only. One that is already a cartridge opens for editing instead."
        }
        Screen::Games => "Each game needs an executable and a cover image.",
        Screen::Review => "Nothing has been written yet.",
        Screen::Working => return None,
        Screen::Done => return None,
        Screen::Listener => "It watches for cartridges and starts the one it recognises.",
    })
}

/// The one set of actions in the program. Every screen's way forward is here.
fn footer(app: &mut App, ctx: &egui::Context, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if let Some(back) = back_target(app)
            && ui.button("← Back").clicked()
        {
            app.error = None;
            app.screen = back;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match app.screen {
                Screen::Volume => {
                    if ui.button("Rescan drives").clicked() {
                        app.refresh_volumes();
                    }
                }
                Screen::Games => match app.plan() {
                    Ok(plan) if plan.is_empty() => {
                        ui.add_enabled(false, egui::Button::new("Review"));
                        ui.label(egui::RichText::new("Nothing to do yet.").weak());
                    }
                    Ok(_) => {
                        if ui.button("Review").clicked() {
                            app.screen = Screen::Review;
                        }
                    }
                    Err(problem) => {
                        ui.add_enabled(false, egui::Button::new("Review"));
                        ui.colored_label(WARN, problem);
                    }
                },
                Screen::Review => {
                    let plan = app.plan();
                    let blocked = plan.is_err() || payload::defect().is_some();
                    if ui
                        .add_enabled(!blocked, egui::Button::new("Write the cartridge"))
                        .clicked()
                        && let Ok(plan) = plan
                    {
                        app.start(ctx, plan);
                    }
                }
                Screen::Working => {
                    let Some(job) = &app.job else { return };
                    if job.cancellable {
                        let cancelling = job.cancelling();
                        if ui
                            .add_enabled(!cancelling, egui::Button::new("Cancel"))
                            .clicked()
                        {
                            job.request_cancel();
                        }
                        if cancelling {
                            ui.label(egui::RichText::new("Stopping…").weak());
                        }
                    }
                }
                Screen::Done => {
                    if ui.button("Finish").clicked() {
                        app.reset();
                    }
                }
                Screen::Home | Screen::Listener => {}
            }
        });
    });
    ui.add_space(6.0);
}

fn back_target(app: &App) -> Option<Screen> {
    Some(match app.screen {
        // Nowhere to go back to, or nowhere it would be safe to.
        Screen::Home | Screen::Working | Screen::Done => return None,
        Screen::Volume | Screen::Listener => Screen::Home,
        // Both modes go back to the drive picker now. Create used to land on the
        // key screen on its way through.
        Screen::Games => Screen::Volume,
        Screen::Review => Screen::Games,
    })
}

// ── Screens ──────────────────────────────────────────────────────────────

fn home(app: &mut App, ui: &mut egui::Ui) {
    ui.label(
        "A cartridge is an ordinary drive with a marker file on it. Plug one into a PC \
         running the listener and its games come up on their own.",
    );
    ui.add_space(16.0);

    ui.horizontal(|ui| {
        if ui
            .add_sized([260.0, 40.0], egui::Button::new("Make or edit a cartridge"))
            .clicked()
        {
            app.refresh_volumes();
            app.screen = Screen::Volume;
        }
        ui.label("Write the launcher, your games and their covers onto a drive.");
    });
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        if ui
            .add_sized([260.0, 40.0], egui::Button::new("Set up this PC"))
            .clicked()
        {
            app.refresh_listeners();
            app.screen = Screen::Listener;
        }
        ui.vertical(|ui| {
            ui.label("Install the listener, so plugging a cartridge in starts it.");
            match app.listener_installs.len() {
                0 => ui.colored_label(WARN, "Not installed on this PC yet."),
                _ => ui.colored_label(
                    GOOD,
                    format!(
                        "Installed: {}",
                        app.listener_installs
                            .iter()
                            .map(|i| i.dir.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ),
            };
        });
    });
}

/// The drive picker. **Every row here can be picked** — `volume::list()` has
/// already dropped the drive Windows is on and every internal disk.
///
/// They used to be listed, greyed out, on the grounds that a filter which
/// silently shortens a list reads as a bug. The line below does that job
/// instead: it says once why a drive might be missing, rather than filling the
/// screen with `C:` and every internal disk as rows that exist to say no.
fn volume_screen(app: &mut App, ui: &mut egui::Ui) {
    if app.volumes.is_empty() {
        ui.colored_label(
            WARN,
            "No external drive found. Plug in a USB stick or drive, then press Rescan drives.",
        );
    }
    ui.label(
        egui::RichText::new(
            "A cartridge has to be something you can unplug and carry, so internal drives \
             and the drive Windows is on are not listed.",
        )
        .weak(),
    );
    ui.add_space(12.0);

    let mut chosen = None;
    for (index, volume) in app.volumes.iter().enumerate() {
        ui.horizontal(|ui| {
            let button = egui::Button::new(volume.root.display().to_string());
            if ui.add_sized([120.0, 28.0], button).clicked() {
                chosen = Some(index);
            }
            ui.vertical(|ui| {
                ui.label(volume.summary());
                if volume.is_cartridge {
                    ui.colored_label(GOOD, "Already a cartridge — opens for editing.");
                } else {
                    ui.label(egui::RichText::new(format!("{} drive.", volume.bus)).weak());
                }
            });
        });
        ui.add_space(4.0);
    }
    if let Some(index) = chosen {
        app.choose_volume(index);
    }
}

fn review(app: &mut App, ui: &mut egui::Ui) {
    let plan = match app.plan() {
        Ok(plan) => plan,
        Err(problem) => {
            ui.colored_label(BAD, problem);
            return;
        }
    };

    ui.label(egui::RichText::new("On this drive").strong());
    ui.label(app.volume().map(|v| v.summary()).unwrap_or_default());
    ui.add_space(10.0);

    if let Some(new_name) = &plan.label {
        let was = app.volume().map(|v| v.label.as_str()).unwrap_or_default();
        ui.label(egui::RichText::new("Rename").strong());
        ui.label(format!("  {} → {}", quoted(was), quoted(new_name)));
        ui.add_space(10.0);
    }

    if !plan.remove.is_empty() {
        ui.label(egui::RichText::new("Remove").strong());
        for entry in &plan.remove {
            ui.label(format!("  − {} ({})", entry.name, entry.exe));
        }
        ui.add_space(10.0);
    }

    if !plan.add.is_empty() {
        ui.label(egui::RichText::new("Copy on").strong());
        for game in &plan.add {
            ui.label(format!(
                "  + {} — {} → games/{}/",
                game.name,
                human_bytes(game.bytes),
                game.slug
            ));
        }
        ui.add_space(10.0);
    }

    if !plan.keep.is_empty() {
        ui.label(egui::RichText::new("Already there, untouched").strong());
        for entry in &plan.keep {
            ui.label(format!("  · {}", entry.name));
        }
        ui.add_space(10.0);
    }

    ui.label(egui::RichText::new("Also written").strong());
    ui.label("  launcher.exe, catalog.json");
    ui.label(
        egui::RichText::new(
            "  the launcher carries the signature that makes this a cartridge — \
             there is nothing else to write and nothing to pair",
        )
        .weak(),
    );
    ui.add_space(12.0);

    // The free-space check is a precheck, not a guarantee — see
    // cartridge::FREE_SPACE_SLACK. A mid-copy failure is still handled, and
    // rolls the cartridge back.
    let free = app.volume().map(|v| v.free_bytes).unwrap_or(0);
    ui.label(format!(
        "{} to copy, {} free on the drive.",
        human_bytes(plan.bytes_to_copy()),
        human_bytes(free)
    ));
    match app.space_shortfall(&plan) {
        Some(short) => ui.colored_label(
            BAD,
            format!(
                "That is {} short, counting {} of headroom. Remove a game or use a bigger drive.",
                human_bytes(short),
                human_bytes(cartridge::FREE_SPACE_SLACK)
            ),
        ),
        None => ui.colored_label(GOOD, "It fits."),
    };
}

/// A drive name for the rename line — quoted, so trailing spaces are visible,
/// and named rather than shown as `""` when there isn't one.
fn quoted(name: &str) -> String {
    if name.is_empty() {
        "no name".into()
    } else {
        format!("\"{name}\"")
    }
}

fn working(app: &mut App, ui: &mut egui::Ui) {
    let Some(job) = &app.job else { return };
    ui.label(egui::RichText::new(&job.title).strong());
    ui.add_space(8.0);

    let bar = match job.fraction {
        Some(fraction) => egui::ProgressBar::new(fraction).show_percentage(),
        None => egui::ProgressBar::new(0.0).animate(true),
    };
    ui.add(bar.desired_width(ui.available_width().min(640.0)));
    ui.add_space(8.0);
    ui.label(egui::RichText::new(&job.label).weak());

    if job.cancellable {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(
                "Cancelling removes what this run has copied so far. Games that were already \
                 on the cartridge are left alone.",
            )
            .weak(),
        );
    }
}

fn done(app: &mut App, ui: &mut egui::Ui) {
    match &app.outcome {
        Some(Ok(lines)) => {
            ui.colored_label(GOOD, egui::RichText::new("Finished.").strong());
            ui.add_space(8.0);
            for line in lines {
                ui.label(format!("• {line}"));
            }
        }
        Some(Err(problem)) if problem == "cancelled" => {
            ui.colored_label(WARN, egui::RichText::new("Cancelled.").strong());
            ui.add_space(8.0);
            ui.label("Nothing this run copied was left behind. The cartridge is as it was.");
        }
        Some(Err(problem)) => {
            ui.colored_label(BAD, egui::RichText::new("It didn't work.").strong());
            ui.add_space(8.0);
            ui.label(problem);
        }
        None => {
            ui.label("Nothing to report.");
        }
    }
}
