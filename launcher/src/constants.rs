// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every tunable number in the launcher, in one place.
//!
//! Nothing here does anything on its own — each constant is either a default
//! under a `config.toml` setting, a bound the config can't cross, or a timing
//! the config doesn't expose at all. The module each one belongs to is named
//! in its section header.
//!
//! The one deliberate exception is `assets::UI_ASSET_EXTENSIONS`, which stays
//! next to the rust-embed include list it has to mirror: they are two halves of
//! one rule, and splitting them is how they drift apart.

use std::time::Duration;

// ── Window size (window.rs) ──────────────────────────────────────────────
// Each cover wants to be IMAGE_WIDTH_FRACTION of the screen width (its
// "target" size), but never larger than its native pixels. The window then
// wraps the covers on BOTH axes — width = the row of covers + gaps + margins,
// height = one cover + margins — so with no crowding the window is just big
// enough for the covers at target size.
//
// Two caps bound it: the row may not exceed MAX_WIDTH_FRACTION of the screen,
// and the window may not exceed MAX_HEIGHT_FRACTION of the screen. Whichever
// cap bites first sets a single scale (≤1) applied to the covers, so when
// they shrink they shrink on both axes — the width each one contributes
// shrinks by the same factor. The CSS in src/index.html reproduces the same
// fit independently, scaling covers DOWN only.

/// Each cover's target width ≈ this × screen width.
pub const IMAGE_WIDTH_FRACTION: f64 = 0.16;
/// The cover row may not exceed this × screen width.
pub const MAX_WIDTH_FRACTION: f64 = 0.90;
/// The window may not exceed this × screen height.
pub const MAX_HEIGHT_FRACTION: f64 = 0.80;

// Native cover resolution, so a cover is never scaled up past it. Assumes the
// 600x900 (2:3) cover art this launcher is built around.
pub const COVER_NATIVE_WIDTH: f64 = 600.0;
pub const COVER_NATIVE_HEIGHT: f64 = 900.0;

// Used only when the monitor size can't be read (logical pixels).
pub const FALLBACK_WINDOW_W: f64 = 1280.0;
pub const FALLBACK_WINDOW_H: f64 = 800.0;

// ── Look and feel (config.rs) ────────────────────────────────────────────
// Fallbacks for the knobs exposed in config.toml, used when a key is absent or
// unusable. They match the values baked into src/index.html's CSS, so an
// existing install with no new keys renders exactly as before. The seed in
// src/config.toml is free to ship different values — these are the floor under
// a config that doesn't mention a setting at all, not the house style. The two
// spacing values (border gap around the covers and gap between them) are also
// used by the window-sizing math and mirrored as PAD/GAP in src/index.html, so
// the computed size and the CSS layout agree.

pub const DEFAULT_BORDER_GAP: f64 = 36.0; // empty space between window edge and covers
pub const DEFAULT_IMAGE_GAP: f64 = 32.0; // gap between adjacent covers
pub const DEFAULT_CORNER_RADIUS: f64 = 14.0; // cover corner rounding
pub const DEFAULT_BACKGROUND_COLOR: &str = "#1b1229";
pub const DEFAULT_SHADOW_SIZE: f64 = 24.0; // how far the shadow reaches from the cover
pub const DEFAULT_SHADOW_FADE: f64 = 0.0; // solid-color reach before it starts fading
pub const DEFAULT_SHADOW_COLOR: &str = "rgba(0, 0, 0, 0.55)";

// The launch states: a game that failed to start (border + message under its
// cover, still clickable to retry), a game whose exe isn't on the cartridge at
// all (dimmed cover, sign over it, not clickable), and the launch transition
// itself (everything else fades, the screen darkens, dots spin).
pub const DEFAULT_ERROR_BORDER_COLOR: &str = "#e0b13a";
pub const DEFAULT_ERROR_BORDER_WIDTH: f64 = 3.0; // border on a cover that failed to launch
pub const DEFAULT_ERROR_TEXT_COLOR: &str = "#e0b13a";
pub const DEFAULT_MISSING_SIGN_COLOR: &str = "#d13a3a";
pub const DEFAULT_MISSING_DIM: f64 = 0.45; // brightness multiplier for a missing game's cover
pub const DEFAULT_OVERLAY_COLOR: &str = "rgba(0, 0, 0, 0.45)"; // screen dimming during a launch
pub const DEFAULT_LOADING_RING_COLOR: &str = "#ffffff";
pub const DEFAULT_LOADING_TEXT_COLOR: &str = "rgba(255, 255, 255, 0.4)";
pub const DEFAULT_LOADING_RING_SEGMENTS: f64 = 12.0; // pieces the loading ring is cut into
pub const DEFAULT_LOADING_RING_SPEED: f64 = 0.3; // turns per second
/// A floor only against a ring that never moves at all: 0 (or a value rounding
/// to it) would leave it frozen, which says the opposite of what it's there to
/// say. Anything above this is the author's business, however slow.
pub const MIN_LOADING_RING_SPEED: f64 = 0.05;
pub const DEFAULT_LOADING_TEXT_GAP: f64 = 12.0; // ring's bottom edge to the status line

// ── Launch timings (ui.rs) ───────────────────────────────────────────────

/// How long the loading state stays on screen after a launch has *failed*,
/// before the page unwinds it and marks the cover.
///
/// Some failures come back in a few milliseconds — a missing file needs no
/// waiting at all — and a ring that flashes and vanishes reads as a glitch
/// rather than as an attempt that was made and didn't work. Measured from the
/// moment the cover was clicked, so a failure that takes longer than this to
/// arrive is reported the instant it does.
///
/// Handed to the page (in ms) as `__UI__.minLoadingAfterFail`; it is a fixed
/// part of the launcher's feel, not a config.toml setting, so this constant is
/// the only place to change it.
pub const MIN_LOADING_AFTER_FAIL: Duration = Duration::from_millis(1000);

/// Backstop for closing after a *successful* launch. The page normally plays
/// its outro and asks to close; this guarantees the launcher never stays on
/// screen in front of a running game if the page's JS is broken or missing.
pub const LAUNCH_EXIT_FALLBACK: Duration = Duration::from_millis(1200);

// ── Starting a game (launch.rs) ──────────────────────────────────────────

/// How long to wait for the game to put a window on screen before giving up on
/// the question and assuming it's simply slow. A cold-start AAA game off a USB
/// cartridge can genuinely take this long.
pub const WINDOW_WAIT_MS: u32 = 30_000;

/// When we can't ask about a window (non-Windows, or a console program), a
/// process that is still alive after this long counts as started.
pub const LIVENESS_GRACE: Duration = Duration::from_secs(2);

/// How long a process has to stay alive *after* it reported a window before we
/// believe it. `WaitForInputIdle` reports success for a process that has
/// already died, and the exit status isn't necessarily posted yet when it
/// returns — so a game that crashes on startup otherwise reads as "up".
pub const READY_CONFIRM: Duration = Duration::from_millis(400);

/// How often the two windows above check whether the process is still there.
pub const LIVENESS_POLL: Duration = Duration::from_millis(50);

// ── Logs (log.rs) ────────────────────────────────────────────────────────

/// Rewrite `launcher.log` from scratch once it passes this size. Same reasoning
/// as the listener's log: this is a troubleshooting trail, not an audit record.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
