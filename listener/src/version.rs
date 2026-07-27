// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! `x.y.z`, and asking a launcher for its own.
//!
//! **x is shared by every GaCaSy program** and means "the way these programs
//! talk to each other"; y and z belong to each program alone. Two programs are
//! compatible when their x matches, which is a thing the listener can actually
//! check before handing control to a cartridge built against a different
//! generation of the system.
//!
//! # Why it is safe to ask
//!
//! [`probe`] runs `launcher.exe --version` and believes the answer. That is only
//! defensible because it happens strictly *after* [`crate::trust`] has verified
//! the binary's signature: at that point it is a program we signed, and taking
//! its word about itself is reasonable. Doing this before the signature check
//! would mean executing an untrusted binary in order to decide whether to
//! execute it.
//!
//! # Why the output is bare
//!
//! Every GaCaSy exe prints exactly `x.y.z` and nothing else — no program name,
//! no prefix. It is one line for a human and one line to parse, and the two
//! having the same shape is what keeps them from drifting apart.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a launcher gets to answer. Generous for a process that only has to
/// print a string and exit — and bounded because the Windows listener asks from
/// its message-pump thread, where an unbounded wait would freeze every later
/// device event behind one wedged binary.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// How often to check whether it has exited. `wait_timeout` is not in std, and
/// polling is both portable and cheap at this granularity.
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

/// This listener's own version, from its Cargo manifest. There is deliberately
/// no second place to update.
pub fn own() -> Version {
    parse(env!("CARGO_PKG_VERSION")).expect("our own version is a valid x.y.z")
}

/// Prints the `--version` line.
pub fn print() {
    println!("{}", own());
}

/// Parses exactly `x.y.z`.
///
/// Strict on purpose. A launcher that answers something else is not a launcher
/// whose compatibility we can reason about, and guessing at "0.2" or
/// "0.2.0-rc1" would be inventing a claim the binary never made.
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

/// Asks a verified launcher for its version.
///
/// `None` means it could not be established — the binary would not start, took
/// too long, or said something unparseable. That is deliberately distinct from
/// "said a different major": the caller launches anyway on `None` and refuses on
/// a genuine mismatch, because a signed binary that fumbles the probe is a
/// weaker signal than one that clearly states an incompatible version.
pub fn probe(exe: &Path) -> Option<Version> {
    // No `current_dir` on the volume: this is a question, not a launch, and the
    // launcher seeds content into its working directory when it really starts.
    //
    // Piped stdout matters more than it looks on Windows. The launcher is built
    // `windows_subsystem = "windows"`, so it has no console — but that only
    // means Windows allocates none. `Command` creates a pipe and passes it in
    // STARTUPINFO, so the child's stdout handle is valid and `println!` lands
    // here.
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
///
/// Split out from [`probe`] so the giving-up path can be tested with a process
/// chosen to never answer — the case that matters most here, since it is the one
/// that would otherwise wedge the Windows message pump.
fn answer(mut child: std::process::Child, timeout: Duration) -> Option<Version> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            // Timed out, or waiting itself failed. Kill it either way: a version
            // probe must not leave a process behind.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    // Safe to read only after it has exited: a version string is far smaller
    // than the pipe buffer, so the child cannot have blocked on a full pipe
    // while we were polling.
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read;
        stdout.read_to_string(&mut output).ok()?;
    }
    parse(output.lines().next()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_bare_version() {
        assert_eq!(
            parse("0.2.0"),
            Some(Version {
                major: 0,
                minor: 2,
                patch: 0
            })
        );
        assert_eq!(parse("  12.3.45  \r\n").map(|v| v.major), Some(12));
    }

    #[test]
    fn refuses_anything_that_is_not_three_numbers() {
        // The shapes a well-meaning change might introduce. Each would be a
        // guess about what the launcher meant, so each is refused.
        assert_eq!(parse("gacasy-launcher 0.2.0"), None);
        assert_eq!(parse("0.2"), None);
        assert_eq!(parse("0.2.0.1"), None);
        assert_eq!(parse("0.2.0-rc1"), None);
        assert_eq!(parse("v0.2.0"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("not a version"), None);
    }

    #[test]
    fn our_own_version_parses() {
        // If this fails, `--version` is printing something the listener on the
        // other side of a probe could not read.
        let own = own();
        assert_eq!(parse(&own.to_string()), Some(own));
    }

    /// Spawns something that will not answer for a long time.
    fn a_process_that_never_answers() -> std::process::Child {
        let mut command = if cfg!(windows) {
            // -t pings until killed, and unlike most commands it ignores stdin
            // entirely, so closing stdin cannot end it early.
            let mut c = Command::new("ping");
            c.args(["-t", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn a blocking process")
    }

    #[test]
    fn a_launcher_that_never_answers_is_given_up_on_and_killed() {
        // The failure this prevents: the Windows listener asks from the thread
        // blocked in GetMessage, so an unbounded wait here would freeze every
        // later device arrival behind one wedged binary.
        let started = Instant::now();
        assert_eq!(answer(a_process_that_never_answers(), Duration::from_millis(150)), None);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "gave up after {:?}, which is not giving up",
            started.elapsed()
        );
    }

    #[test]
    fn a_binary_that_answers_with_junk_is_not_a_version() {
        // Exits promptly, prints something that is not an x.y.z.
        let child = Command::new(if cfg!(windows) { "cmd" } else { "echo" })
            .args(if cfg!(windows) {
                vec!["/C", "echo hello"]
            } else {
                vec!["hello"]
            })
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn");
        assert_eq!(answer(child, PROBE_TIMEOUT), None);
    }

    #[test]
    fn a_binary_that_does_not_exist_is_not_a_version() {
        assert_eq!(probe(Path::new("./no-such-launcher-anywhere")), None);
    }
}
