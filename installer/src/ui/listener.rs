// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Job 2 — the PC side, on its own screen because it is independent of the
//! cartridge flow. Setting up a PC and making a cartridge are separate errands
//! that happen to be shipped in one exe.
//!
//! There is **no choice of location** and no elevation on this screen: the
//! listener lives in `%LOCALAPPDATA%\GaCaSy` with its config and its log, and
//! that is the whole of it. The path is still shown, because the one thing a
//! user needs from a program with no window is to know where to look.
//!
//! The one thing this screen has to communicate, and the reason it is wordier
//! than the others: **an empty key list trusts every cartridge**. That is the
//! listener's unpaired default (`../../../listener/structure.md`), so pairing is
//! the installer *tightening* something that starts open — not opening something
//! that starts shut. A user who skips the key field should know which of those
//! they just did.

use eframe::egui;

use crate::app::App;
use crate::listener;

use super::{BAD, GOOD, WARN};

pub fn screen(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();

    installed(app, ui);
    ui.separator();

    let installed_here = app
        .listener_installs
        .iter()
        .any(|install| !install.legacy);
    ui.label(
        egui::RichText::new(if installed_here {
            "Repair or update"
        } else {
            "Install"
        })
        .strong(),
    );
    ui.add_space(6.0);

    match listener::install_dir() {
        Some(dir) => {
            ui.horizontal(|ui| {
                ui.label("Goes in:");
                ui.label(egui::RichText::new(dir.display().to_string()).strong());
            });
            ui.label(
                egui::RichText::new(
                    "Its log sits in that same folder. Nothing here needs administrator.",
                )
                .weak(),
            );
        }
        None => {
            ui.colored_label(
                BAD,
                "This account has no %LOCALAPPDATA%, so there is nowhere to install to.",
            );
        }
    }
    ui.add_space(10.0);

    // No pairing step. There used to be a key field here, and a warning that
    // leaving it blank made this PC auto-launch *any* cartridge — which was
    // true, and was the default. Neither exists now: the listener only starts a
    // launcher whose signature it can verify against a key built into it, so
    // there is nothing to type and no way to leave it open by accident.
    ui.colored_label(
        GOOD,
        "This PC will auto-launch any cartridge made by this installer, and no others.",
    );
    ui.label(
        egui::RichText::new(
            "Cartridges are recognised by the signature inside their launcher, so there is \
             no key to enter and nothing to pair.",
        )
        .weak(),
    );

    ui.add_space(10.0);
    ui.checkbox(
        &mut app.listener_start_now,
        "Start it now as well as at every login",
    );

    ui.add_space(12.0);
    let blocked = listener::install_dir().is_none();
    let label = if installed_here {
        "Repair / update"
    } else {
        "Install"
    };
    if ui
        .add_enabled(
            !blocked,
            egui::Button::new(label).min_size([140.0, 32.0].into()),
        )
        .clicked()
    {
        app.install_listener(&ctx);
    }
}

fn installed(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    ui.label(egui::RichText::new("On this PC").strong());
    ui.add_space(6.0);

    if app.listener_installs.is_empty() {
        ui.colored_label(
            WARN,
            "The listener is not installed. Cartridges won't auto-start.",
        );
        ui.add_space(6.0);
        return;
    }

    let mut uninstall = None;
    for install in &app.listener_installs {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(install.dir.display().to_string());
                    if install.legacy {
                        // Left by an earlier build that installed elsewhere.
                        // Installing folds it in; the row exists so the user can
                        // also just delete it, and so the folder is never a
                        // surprise found months later.
                        ui.colored_label(
                            WARN,
                            "Left by an earlier version. Installing removes it and puts the \
                             current one in the folder above.",
                        );
                    } else if install.autostart {
                        ui.colored_label(GOOD, "Starts at login.");
                    } else {
                        // The repair case worth naming: the binary is there and
                        // nothing runs it, so the PC looks set up and isn't.
                        ui.colored_label(
                            BAD,
                            "Installed but NOT set to start at login — use Repair below.",
                        );
                    }
                    // Nothing about trust is shown per install any more, because
                    // there is nothing per install to show: what a listener
                    // accepts is compiled into it, identical for every copy this
                    // installer has ever written.
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Uninstall").clicked() {
                        uninstall = Some(install.dir.clone());
                    }
                });
            });
        });
        ui.add_space(4.0);
    }

    if let Some(dir) = uninstall {
        app.uninstall_listener(&ctx, dir);
    }
}
