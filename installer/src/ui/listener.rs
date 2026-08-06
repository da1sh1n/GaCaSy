// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Draws the listener screen: install, repair and remove buttons, the install
//! path, and the AutoPlay checkbox.

// ########## THE LISTENER SCREEN ##########

use crate::app::App;
use crate::autoplay;
use crate::listener;

use super::{BAD, GOOD, WARN};

pub fn screen(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();

    installed(app, ui);
    ui.separator();

    let installed_here = app.listener_installs.iter().any(|install| !install.legacy);
    ui.label(
        egui::RichText::new(if installed_here {
            "Repair or update"
        } else {
            "Install"
        })
        .strong(),
    );
    ui.add_space(6.0);

    match listener::installDir() {
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

    // The other half of "plug it in and the launcher comes up". The listener
    // starting the launcher is not enough on its own if Windows opens an
    // Explorer window over it a moment later, which is what most PCs are set to
    // do. Spelled out rather than done quietly because it is the one setting
    // this installer changes that is not Romzeta's own — see ../autoplay.rs.
    let already = autoplay::suppressed();
    ui.add_enabled_ui(!already, |ui| {
        let mut ticked = app.suppress_autoplay || already;
        if ui
            .checkbox(
                &mut ticked,
                "Stop Windows opening a folder when a cartridge is plugged in",
            )
            .changed()
        {
            app.suppress_autoplay = ticked;
        }
    });
    ui.label(
        egui::RichText::new(if already {
            "Already set — Windows takes no action when a removable drive arrives. \
             Uninstalling the listener puts your previous setting back."
        } else {
            "Sets AutoPlay to \"Take no action\" for all removable drives on your account. \
             Windows offers no per-device setting for a drive it has not seen before, so this \
             is the only way to stop it. Uninstalling puts your previous setting back."
        })
        .weak(),
    );

    ui.add_space(12.0);
    let blocked = listener::installDir().is_none();
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
        app.installListener(&ctx);
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
        autoplayStatus(ui);
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

    autoplayStatus(ui);

    if let Some(dir) = uninstall {
        app.uninstallListener(&ctx, dir);
    }
}

/// What Windows itself does when a drive arrives. Shown next to the install
/// state because the two together are what "plugging a cartridge in works"
/// actually means: a listener that starts the launcher, and nothing else
/// opening on top of it.
fn autoplayStatus(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    if autoplay::suppressed() {
        ui.colored_label(
            GOOD,
            "Windows leaves removable drives alone — nothing opens over the launcher.",
        );
    } else if autoplay::opensAFolder() {
        ui.colored_label(
            WARN,
            "Windows opens a folder when a drive is plugged in — it will appear over the \
             launcher.",
        );
    } else {
        // Some other handler, or the "choose what to do" prompt. Still
        // something arriving on screen uninvited, but not worth naming
        // specifically when the fix below is the same either way.
        ui.colored_label(
            WARN,
            "Windows does something of its own when a drive is plugged in, which may appear \
             over the launcher.",
        );
    }
}
