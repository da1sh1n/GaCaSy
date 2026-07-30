// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// Cartridge launcher shell.
//
// The whole UI (HTML/CSS/JS) is baked into this exe with rust-embed and
// served over an `app://` custom protocol straight out of memory — there
// is no bundled web server and nothing ever listens on a port.
//
// Everything the exe reads from disk is cartridge *content* that lives
// beside it in `output/`:
//
//   output/
//     launcher.exe   <- this program
//     config.toml    <- seeded from the baked-in default if missing
//     catalog.json   <- the game list (name / exe / image), seeded likewise
//     images/        <- 600x900 cover art, dropped in by hand
//     games/         <- the actual game installs
//     logs/          <- launch history + each game's stdout/stderr (see log.rs)
//     EBWebView/     <- WebView2's own data folder (its only on-disk crumbs)
//
// `cargo run` builds into target/ and runs in place, resolving `output/`
// as its content directory and refreshing `output/launcher.exe` so the
// shippable copy stays current. It does not relaunch itself, so there is
// exactly one launcher process (WebView2 still spawns its own renderer
// process — that is inherent to the engine and cannot be merged away).
//
// The source is split by job, and this file is only the front door:
//
//   constants.rs  every tunable number in the crate, in one place
//   content.rs    which folder holds the content, and seeding it on first run
//   config.rs     reading config.toml, key by key, with defaults under it
//   catalog.rs    the game list, and marking which games are actually present
//   assets.rs     the `app://` protocol: the embedded UI and disk content
//   window.rs     how big the window is and where it sits
//   ui.rs         the window + webview, the IPC, and the event loop
//   launch.rs     starting a game and deciding whether it came up
//   log.rs        logs/launcher.log and each game's own output
//   instance.rs   the single-instance mutex
//   version.rs    --version / --signature, answered before anything else
//   index.html    the UI itself, served over app://
//
// This exe carries its own minisign signature, appended past the end of the
// image (see the sigblock crate). That signature is the cartridge's whole
// identity: the listener reads it off the disk and refuses to start a launcher
// it cannot verify. The launcher itself does nothing to earn that beyond being
// the signed binary — it holds no secret and checks nothing.
//
//   launcher.exe --version     print x.y.z and exit
//   launcher.exe --signature   print this exe's signature and exit
//
// Window sizing (see `constants`): the window wraps the covers on both axes —
// covers aim for a fraction of the screen width and the window is just big
// enough for them plus margins — but two caps (max width and max height
// fraction of the screen) shrink the covers to fit when they'd be too big.
// Rust picks the window size and the CSS in src/index.html fits the covers into
// it; the shared border/image gap numbers (from config.toml, mirrored as
// PAD/GAP in the page) keep the two in step.
//
// No console window: this is a GUI app, not a CLI tool.
#![windows_subsystem = "windows"]

mod assets;
mod catalog;
mod config;
mod constants;
mod content;
mod instance;
mod launch;
mod log;
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

    let base_dir = content::resolve_base_dir();
    content::ensure_layout(&base_dir);

    // Single-instance is enforced only for the shipped launcher (the exe in
    // output/). Under `cargo run` it is deliberately skipped so a rebuild
    // always opens a fresh window instead of silently exiting when an older
    // run is still on screen holding the lock — the classic "my change did
    // nothing" trap during development. Nothing listens on a port: the guard
    // is a named mutex the OS releases when the process dies.
    let _instance = if content::running_deployed() {
        match instance::acquire() {
            Some(guard) => Some(guard),
            None => return Ok(()),
        }
    } else {
        None
    };

    ui::run(&base_dir)
}
