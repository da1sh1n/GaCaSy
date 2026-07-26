// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// GaCaSy listener — the PC side.
//
// Notices a cartridge being connected, checks that it is one this PC trusts,
// and starts that cartridge's launcher. Plugging a cartridge in then "just
// works", the way slotting one into a console does.
//
// The code is one crate in two halves:
//
//   volume.rs   the shared core — marker, trust check, launch. Steps 2-5 of
//               structure.md's flow, existing exactly once.
//   trigger/    the per-OS half: what notices a volume, and how long this
//               process lives while it waits.
//
// The two triggers differ in **process lifetime**, not merely in which API
// they call. On Windows the listener is resident from login to logout, blocked
// in GetMessage waiting for WM_DEVICECHANGE. On Linux it does not exist until
// udev starts it, and it exits once it has acted. Neither is free; the
// question is only which already-resident host does the waiting, and Linux has
// one (systemd-udevd) where Windows does not. See structure.md,
// "Execution models" — it is the most important thing about this program.
//
// Windows is built; Linux is specced but not implemented (see
// trigger/linux.rs).
//
// The listener keeps its own files in one folder — the same shape the launcher
// uses, minus the content it has no use for:
//
//   output/
//     listener.exe   <- this program
//     config.toml    <- the keys this PC trusts; seeded from src/config.toml
//     listener.log   <- what it did, and why it ignored what it ignored
//
// `cargo run` builds into target/ and runs in place, resolving the repo's
// `output/` as its folder and refreshing `output/listener.exe` so the shippable
// copy stays current. config.toml is never overwritten once present, so an
// edited key list survives every build.
//
// The only thing it reads off a cartridge is that cartridge's own marker,
// <volume>/.cartridge — version, key, and the launcher to start.
//
// The log matters more here than in most programs: there is no console and, on
// Windows, no window, so it is the only way to see what happened. Running the
// listener by hand is supported and does nothing special — registering the
// login entry (Windows) and installing the udev rule (Linux) belong to the
// installer.
//
//   listener.exe                  start the trigger for this platform
//   listener.exe --check E:\      run the core once against one volume, then
//                                 exit. The result goes to the log.
//
// No console window: this waits in the background, it is not a CLI tool.
#![windows_subsystem = "windows"]

mod config;
mod log;
mod marker;
mod trigger;
mod volume;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use log::Log;

fn main() {
    let dir = resolve_base_dir();
    ensure_layout(&dir);
    let settings = config::load(&dir);
    let logger = Log::open(settings.log_file.clone());

    match check_target() {
        // Hand-run against one volume: the same core the triggers call, so
        // "does this cartridge work on this PC?" can be answered without
        // plugging anything in — and with no console, the log is the answer.
        Some(root) => {
            volume::handle_volume(&root, &settings, &logger);
        }
        None => trigger::run(settings, logger),
    }
}

/// The folder holding `listener.exe`, its config.toml and its log.
///
/// Normally that is simply the folder the exe is in, whether that is `output/`,
/// `C:\Program Files\GaCaSy\`, or wherever the exe was dropped. The one
/// exception is a `cargo run`/`cargo test` build, whose exe sits under
/// `target/`: that resolves to the repo's `output/` instead, so development
/// reads and writes the same deployed folder it refreshes.
///
/// The dev check is "is the exe inside this crate's `target/`?" rather than the
/// launcher's "is the exe's parent folder named `output`?". The difference
/// matters: the latter silently treats an installed `Program Files\…\listener.exe`
/// as a dev build, because that parent is not named `output` either — the bug
/// noted against `running_deployed()` in ../../launcher/src/main.rs.
fn resolve_base_dir() -> PathBuf {
    let manifest_target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let exe = env::current_exe().ok();
    if let Some(exe) = &exe
        && exe.starts_with(&manifest_target)
    {
        return Path::new(env!("CARGO_MANIFEST_DIR")).join("output");
    }
    exe.and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Creates the folder and refreshes the deployed exe. config.toml is seeded by
/// `config::load`, and nothing existing is ever overwritten.
fn ensure_layout(base: &Path) {
    let _ = fs::create_dir_all(base);
    refresh_deployed_exe(base);
}

/// Copies the freshly built exe to `output/listener.exe`, so the shippable copy
/// tracks the source the same way the launcher's does.
///
/// Skipped when we already are that copy. Failure is non-fatal and expected in
/// two ordinary cases: a deployed listener is holding its own file open, or the
/// folder is read-only (`Program Files`).
fn refresh_deployed_exe(base: &Path) {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let deployed = base.join(if cfg!(windows) {
        "listener.exe"
    } else {
        "listener"
    });
    if let (Ok(a), Ok(b)) = (exe.canonicalize(), deployed.canonicalize())
        && a == b
    {
        return;
    }
    let _ = fs::copy(&exe, &deployed);
}

/// The path from `--check <path>`, if given. Anything else — including
/// `--check` with no path — falls through to the trigger, since a listener
/// that refuses to start over a typo'd argument is a listener that silently
/// stops working at login.
fn check_target() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--check" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}
