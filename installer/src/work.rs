// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The worker thread, and how the UI hears from it.
//!
//! Two operations in this installer take long enough that doing them on the UI
//! thread would freeze the window: walking a game folder, and copying one. Both
//! go through here.
//!
//! egui only repaints when something asks it to, so every message is followed by
//! a `request_repaint`. Without that the progress bar would advance only when the
//! mouse moved — the classic "it looks frozen but isn't" bug in an immediate-mode
//! UI over a background thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, mpsc};
use std::thread;

use crate::cartridge::Progress;
use crate::detect;

enum Update {
    Progress(Progress),
    Done(Result<Vec<String>, String>),
}

/// A running background operation, polled once per frame.
pub struct Job {
    updates: Receiver<Update>,
    cancel: Arc<AtomicBool>,
    /// Headline: "Writing the cartridge".
    pub title: String,
    /// The current step, changing several times a second during a copy.
    pub label: String,
    /// 0..=1, or `None` before the first progress arrives.
    pub fraction: Option<f32>,
    /// `Some` once it has finished, whether it worked or not.
    pub outcome: Option<Result<Vec<String>, String>>,
    /// Whether a Cancel button is worth offering. False for the jobs that are
    /// over in a moment — a cancel that can only land after the work is done is
    /// a button that lies.
    pub cancellable: bool,
}

impl Job {
    /// Starts `task` on its own thread.
    ///
    /// The task is handed a cancel flag to check and a reporter to call; both
    /// are the only channels it has to the UI, which is what keeps every
    /// long-running operation in this program shaped the same way.
    pub fn spawn<F>(ctx: &egui::Context, title: impl Into<String>, task: F) -> Job
    where
        F: FnOnce(&AtomicBool, &mut dyn FnMut(Progress)) -> Result<Vec<String>, String>
            + Send
            + 'static,
    {
        let (sender, updates) = channel();
        let cancel = Arc::new(AtomicBool::new(false));

        let flag = cancel.clone();
        let ctx = ctx.clone();
        let progress_sender = sender.clone();
        thread::spawn(move || {
            let mut report = |progress| {
                let _ = progress_sender.send(Update::Progress(progress));
                ctx.request_repaint();
            };
            let result = task(&flag, &mut report);
            let _ = sender.send(Update::Done(result));
            ctx.request_repaint();
        });

        Job {
            updates,
            cancel,
            title: title.into(),
            label: "Starting…".into(),
            fraction: None,
            outcome: None,
            cancellable: true,
        }
    }

    pub fn uncancellable(mut self) -> Job {
        self.cancellable = false;
        self
    }

    /// Drains whatever the worker has sent since the last frame.
    pub fn poll(&mut self) {
        loop {
            match self.updates.try_recv() {
                Ok(Update::Progress(progress)) => {
                    self.fraction = Some((progress.done as f64 / progress.total.max(1) as f64) as f32);
                    self.label = progress.label;
                }
                Ok(Update::Done(result)) => self.outcome = Some(result),
                // Disconnected without a result means the worker panicked. Say
                // so rather than showing a progress bar that never moves again.
                Err(TryRecvError::Disconnected) if self.outcome.is_none() => {
                    self.outcome = Some(Err("The installer's worker stopped unexpectedly.".into()));
                    return;
                }
                Err(_) => return,
            }
        }
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelling(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn finished(&self) -> bool {
        self.outcome.is_some()
    }
}

/// Walks one game folder in the background — see [`detect::scan`].
///
/// Returned as a bare receiver rather than a [`Job`]: a scan has no progress
/// worth showing and no result but its own, and several run at once when the
/// user adds several folders in a row.
pub struct Scanning {
    result: Receiver<detect::Scan>,
    cancel: Arc<AtomicBool>,
}

impl Scanning {
    pub fn start(ctx: &egui::Context, folder: PathBuf) -> Scanning {
        let (sender, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(detect::scan(&folder, &flag));
            ctx.request_repaint();
        });
        Scanning { result, cancel }
    }

    /// The finished scan, once there is one.
    pub fn take(&self) -> Option<detect::Scan> {
        self.result.try_recv().ok()
    }
}

impl Drop for Scanning {
    /// Removing a game while its folder is still being walked must not leave a
    /// thread grinding through a 100 GB install for nothing.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

// `LauncherProbe` was here: a background thread that ran a cartridge's own
// `launcher.exe --version` and reported what it printed. It is gone, and not
// because it was slow. The version it went looking for is inside the signature
// the file already carries, so the thread existed to execute an arbitrary binary
// off a stranger's USB stick in order to learn something the installer could
// simply read. See `../src/volume.rs::attested_launcher`.
