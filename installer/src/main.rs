// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// GaCaSy installer — the only file a user has to obtain.
//
// Everything else in the system is placed by this program: `launcher.exe` on
// the cartridge, `listener.exe` on the PC, and the config and catalog files
// beside them. It carries all of them inside itself and **downloads nothing** —
// one self-contained exe, no prerequisites, no side-by-side files. See
// payload.rs and build.rs for how that is enforced.
//
// It does three jobs, specced in structure.md:
//
//   1. Create a cartridge   pick a drive, add games, copy.
//   2. Install the listener put it on the PC and make it start at login.
//   3. Edit a cartridge     a drive that already has a launcher on it.
//
// Jobs 1 and 3 are the same write with a different starting catalog, which is
// why they share the games screen and cartridge::apply. Job 2 is independent
// and reachable on its own.
//
// **There is no pairing step and no key screen.** A cartridge's identity is the
// minisign signature inside the `launcher.exe` this installer carries, put there
// at build time; the listener it installs is compiled to trust that same key. So
// creating a cartridge is only ever copying files, and the two halves recognise
// each other because they were built together — not because anything on either
// disk says so.
//
// The source is split by job:
//
//   app.rs        wizard state, and the create-vs-edit routing rule
//   ui/           the screens; ui/mod.rs holds the shell and the only footer
//   work.rs       the worker thread, and how the UI hears from it
//   payload.rs    the embedded launcher, listener and seed files
//   volume.rs     which drives can be cartridges, and which already are
//   detect.rs     finding the game's exe inside a folder, and measuring it
//   image.rs      cover dimensions, for the 2:3 warning
//   catalog.rs    catalog.json, the file the launcher reads
//   cartridge.rs  the write itself: copy, catalog, config, launcher
//   listener.rs   job 2 — install folder, Run entry, uninstall
//   copy.rs       the cancellable, measured file copy underneath it all
//   version.rs    --version / --signature
//
// This is egui rather than a webview, unlike the launcher: a WebView2-based
// installer that found the runtime missing would have no way to bootstrap
// itself with no internet.
//
// It asks for **no elevation**, and carries no code that could. Writing a
// cartridge never needed it, and the listener has exactly one home —
// %LOCALAPPDATA%\GaCaSy, alongside its log — which the user can always write.
// See listener.rs for why that differs from structure.md's first draft, which
// specced Program Files.
//
//   installer.exe --version     print x.y.z and exit
//   installer.exe --signature   print this exe's signature and exit
//
// No console window: this is a GUI app.
#![windows_subsystem = "windows"]

mod app;
mod cartridge;
mod catalog;
mod copy;
mod detect;
mod image;
mod listener;
mod payload;
mod ui;
mod version;
mod volume;
mod work;

use eframe::egui;

fn main() -> eframe::Result {
    // Before the window, before anything. Same rule as the other two: being
    // asked a question is not a reason to start doing work.
    if version::handled() {
        return Ok(());
    }

    // Enumerating drive letters touches removable drives, and an empty card
    // reader would otherwise pop the modal "There is no disk in the drive" box.
    // Same reasoning as the listener's sweep — see
    // ../../listener/src/trigger/windows.rs.
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode(
            windows_sys::Win32::System::Diagnostics::Debug::SEM_FAILCRITICALERRORS,
        );
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GaCaSy Installer")
            .with_inner_size([920.0, 660.0])
            .with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "GaCaSy Installer",
        options,
        Box::new(|cc| {
            // A shade larger than egui's default. This is a wizard read once by
            // someone who has never seen it, not a tool used daily.
            cc.egui_ctx.all_styles_mut(|style| {
                for (_, font) in style.text_styles.iter_mut() {
                    font.size *= 1.15;
                }
                style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            });
            Ok(Box::new(app::App::new()))
        }),
    )
}
