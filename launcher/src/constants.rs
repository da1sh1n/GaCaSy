// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Every tunable number in the crate: the defaults behind `config.toml`
//! settings, the bounds a config cannot cross, and the timings it does not
//! expose. Section headers name the module each belongs to.

// ########## TUNABLE NUMBERS ##########

use std::time::Duration;

// ── Window size (window.rs) ──────────────────────────────────────────────
// Each cover wants to be its own native size. The window then wraps a row of
// them: one margin on every side and nothing else. The cover row's top edge is
// `border_gap` from the top of the window, its bottom edge `border_gap` from
// the bottom, its sides `border_gap` from the sides.
//
// The toolbar and the name line take no height of their own — they sit INSIDE
// the top and bottom margins, pinned to the window rather than to the covers.
// That is what makes the row's box symmetric, and it makes `border_gap` the one
// knob for every piece of spacing in the window. See src/style.css, which
// places both of them.
//
// How many games there are decides how WIDE the window is, and nothing else.
// The row holds every game up to what MAX_WIDTH_FRACTION of the screen allows,
// and never fewer than MIN_VISIBLE_COVERS; past that the page scrolls the row
// sideways. A long catalog therefore costs scrolling, not cover size — the
// covers a player can see are always the size they were meant to be.
//
// A cover only shrinks for the screen: MAX_HEIGHT_FRACTION when the display is
// too short for one at native size, and the MIN_VISIBLE_COVERS floor when it is
// too narrow for three. Whichever bites hardest sets a single scale (≤1) applied
// to every cover on both axes. The CSS in src/style.css reproduces the same
// fit independently, from the window height alone, scaling covers DOWN only.
//
// There used to be an IMAGE_WIDTH_FRACTION here — each cover asked for a
// fraction of the screen width, capped at native. It is gone because it was
// almost always the binding constraint rather than the cap: at 0.16 a 2560px
// display asked for 410px of a 600px cover, so the art was never once shown at
// the size it was drawn at, on any display, and the "shrinks only for the
// screen" rule above was not true.

/// The cover row may not exceed this × screen width.
pub const MAX_WIDTH_FRACTION: f64 = 0.90;
/// The window may not exceed this × screen height.
pub const MAX_HEIGHT_FRACTION: f64 = 0.80;

/// The window is never narrower than this many covers, however few games the
/// cartridge holds — so zero, one and two games all open the same window, and
/// there is always room for the toolbar across the top of it. A screen too
/// narrow to hold this many at target size shrinks the covers until it can.
pub const MIN_VISIBLE_COVERS: f64 = 3.0;

// Native cover resolution, so a cover is never scaled up past it. Assumes the
// 600x900 (2:3) cover art this launcher is built around.
pub const COVER_NATIVE_WIDTH: f64 = 600.0;
pub const COVER_NATIVE_HEIGHT: f64 = 900.0;

// Used only when the monitor size can't be read (logical pixels).
pub const FALLBACK_WINDOW_W: f64 = 1280.0;
pub const FALLBACK_WINDOW_H: f64 = 800.0;

// ── Look and feel (config.rs) ────────────────────────────────────────────
// Fallbacks for the knobs exposed in config.toml, used when a key is absent or
// unusable. They match the values baked into src/style.css, so both sides of
// the page agree without talking to each other. The seed in src/config.toml is
// free to ship different values — these are the floor under a config that
// doesn't mention a setting at all, not the house style. The two spacing values
// (border gap around the covers and gap between them) are also used by the
// window-sizing math and mirrored as PAD/GAP in src/theme.js, so the computed
// size and the CSS layout agree.
//
// DEFAULT_BORDER_GAP is 56 rather than the 36 it was, because the margin now
// has to hold the toolbar above the covers and the name line below them. The
// bottom furniture takes a fixed 42px, so a gap under 42 puts the name line on
// the art and one under 30 puts the toolbar on it. Nothing clamps that — a
// cartridge is free to ask for a layout that doesn't work.

// ── The palette: 60 / 30 / 10 ────────────────────────────────────────────
// Three colors carry the whole launcher, in the proportions the rule is named
// for. Everything that is not cover art, and not one of the two semantic
// states below, is one of these or a shade worked out from them.
//
//   primary    60%  the window behind everything
//   secondary  30%  shadows, borders, the plate behind missing art
//   accent     10%  text, the selected cover, the close button
//
// Two shades are derived rather than used raw, and both for measured reasons —
// see the ramp in src/app.js. The accent at full strength is a fine FILL (the
// close square, the active order segment) but fails WCAG AA as body text on a
// dark primary; text therefore uses it lifted toward the light. Secondary at
// full strength is nearly invisible as a hairline on primary, so borders use it
// lifted toward the accent. Set the three, get a coherent launcher.
pub const DEFAULT_PRIMARY_COLOR: &str = "#191325"; // violet
pub const DEFAULT_SECONDARY_COLOR: &str = "#3D1F37"; // plum
pub const DEFAULT_ACCENT_COLOR: &str = "#925E37"; // caramel

pub const DEFAULT_BORDER_GAP: f64 = 56.0; // margin on every side of the cover row
pub const DEFAULT_IMAGE_GAP: f64 = 32.0; // gap between adjacent covers
pub const DEFAULT_CORNER_RADIUS: f64 = 14.0; // cover corner rounding
/// The window's own corners, in logical pixels — used only where Windows won't
/// choose for us. Windows 11 rounds the window itself and picks its own radius;
/// this is the Windows 10 fallback's, where the launcher does the clipping. See
/// [`crate::window::roundCorners`], which is also where the reason the page
/// doesn't draw these is written down.
pub const DEFAULT_WINDOW_CORNER_RADIUS: f64 = 12.0;
pub const DEFAULT_SHADOW_SIZE: f64 = 24.0; // how far the shadow reaches from the cover
pub const DEFAULT_SHADOW_FADE: f64 = 0.0; // solid-color reach before it starts fading

