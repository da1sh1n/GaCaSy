// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// Romzeta installer — the only file a user has to obtain.
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
//   ui/           the screens; ui/mod.rs holds the frame and the only footer
//   shell.rs      the window, the GL context and the event loop under it
//   font.rs       the desktop's own UI font, read off the disk
//   clipboard.rs  copy and paste for the one field that has any
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
// itself with no internet. It is egui *without* `eframe`, and without an
// embedded typeface, because this is the one file a user downloads and both
// cost more than they are worth here — see shell.rs and font.rs.
//
// It asks for **no elevation**, and carries no code that could. Writing a
// cartridge never needed it, and the listener has exactly one home —
// %LOCALAPPDATA%\Romzeta, alongside its log — which the user can always write.
// See listener.rs for why that differs from structure.md's first draft, which
// specced Program Files.
//
//   installer.exe --version     print x.y.z and exit
//   installer.exe --signature   print this exe's signature and exit
//
// No console window: this is a GUI app.
#![windows_subsystem = "windows"]

mod app;
mod autoplay;
mod cartridge;
mod catalog;
mod clipboard;
mod copy;
mod detect;
mod font;
mod image;
mod listener;
mod payload;
mod reg;
mod shell;
mod ui;
mod version;
mod volume;
mod work;

#[cfg(test)]
mod tests;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    shell::run(app::App::new())?;
    Ok(())
}
