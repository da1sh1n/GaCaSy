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
//   volume.rs   the shared core — verify, version check, launch. Steps 2-5 of
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
// What makes a volume a cartridge is one file and one fact about it: a
// `launcher.exe` at the volume root carrying a signature from a key this binary
// was compiled to trust. There is no marker file and no secret on the disk.
//
//   trust.rs     the signature check, and the keys it checks against
//   version.rs   x.y.z, and asking a verified launcher for its own
//   alert.rs     the single case worth interrupting the user for
//   settings.rs  the fixed tunables, and where the log goes
//
// There is no configuration file. Trust is compiled in — a list of trusted keys
// in a writable file beside the exe would let anything able to edit it grant
// itself auto-run on every insert — and once that was gone, nothing left in the
// file was worth keeping. See build.rs and settings.rs.
//
// The listener keeps its files in one folder:
//
//   %LOCALAPPDATA%\GaCaSy\     (output/ in the repo)
//     listener.exe   <- this program
//     listener.log   <- what it did, and why it ignored what it ignored
//
// That folder is the whole install. The installer writes it there rather than
// into Program Files precisely so both stay together and stay writable: the log
// is the only way to see what this program did, and it must not end up
// somewhere other than beside the exe that wrote it.
//
// `cargo run` builds into target/ and runs in place, resolving the repo's
// `output/` as its folder and refreshing `output/listener.exe` so the shippable
// copy stays current.
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
//   listener.exe --version        print x.y.z and exit
//   listener.exe --signature      print this exe's own signature and the keys
//                                 it trusts, then exit
//
// No console window: this waits in the background, it is not a CLI tool.
#![windows_subsystem = "windows"]

mod alert;
mod log;
mod settings;
mod trigger;
mod trust;
mod version;
mod volume;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use log::Log;

/// What this invocation was asked to do.
enum Mode {
    /// Print `x.y.z` and exit.
    Version,
    /// Print this exe's own signature block and the keys it trusts, then exit.
    Signature,
    /// Run the core once against one volume, then exit.
    Check(PathBuf),
    /// Wait for cartridges. The default, and what the login entry starts.
    Trigger,
}

fn main() {
    // The two printing modes are answered before anything touches the disk. A
    // listener asks a *launcher* the same `--version` question, and the two are
    // the same program shape: creating folders or refreshing an exe as a side
    // effect of being asked a question would be a surprising thing for a
    // question to do — and on a cartridge, it would be a write to someone
    // else's disk.
    match mode() {
        Mode::Version => {
            attach_console();
            version::print();
        }
        Mode::Signature => {
            attach_console();
            print_signature();
        }
        Mode::Check(root) => {
            // Hand-run against one volume: the same core the triggers call, so
            // "does this cartridge work on this PC?" can be answered without
            // plugging anything in — and with no console, the log is the answer.
            let logger = start();
            volume::handle_volume(&root, &logger);
        }
        Mode::Trigger => trigger::run(start()),
    }
}

/// Sets the deployment folder up and opens the log. Shared by the two modes
/// that actually do something.
fn start() -> Log {
    let dir = resolve_base_dir();
    let _ = fs::create_dir_all(&dir);
    refresh_deployed_exe(&dir);
    Log::open(Some(settings::default_log_path(&dir)))
}

/// Prints this exe's own signature, for a human checking a download by hand.
///
/// Note what this is *not*. The listener never establishes a launcher's identity
/// by running it and reading this — a program reporting its own trustworthiness
/// proves nothing, and asking would mean executing the very binary you are
/// deciding whether to execute. See trust.rs. This output is for you.
fn print_signature() {
    match env::current_exe().and_then(fs::read) {
        Ok(bytes) => match sigblock::split(&bytes).1 {
            Some(signature) => print!("{signature}"),
            None => println!("unsigned"),
        },
        Err(e) => println!("unsigned (could not read this exe: {e})"),
    }
    println!("trusts: {}", trust::anchor_names());
}

/// Lets a GUI-subsystem process print to the terminal that started it.
///
/// Without this, `listener.exe --version` typed at a prompt writes to a handle
/// that goes nowhere. It has no bearing on being *probed* by another process —
/// there the parent supplies a real pipe and `println!` reaches it either way —
/// so this is purely for the human case. Failure is the normal path when
/// nothing launched us from a console, and is ignored.
#[cfg(windows)]
fn attach_console() {
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

#[cfg(not(windows))]
fn attach_console() {}

/// The folder holding `listener.exe` and its log.
///
/// Normally that is simply the folder the exe is in — `%LOCALAPPDATA%\GaCaSy\`
/// where the installer puts it, `output/`, or wherever the exe was dropped by
/// hand. The one exception is a `cargo run`/`cargo test` build, whose exe sits
/// under `target/`: that resolves to the repo's `output/` instead, so
/// development reads and writes the same deployed folder it refreshes.
///
/// The dev check is "is the exe inside a `target/` belonging to this crate?"
/// rather than the launcher's "is the exe's parent folder named `output`?". The
/// difference matters: the latter silently treats an installed
/// `…\AppData\Local\GaCaSy\listener.exe` as a dev build, because that parent is
/// not named `output` either — the bug noted against `running_deployed()` in
/// ../../launcher/src/main.rs.
///
/// *Two* target directories are checked because the crate builds both ways:
/// standalone its artifacts land in `listener/target/`, and inside the repo
/// workspace (../Cargo.toml) they land in the shared `../target/`. Checking only
/// the first would make a workspace `cargo run` look deployed and write its log
/// into `target/debug/` — silently, since there is no console.
fn resolve_base_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dev_targets = [
        Some(manifest.join("target")),
        manifest.parent().map(|root| root.join("target")),
    ];

    let exe = env::current_exe().ok();
    if let Some(exe) = &exe
        && dev_targets
            .iter()
            .flatten()
            .any(|target| exe.starts_with(target))
    {
        return manifest.join("output");
    }
    exe.and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Copies the freshly built exe to `output/listener.exe`, so the shippable copy
/// tracks the source the same way the launcher's does.
///
/// Skipped when we already are that copy. Failure is non-fatal and expected
/// whenever a deployed listener is holding its own file open, or the exe was
/// dropped somewhere read-only.
///
/// The signature block rides along, being part of the file: a signed exe stays
/// signed through a deploy, and a `cargo build` that produced an unsigned one
/// overwrites the signed copy with an unsigned one — which is exactly what you
/// want to notice, and what `xtask verify` will tell you.
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

/// Reads the command line.
///
/// Anything unrecognised — including `--check` with no path — falls through to
/// the trigger, since a listener that refuses to start over a typo'd argument is
/// a listener that silently stops working at login. The two printing modes are
/// the exception and win outright: doing nothing else is their whole purpose.
fn mode() -> Mode {
    let mut args = env::args_os().skip(1);
    let mut check = None;
    while let Some(arg) = args.next() {
        if arg == "--version" {
            return Mode::Version;
        }
        if arg == "--signature" {
            return Mode::Signature;
        }
        if arg == "--check" && check.is_none() {
            check = args.next().map(PathBuf::from);
        }
    }
    match check {
        Some(root) => Mode::Check(root),
        None => Mode::Trigger,
    }
}
