// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The games screen — steps 3 to 4 of job 1, and all of job 3's editing.
//!
//! The cartridge's own name, then one list of what is already on it (edit mode
//! only) and one of what is being added. Every added game shows the same three
//! decisions: its name, which executable to start, and which cover to show.
//!
//! The executable row is the one that matters. Auto-detection preselects a
//! *clear* winner and leaves the field empty otherwise, and Browse is offered
//! either way — the guess is never presented as settled.

use crate::app::{App, Mode};
use crate::version;
use crate::volume::human_bytes;

use super::{BAD, GOOD, WARN};

pub fn screen(app: &mut App, ctx: &egui::Context, ui: &mut egui::Ui) {
    name(app, ui);
    ui.separator();

    if app.mode == Mode::Edit {
        stale_launcher(app, ctx, ui);
        existing(app, ui);
        ui.separator();
    }

    ui.horizontal(|ui| {
        if ui.button("Add a game folder…").clicked()
            && let Some(folder) = rfd::FileDialog::new()
                .set_title("Pick the folder the game is installed in")
                .pick_folder()
        {
            app.add_game(ctx, folder);
        }
        ui.label(egui::RichText::new("The whole folder is copied onto the cartridge.").weak());
    });
    ui.add_space(8.0);

    let mut drop = None;
    for index in 0..app.drafts.len() {
        if draft(app, index, ui) {
            drop = Some(index);
        }
        ui.add_space(6.0);
    }
    if let Some(index) = drop {
        app.drafts.remove(index);
    }

    if app.drafts.is_empty() && app.mode == Mode::Create {
        ui.add_space(12.0);
        ui.label(egui::RichText::new("No games added yet.").weak());
    }
}

/// What the cartridge is called — the drive's volume label, and the only part of
/// a cartridge that isn't a file on it.
///
/// Nothing is written here: the new name rides along in the plan and is applied
/// with the rest, so backing out of this screen leaves the drive alone. A name
/// the filesystem won't take is reported by the footer, like every other thing
/// standing between here and Review.
fn name(app: &mut App, ui: &mut egui::Ui) {
    let limit = app.volume().map(|v| v.max_label_len()).unwrap_or(32);
    ui.horizontal(|ui| {
        ui.label("Cartridge name:");
        ui.add(egui::TextEdit::singleline(&mut app.name).desired_width(300.0));
    });
    ui.label(
        egui::RichText::new(format!(
            "The drive's name — what Windows shows beside it in Explorer. Up to {limit} \
             characters; leave it empty for no name."
        ))
        .weak()
        .small(),
    );
    ui.add_space(8.0);
}

/// Shown when `App::poll_launcher_probe` found this cartridge's launcher.exe
/// answering a version other than the one this installer carries. There is
/// deliberately no way to reach Review over this alone — see `Plan::is_empty` —
/// so this is the only path to refreshing a launcher on a cartridge whose games
/// and name are otherwise fine.
fn stale_launcher(app: &mut App, ctx: &egui::Context, ui: &mut egui::Ui) {
    let Some(theirs) = app.stale_launcher else {
        return;
    };
    let ours = version::bundled()
        .expect("stale_launcher is only set when both the probe and the bundled version parsed");
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.colored_label(
            WARN,
            format!(
                "This cartridge's launcher is version {theirs}, this installer carries {ours}."
            ),
        );
        if ui.button("Update launcher").clicked() {
            app.update_launcher(ctx);
        }
    });
    ui.add_space(8.0);
}

/// What the cartridge already holds, with a checkbox per game.
fn existing(app: &mut App, ui: &mut egui::Ui) {
    if app.existing.is_empty() {
        ui.label(egui::RichText::new("This cartridge has no games on it yet.").weak());
        ui.add_space(8.0);
        return;
    }
    ui.label(egui::RichText::new("Already on this cartridge").strong());
    ui.add_space(4.0);

    // `remove` is kept the same length as `existing` here rather than at every
    // call site, so a catalog read that changed length can't panic the UI.
    app.remove.resize(app.existing.len(), false);

    for (index, entry) in app.existing.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut remove = app.remove[index];
            if ui.checkbox(&mut remove, "Remove").changed() {
                app.remove[index] = remove;
            }
            ui.vertical(|ui| {
                let name = if remove {
                    egui::RichText::new(&entry.name).strikethrough().color(BAD)
                } else {
                    egui::RichText::new(&entry.name)
                };
                ui.label(name);
                ui.label(egui::RichText::new(&entry.exe).weak().small());
            });
        });
    }
    if app.remove.iter().any(|r| *r) {
        ui.colored_label(
            WARN,
            "Removing deletes that game's folder and cover from the cartridge.",
        );
    }
    ui.add_space(8.0);
}

