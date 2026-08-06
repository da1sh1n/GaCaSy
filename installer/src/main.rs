// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// Entry point. Answers `--version` / `--signature` / `--help`, silences the
// "no disk in drive" dialog that enumerating removable drives would raise, then
// opens the wizard window through `shell::run`.
//
// No console window: this is a GUI app.
#![windows_subsystem = "windows"]
// Functions are camelCase in this project while variables stay snake_case,
// which rustc's default lints object to. Silenced once, at the crate root.
#![allow(non_snake_case)]

// ########## ENTRY POINT ##########

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
