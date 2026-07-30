// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! `--version` and `--signature`, answered before the window opens.
//!
//! Nothing probes the installer the way the listener probes a launcher — it is
//! the one program in the system no other program runs. It answers anyway,
//! because the person who just downloaded a setup exe from the internet is
//! exactly the person entitled to ask what it is and who signed it, and because
//! three programs that answer the same two questions the same way are easier to
//! trust than two that do and one that doesn't.
//!
//! # Probing a cartridge's own launcher
//!
//! The installer *does* probe one thing: the `launcher.exe` already sitting on a
//! cartridge being edited, to notice when it is stale next to the one this
//! installer carries. [`probe`] mirrors `../../listener/src/version.rs` — same
//! bare `x.y.z` contract, same bounded wait — but the comparison in
//! [`crate::app`] checks every field, not just the major: the listener only cares
//! whether two programs can *talk*, while this is "does this cartridge have the
//! newest launcher this installer knows how to write."

use std::env;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::payload;

/// How long a cartridge's launcher gets to answer `--version`. Same bound as the
/// listener's probe, for the same reason: this runs on the UI thread's behalf and
/// must not be able to wedge it behind an exe that never exits.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// How often to check whether it has exited.
const POLL: Duration = Duration::from_millis(25);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The launcher this installer carries — from `payload::LAUNCHER_VERSION`, which
/// `build.rs` reads out of `../launcher/Cargo.toml`. `None` only under the
/// `GACASY_PAYLOAD_OPTIONAL` escape hatch, where there is no real launcher to
/// compare against anyway.
pub fn bundled() -> Option<Version> {
    parse(payload::LAUNCHER_VERSION)
}

/// Parses exactly `x.y.z`. Strict for the same reason the listener's is: a
/// launcher that answers something else is not one whose version we can compare.
pub fn parse(text: &str) -> Option<Version> {
    let mut parts = text.trim().split('.');
    let mut next = || parts.next()?.trim().parse::<u64>().ok();
    let version = Version {
        major: next()?,
        minor: next()?,
        patch: next()?,
    };
    parts.next().is_none().then_some(version)
}

/// Asks a cartridge's `launcher.exe` for its version.
///
/// `None` means it could not be established — refused to start, took too long, or
/// said something unparseable. Treated by the caller as "nothing to report" rather
/// than "definitely stale": a launcher too old to have `--version` at all is a
/// real possibility this installer has no business guessing at.
pub fn probe(exe: &Path) -> Option<Version> {
    let child = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    answer(child, PROBE_TIMEOUT)
}

/// Waits for a probe to finish and reads its first line, killing it on timeout.
fn answer(mut child: std::process::Child, timeout: Duration) -> Option<Version> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read;
        stdout.read_to_string(&mut output).ok()?;
    }
    parse(output.lines().next()?)
}

/// Handles `--version` / `--signature` / `--help` if asked. `true` means the
/// program has said what it was asked and should exit now.
pub fn handled() -> bool {
    let mut version = false;
    let mut signature = false;
    let mut help = false;
    for arg in env::args_os().skip(1) {
        version |= arg == "--version";
        signature |= arg == "--signature";
        help |= arg == "--help" || arg == "-h";
    }
    if !version && !signature && !help {
        return false;
    }

    sigblock::cli::attach_console();
    if version {
        println!("{}", env!("CARGO_PKG_VERSION"));
    }
    if signature {
        sigblock::cli::print_signature();
    }
    if help {
        println!("GaCaSy installer {}", env!("CARGO_PKG_VERSION"));
        println!();
        println!("Run it with no arguments to open the installer.");
        println!("  --version    print x.y.z");
        println!("  --signature  print this exe's signature");
    }
    true
}
