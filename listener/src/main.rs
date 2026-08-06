// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// Entry point. Parses the command line into a `Mode` and dispatches: print a
// version or signature, run the core once against one volume, or hand off to
// the platform trigger. Also resolves the folder the exe and its log live in.
//
// No console window: this waits in the background, it is not a CLI tool.
#![windows_subsystem = "windows"]
// Functions are camelCase in this project while variables stay snake_case,
// which rustc's default lints object to. Silenced once, at the crate root.
#![allow(non_snake_case)]

mod alert;
mod log;
mod settings;
mod trigger;
mod trust;
mod version;
mod volume;

#[cfg(test)]
mod tests;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use log::Log;

// ########## STARTUP ##########

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
    // The two printing modes are answered before anything touches the disk:
    // creating folders or refreshing an exe as a side effect of being asked a
    // question would be surprising, and on a cartridge it would be a write to
    // someone else's disk.
    match mode() {
        Mode::Version => {
            sigblock::cli::attachConsole();
            version::print();
        }
        Mode::Signature => {
            sigblock::cli::attachConsole();
            sigblock::cli::printSignature();
            // What this listener will accept, which is the other half of the
            // question "is this exe the one I think it is?".
            println!("trusts: {}", trust::anchorNames());
        }
        // Hand-run against one volume: the same core the triggers call, so
        // "does this cartridge work on this PC?" can be answered without
        // plugging anything in.
        Mode::Check(root) => {
            let logger = start();
            volume::handleVolume(&root, &logger);
        }
        Mode::Trigger => trigger::run(start()),
    }
}

/// Sets the deployment folder up and opens the log. Shared by the two modes
/// that actually do something.
fn start() -> Log {
    let dir = resolveBaseDir();
    let _ = fs::create_dir_all(&dir);
    refreshDeployedExe(&dir);
    Log::open(Some(settings::defaultLogPath(&dir)))
}

/// Reads the command line into a `Mode`. Anything unrecognised, including
/// `--check` with no path, falls through to `Mode::Trigger`; `--version` and
/// `--signature` return immediately and win over everything else.
fn mode() -> Mode {
    // `args_os` so a non-UTF-8 argument cannot panic us before we have started.
    let mut args = env::args_os().skip(1);
    let mut check = None;
    // `while let` rather than `for`, because `--check` consumes the next
    // argument and so needs the iterator itself inside the loop.
    while let Some(arg) = args.next() {
        if arg == "--version" {
            return Mode::Version;
        }
        if arg == "--signature" {
            return Mode::Signature;
        }
        // `is_none()` so a repeated `--check` keeps the first path.
        if arg == "--check" && check.is_none() {
            check = args.next().map(PathBuf::from);
        }
    }
    match check {
        Some(root) => Mode::Check(root),
        None => Mode::Trigger,
    }
}

// ========== The Deployment Folder ==========

/// The folder holding `listener.exe` and its log: normally just the folder the
/// exe is in, but the repo's `output/` for a `cargo run` or `cargo test` build.
///
/// The dev check is "is the exe inside a `target/` belonging to this crate?"
/// rather than "is its parent named `output`?" — the latter would treat an
/// installed `…\AppData\Local\Romzeta\listener.exe` as a dev build too.
///
/// *Two* target directories, because the crate builds both ways: standalone its
/// artifacts land in `listener/target/`, and inside the workspace in the shared
/// `../target/`. Checking only the first would make a workspace `cargo run`
/// look deployed and silently write its log into `target/debug/`.
fn resolveBaseDir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dev_targets = [
        Some(manifest.join("target")),
        manifest.parent().map(|root| root.join("target")),
    ];

    let exe = env::current_exe().ok();
    // A let-chain: bind the exe path and test it in one condition, so the
    // `else` below can still consume `exe` by value.
    if let Some(exe) = &exe
        && dev_targets
            .iter()
            // `flatten` drops the None from a manifest with no parent.
            .flatten()
            .any(|target| exe.starts_with(target))
    {
        return manifest.join("output");
    }
    exe.and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Copies the freshly built exe to `output/listener.exe`, so the shippable copy
/// tracks the source. Skipped when we already are that copy.
///
/// Failure is non-fatal and expected whenever a deployed listener is holding
/// its own file open. The signature block rides along, being part of the file —
/// so a `cargo build` that produced an unsigned exe overwrites the signed copy,
/// which is exactly what `xtask verify` is for noticing.
fn refreshDeployedExe(base: &Path) {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let deployed = base.join(if cfg!(windows) {
        "listener.exe"
    } else {
        "listener"
    });
    // Canonicalized before comparing, so a path reached two different ways
    // (a symlink, `..`, a short 8.3 name) is still recognised as the same file.
    if let (Ok(a), Ok(b)) = (exe.canonicalize(), deployed.canonicalize())
        && a == b
    {
        return;
    }
    let _ = fs::copy(&exe, &deployed);
}
