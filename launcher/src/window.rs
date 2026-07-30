// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! How big the window is, and where it sits.
//!
//! The size is decided here, before the page ever loads: no measure-and-report
//! round-trip. The CSS in `src/index.html` then reproduces the same fit
//! independently from the same numbers (the gap and border gap reach it as
//! `GAP`/`PAD`), so the two agree without talking to each other.
//!
//! The fractions and the native cover size live in [`crate::constants`].

use tao::dpi::{PhysicalPosition, Position};

use crate::constants::*;

/// The window size in logical (CSS) pixels — it wraps the covers on both
/// axes, with the covers scaled to satisfy the width and height caps.
///
/// Logical pixels so these numbers, and the gap/margin, are the same units the
/// CSS uses — the computed size and the page layout stay in step at any DPI.
/// (The caps are still true fractions of the screen: the logical screen size
/// is the physical size divided by the same scale factor.)
pub fn size<T>(
    event_loop: &tao::event_loop::EventLoop<T>,
    game_count: usize,
    gap: f64,
    margin: f64,
) -> (f64, f64) {
    // An empty catalog has no covers to wrap, and the math below would size the
    // window for one anyway (`max(1)`) — a tall 2:3 box holding a single line of
    // text. The page shows its empty state instead, so give it the same ordinary
    // window the no-monitor case gets.
    if game_count == 0 {
        return (FALLBACK_WINDOW_W, FALLBACK_WINDOW_H);
    }

    let Some(monitor) = event_loop.primary_monitor() else {
        return (FALLBACK_WINDOW_W, FALLBACK_WINDOW_H);
    };
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let screen_w = size.width as f64 / scale;
    let screen_h = size.height as f64 / scale;
    let n = game_count.max(1) as f64;

    // Target ("unscaled") cover size: the set fraction of screen width, but
    // never larger than the native cover so it's never scaled up.
    let target_w = (IMAGE_WIDTH_FRACTION * screen_w).min(COVER_NATIVE_WIDTH);
    let target_h = target_w * (COVER_NATIVE_HEIGHT / COVER_NATIVE_WIDTH);

    // The largest scale (never above 1) that keeps the row of covers within
    // the width cap and one cover within the height cap. Whichever cap bites
    // hardest wins, and shrinks the covers on both axes together.
    let width_room = MAX_WIDTH_FRACTION * screen_w - (n - 1.0) * gap - 2.0 * margin;
    let height_room = MAX_HEIGHT_FRACTION * screen_h - 2.0 * margin;
    let fit_width = width_room / (n * target_w);
    let fit_height = height_room / target_h;
    let cover_scale = 1.0_f64.min(fit_width).min(fit_height).max(0.0);

    let cover_w = target_w * cover_scale;
    let cover_h = target_h * cover_scale;
    let width = n * cover_w + (n - 1.0) * gap + 2.0 * margin;
    let height = cover_h + 2.0 * margin;
    (width.max(1.0), height.max(1.0))
}

/// Puts the window in the middle of the primary monitor. Physical pixels here,
/// because a screen position is not a CSS length.
pub fn center(window: &tao::window::Window) {
    let Some(monitor) = window.primary_monitor() else {
        return;
    };
    let monitor_size = monitor.size();
    let window_size = window.outer_size();

    let x = (monitor_size.width as i32 - window_size.width as i32) / 2;
    let y = (monitor_size.height as i32 - window_size.height as i32) / 2;

    window.set_outer_position(Position::Physical(PhysicalPosition::new(x, y)));
}

/// Brings the window to the front and pins it there briefly.
///
/// The launcher is not started by the person looking at the screen — the
/// listener spawns it from its message-pump thread when a cartridge arrives —
/// so this process has never had the foreground and never received an input
/// event. Windows' foreground lock refuses `SetForegroundWindow` on that basis
/// and flashes the taskbar button instead, which for a launcher that is
/// supposed to *be* the response to plugging something in is no response at
/// all. `set_focus` is tao's `force_window_active`: it tries the plain call
/// first and only falls back to lifting the lock (a synthesised Alt press, so
/// the process has "received input") if that is refused.
///
/// Topmost is the other half. Focus is a one-off, and it is lost to any window
/// that appears a moment *after* us — which is exactly what an AutoPlay Explorer
/// window does, since it opens off the same device event. Held only for
/// [`TOPMOST_GRACE`] and then dropped by the event loop, so a launcher still on
/// screen later cannot end up hovering over a running game.
pub fn raise(window: &tao::window::Window) {
    window.set_always_on_top(true);
    window.set_focus();
}

/// Ends the [`raise`] grace period, putting the window back in the normal
/// z-order. Separate from `raise` because the wait between them belongs to the
/// event loop, which is the only thing that can wait without blocking the UI.
pub fn drop_topmost(window: &tao::window::Window) {
    window.set_always_on_top(false);
}