// `background_color` and `shadow_color` have no defaults here on purpose. They
// are the two keys the palette above replaced, and `config::load` reads them
// straight into primary and secondary when a cartridge still names them — so
// they need no fallback of their own, and giving them one would make "unset"
// indistinguishable from "set to the old value".

// The launch states: a game that failed to start (border + message under its
// cover, still clickable to retry), a game whose exe isn't on the cartridge at
// all (veiled cover, sign over it, not clickable), and the launch transition
// itself (everything else fades, the screen darkens, a progress line sweeps
// under the chosen cover).
pub const DEFAULT_ERROR_BORDER_COLOR: &str = "#e0b13a";
pub const DEFAULT_ERROR_BORDER_WIDTH: f64 = 3.0; // border on a cover that failed to launch
pub const DEFAULT_ERROR_TEXT_COLOR: &str = "#e0b13a";
pub const DEFAULT_MISSING_SIGN_COLOR: &str = "#d13a3a";
pub const DEFAULT_MISSING_DIM: f64 = 0.45; // brightness multiplier for a missing game's cover
pub const DEFAULT_OVERLAY_COLOR: &str = "rgba(0, 0, 0, 0.45)"; // screen dimming during a launch
/// The progress line under the cover of a game that's starting. Blank derives
/// it from the accent, like the rest of the chrome.
///
/// Named for a spinning ring it no longer draws. The name stays: renaming it
/// would silently revert every cartridge whose config already sets it.
pub const DEFAULT_LOADING_RING_COLOR: &str = "";
pub const DEFAULT_LOADING_TEXT_COLOR: &str = "";
pub const DEFAULT_LOADING_TEXT_GAP: f64 = 12.0; // hairline's bottom edge to the status line

// The toolbar across the top (order control, arrange toggle, search box) and
// the scrollbar under a row too long to fit. Both are chrome rather than
// content: one color each, deliberately, so they can be tuned separately
// without becoming a theming system.
//
// Blank means "work it out from the three above" — the toolbar from the accent,
// the scrollbar from the secondary. Naming either still wins, which is the part
// that matters; blank is just the answer that stays right when somebody changes
// the palette out from under them.
pub const DEFAULT_TOOLBAR_COLOR: &str = "";
pub const DEFAULT_SCROLLBAR_COLOR: &str = "";

// The ring drawn in place of the system pointer. Blank leaves it to the page,
// which draws it white or black from the brightness of the art under it; a named
// color pins it to that instead, on every cover.
pub const DEFAULT_CURSOR_COLOR: &str = "";

// ── The moving background (backdrop.js) ──────────────────────────────────

/// The three the page knows how to draw. A value outside this list leaves
/// `background_effect` at its default rather than picking one arbitrarily —
/// see [`crate::config`], which follows the same rule for `order_mode`.
pub const BACKGROUND_EFFECTS: [&str; 3] = ["simple", "particles", "fog"];

/// "simple" — a flat field — because it is the option that changes nothing.
/// A cartridge written before this key existed gets exactly the launcher it
/// had, and the shipped config.toml is where the effect is actually turned on.
pub const DEFAULT_BACKGROUND_EFFECT: &str = "simple";

/// Blank derives the four tints from the secondary and the accent, the same
/// way the toolbar and the scrollbar derive theirs. See the ramp in
/// src/theme.js for why one color is asked for rather than four.
pub const DEFAULT_BACKGROUND_EFFECT_COLOR: &str = "";

/// How opaque a cover is while it is not the one being pointed at, so the
/// field behind shows through. A plain opacity: 1 is solid, 0 invisible.
pub const DEFAULT_COVER_OPACITY: f64 = 0.9;

// ── Cover order (order.rs, config.rs) ────────────────────────────────────

/// What `order_mode` is when the config doesn't say — see [`crate::order::MODES`]
/// for the four it can hold. "usage" because the cover a player wants next is
/// most often the one they had last, and until anything has been played it is
/// indistinguishable from plain catalog order.
pub const DEFAULT_ORDER_MODE: &str = "usage";

// ── Launch timings (ui.rs) ───────────────────────────────────────────────

/// How long the loading state stays on screen after a launch has *failed*,
/// measured from the click, before the page unwinds it and marks the cover.
///
/// Some failures come back in milliseconds, and a ring that flashes and
/// vanishes reads as a glitch rather than as an attempt that did not work.
/// Handed to the page as `__UI__.minLoadingAfterFail`; not a config setting.
pub const MIN_LOADING_AFTER_FAIL: Duration = Duration::from_millis(1000);

/// Backstop for closing after a *successful* launch. The page normally plays
/// its outro and asks to close; this guarantees the launcher never stays on
/// screen in front of a running game if the page's JS is broken or missing.
pub const LAUNCH_EXIT_FALLBACK: Duration = Duration::from_millis(1200);

/// How long the window stays pinned above everything else after it opens.
///
/// Long enough to outlast anything else the same device event set off — an
/// AutoPlay Explorer window is the one that matters, and it takes a moment to
/// appear — and short enough that it is over before the player has finished
/// looking at the covers. See [`crate::window::raise`].
pub const TOPMOST_GRACE: Duration = Duration::from_millis(1500);

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
