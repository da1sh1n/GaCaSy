// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Starting a game, and waiting for it to actually come up.
//!
//! "Started" deliberately means *the game's window is up*, not "spawn returned
//! Ok" — the launcher closes itself on that signal, and closing while the game
//! is still an invisible process makes a working launch look like a broken one.
//! On Windows that signal is `WaitForInputIdle`; everywhere else (and for
//! console programs, which `WaitForInputIdle` refuses) it degrades to "still
//! alive a moment later".
//!
//! Every attempt and its outcome goes to `logs/launcher.log` with the full OS
//! error text — the UI only ever gets one short sentence. That, and the game's
//! own redirected output, is [`crate::log`]'s side of the job. The timings are
//! in [`crate::constants`].

use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::Instant;

use crate::catalog::Game;
use crate::constants::*;
use crate::log;

/// What became of one launch, as far as the player is concerned.
pub enum Outcome {
    /// The game is up. The launcher's cue to close itself.
    Started,
    /// It isn't, and this is the one short line to put under its cover. The
    /// long version is already in `logs/launcher.log`.
    Failed(String),
}

/// Starts `game`'s exe.
///
/// The working directory is the exe's **own folder**, not the cartridge root:
/// games overwhelmingly resolve their assets relative to themselves, and a
/// game started from the wrong cwd fails in ways that look like corruption.
///
/// On success the caller owns the [`Child`] and should hand it to
/// [`supervise`] on a worker thread — everything from here on blocks.
pub fn spawn(base: &Path, game: &Game, index: usize) -> Result<Child, String> {
    let exe = base.join(&game.exe);
    log::line(base, &format!("launching {} ({})", game.name, exe.display()));

    // Checked again here even though the catalog was screened at startup: the
    // cartridge is removable, and the file may be gone since.
    if !exe.is_file() {
        log::line(base, &format!("FAILED {}: no such file", exe.display()));
        return Err("Failed to start — game files missing".to_string());
    }

    // The exe's parent always exists here (base.join of a relative path), but
    // fall back to the cartridge root rather than refusing to launch.
    let workdir = exe.parent().unwrap_or(base).to_path_buf();
    let (stdout, stderr) = log::game_output(base, game, index);

    match Command::new(&exe)
        .current_dir(&workdir)
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
    {
        Ok(child) => {
            log::line(
                base,
                &format!("started pid {} in {}", child.id(), workdir.display()),
            );
            Ok(child)
        }
        Err(e) => {
            log::line(base, &format!("FAILED {}: {e}", exe.display()));
            Err(format!("Failed to start — {}", short_reason(&e)))
        }
    }
}

/// Blocks until the game is up (or clearly isn't). Call on a worker thread —
/// never on the UI thread, which has a window to keep repainting.
pub fn supervise(base: &Path, game: &Game, mut child: Child) -> Outcome {
    match wait_for_window(&child) {
        // A window came up — but hold on briefly before believing it. A game
        // that flashes an error box and quits satisfies WaitForInputIdle just
        // as well as one that is genuinely running, and so does one that was
        // already dead when it was asked.
        Window::Ready => match outlives(&mut child, READY_CONFIRM) {
            None => {
                log::line(base, &format!("{} is up", game.name));
                Outcome::Started
            }
            Some(status) => finished_early(base, game, status),
        },
        Window::TimedOut => {
            // Not a failure. Saying otherwise would punish slow games, and the
            // player can see for themselves whether one is coming up.
            log::line(
                base,
                &format!(
                    "{} has no window yet after {WINDOW_WAIT_MS}ms; assuming slow start",
                    game.name
                ),
            );
            Outcome::Started
        }
        // WaitForInputIdle refuses non-GUI processes, and there's nothing to
        // ask on other platforms. Fall back to plain survival.
        Window::Unsupported => match outlives(&mut child, LIVENESS_GRACE) {
            None => {
                log::line(base, &format!("{} is running", game.name));
                Outcome::Started
            }
            Some(status) => finished_early(base, game, status),
        },
    }
}

/// A game that was gone before we ever reported it as started.
fn finished_early(base: &Path, game: &Game, status: std::process::ExitStatus) -> Outcome {
    // Exit code 0 in the first couple of seconds is odd but not an error — a
    // stub that hands off to another process does exactly this.
    if status.success() {
        log::line(base, &format!("{} exited immediately, cleanly", game.name));
        return Outcome::Started;
    }
    // The exit code goes to the log, not under the cover: it means nothing to
    // a player, and a line long enough to wrap is worse than a short one.
    log::line(
        base,
        &format!("FAILED {}: exited immediately ({status})", game.name),
    );
    Outcome::Failed("Failed to start — closed immediately".to_string())
}

/// `Some(status)` if the process has already exited, `None` if it's running.
/// A wait error is treated as "running": the game is not the thing at fault.
fn exited(child: &mut Child) -> Option<std::process::ExitStatus> {
    child.try_wait().ok().flatten()
}

/// Polls for `window`, returning early with the exit status if the process dies
/// inside it and `None` if it's still alive at the end.
fn outlives(child: &mut Child, window: std::time::Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + window;
    loop {
        if let Some(status) = exited(child) {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(LIVENESS_POLL);
    }
}

enum Window {
    /// The process is up and waiting for input — its window exists.
    Ready,
    /// Still no window after [`WINDOW_WAIT_MS`].
    TimedOut,
    /// The question doesn't apply here (console program, or not Windows).
    Unsupported,
}

/// Waits for the process to finish initialising and start pumping messages,
/// which in practice means "its first window is on screen".
#[cfg(windows)]
fn wait_for_window(child: &Child) -> Window {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::WaitForInputIdle;

    // Documented return values. Spelled out rather than imported because
    // WaitForInputIdle's success value (0) has no name of its own.
    const WAIT_TIMEOUT: u32 = 0x0000_0102;

    let handle = child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    match unsafe { WaitForInputIdle(handle, WINDOW_WAIT_MS) } {
        0 => Window::Ready,
        WAIT_TIMEOUT => Window::TimedOut,
        // WAIT_FAILED — in practice ERROR_NOT_GUI_PROCESS, i.e. a console
        // program. Not an error about the game, just the wrong question.
        _ => Window::Unsupported,
    }
}

#[cfg(not(windows))]
fn wait_for_window(_child: &Child) -> Window {
    Window::Unsupported
}

/// Maps an OS spawn error to something worth showing a player. Anything beyond
/// the two everyday causes points at the log rather than guessing.
fn short_reason(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "file not found",
        std::io::ErrorKind::PermissionDenied => "access denied",
        _ => "see logs/launcher.log",
    }
}