/// One game being added. Returns true when its Remove button was pressed.
fn draft(app: &mut App, index: usize, ui: &mut egui::Ui) -> bool {
    let mut dropped = false;
    let mut error = None;

    egui::Frame::group(ui.style()).show(ui, |ui| {
        let draft = &mut app.drafts[index];

        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(300.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Remove").clicked() {
                    dropped = true;
                }
            });
        });
        ui.label(
            egui::RichText::new(draft.source.display().to_string())
                .weak()
                .small(),
        );
        ui.add_space(6.0);

        if draft.scanning() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading the folder…");
            });
            return;
        }

        let Some(scan) = &draft.scan else {
            ui.colored_label(BAD, "That folder could not be read.");
            return;
        };
        let candidates = scan.candidates.clone();
        let total_bytes = scan.total_bytes;
        let file_count = scan.file_count;

        // ── Executable ──────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label("Starts:");
            let selected_text = draft
                .exe_relative()
                .map(|p| crate::catalog::to_relative_string(&p))
                .unwrap_or_else(|| "— choose one —".into());

            egui::ComboBox::from_id_salt(("exe", index))
                .selected_text(selected_text)
                .width(380.0)
                .show_ui(ui, |ui| {
                    for (n, candidate) in candidates.iter().enumerate() {
                        let label =
                            format!("{} — {}", candidate.display(), human_bytes(candidate.bytes));
                        if ui
                            .selectable_label(draft.selected == Some(n), label)
                            .clicked()
                        {
                            draft.selected = Some(n);
                            draft.manual_exe = None;
                        }
                    }
                    if candidates.is_empty() {
                        ui.label(egui::RichText::new("Nothing was detected.").weak());
                    }
                });

            if ui.button("Browse…").clicked()
                && let Some(chosen) = rfd::FileDialog::new()
                    .set_title("Pick the executable to start")
                    .set_directory(&draft.source)
                    .add_filter("Programs", &["exe"])
                    .pick_file()
                && let Err(e) = draft.set_manual_exe(&chosen)
            {
                error = Some(e);
            }
        });

        if draft.manual_exe.is_some() {
            ui.label(egui::RichText::new("Chosen by hand.").weak().small());
        } else if candidates.is_empty() {
            ui.colored_label(
                WARN,
                "No likely executable in that folder — use Browse to point at one.",
            );
        } else if draft.selected.is_none() {
            // The ambiguous case. Naming the count is what makes it clear this
            // is a real choice and not a failure.
            ui.colored_label(
                WARN,
                format!(
                    "{} executables look equally likely — pick the right one.",
                    candidates.len()
                ),
            );
        }

        ui.add_space(6.0);

        // ── Cover ───────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label("Cover:");
            if ui.button("Choose an image…").clicked()
                && let Some(chosen) = rfd::FileDialog::new()
                    .set_title("Pick this game's cover")
                    .add_filter("Images", &["png", "webp", "jpg", "jpeg", "gif", "avif"])
                    .pick_file()
            {
                draft.set_image(chosen);
            }
            match &draft.image {
                Some(path) => {
                    ui.label(
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                    );
                }
                None => {
                    ui.colored_label(WARN, "None chosen.");
                }
            }
        });
        if let Some(warning) = &draft.image_warning {
            // A warning, not a rejection: v1 copies the file as-is rather than
            // resizing it, and the launcher renders whatever shape it is given.
            ui.colored_label(WARN, warning);
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} in {file_count} files",
                    human_bytes(total_bytes)
                ))
                .weak()
                .small(),
            );
            match draft.blocker() {
                Some(blocker) => ui.colored_label(WARN, blocker),
                None => ui.colored_label(GOOD, "Ready."),
            };
        });
    });

    if let Some(e) = error {
        app.error = Some(e);
    }
    dropped
}
