// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// Entry point. Answers `--version` / `--signature`, resolves the content folder
// and seeds it, takes the single-instance lock when deployed, then hands off to
// `ui::run`.
//
// No console window: this is a GUI app.
#![windows_subsystem = "windows"]
// Functions are camelCase in this project while variables stay snake_case,
// which rustc's default lints object to. Silenced once, at the crate root.
#![allow(non_snake_case)]

// ########## ENTRY POINT ##########

mod assets;
mod catalog;
mod config;
mod constants;
mod content;
mod instance;
mod launch;
mod log;
mod order;
mod ui;
mod version;
mod window;

#[cfg(test)]
mod tests;

fn main() -> wry::Result<()> {
    // First, before anything touches the disk. The listener asks a verified
    // launcher for its version, and a launcher that seeded folders or rewrote
    // its own exe on the way to answering would be writing to the cartridge in
    // response to a question. See version.rs.
    if version::handled() {
        return Ok(());
    }

    let base_dir = content::resolveBaseDir();
    content::ensureLayout(&base_dir);

    // Single-instance is enforced only for the shipped launcher (the exe in
    // output/). Under `cargo run` it is deliberately skipped so a rebuild
    // always opens a fresh window instead of silently exiting when an older
    // run is still on screen holding the lock — the classic "my change did
    // nothing" trap during development. Nothing listens on a port: the guard
    // is a named mutex the OS releases when the process dies.
    let _instance = if content::runningDeployed() {
        match instance::acquire() {
            Some(guard) => Some(guard),
            None => return Ok(()),
        }
    } else {
        None
    };

    ui::run(&base_dir)
}
