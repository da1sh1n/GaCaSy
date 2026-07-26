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
