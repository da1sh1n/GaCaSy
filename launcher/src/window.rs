// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Computes the window's size in logical pixels from the game count and the
//! screen's caps, then centres it, rounds its corners and raises it.

// ########## WINDOW SIZE AND PLACEMENT ##########

use tao::dpi::{PhysicalPosition, Position};

use crate::constants::*;

/// The window size in logical (CSS) pixels — a row of covers with one `margin`
/// on every side, the covers scaled to satisfy the screen's caps.
///
/// Logical pixels so these numbers, and the gap/margin, are the same units the
/// CSS uses — the computed size and the page layout stay in step at any DPI.
/// (The caps are still true fractions of the screen: the logical screen size
/// is the physical size divided by the same scale factor.)
///
/// An empty catalog is not a special case here. It gets the floor width like
/// any cartridge with fewer than [`MIN_VISIBLE_COVERS`] games, and the page puts
/// its "no games" message in the middle of it — so a cartridge somebody hasn't
/// filled yet opens the same window as one they have, rather than a differently
/// shaped one that reads as a different kind of failure.
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

    // Target ("unscaled") cover size: the art at its own resolution. The caps
    // below are the only thing that ever takes it off that.
    //
    // This used to be a fraction of the screen width capped at the native size,
    // and the fraction was almost always the binding constraint — on a 2560px
    // display it asked for 410px of a 600px cover and the cap never came into
    // play at all. The rule this module documents is that a cover shrinks for
    // the screen and nothing else; targeting native is what makes that true
    // rather than merely stated.
    let target_w = COVER_NATIVE_WIDTH;
    let target_h = COVER_NATIVE_HEIGHT;

    // What's left of the screen once the caps and the margins are taken out.
    // Two margins on each axis, and nothing else: the toolbar and the name line
    // live inside the top and bottom margins rather than beside them, so the
    // cover row's box is the same distance from all four window edges.
    let width_room = MAX_WIDTH_FRACTION * screen_w - 2.0 * margin;
    let height_room = MAX_HEIGHT_FRACTION * screen_h - 2.0 * margin;

    // The largest scale (never above 1) that fits one cover under the height cap
    // and MIN_VISIBLE_COVERS of them under the width cap. Note what is *not*
    // here: the game count. Covers shrink for a small screen, never for a long
    // catalog.
    let fit_height = height_room / target_h;
    let fit_floor =
        (width_room - (MIN_VISIBLE_COVERS - 1.0) * gap) / (MIN_VISIBLE_COVERS * target_w);
    let cover_scale = 1.0_f64.min(fit_height).min(fit_floor).max(0.0);

    let cover_w = target_w * cover_scale;
    let cover_h = target_h * cover_scale;

    // How many covers stand side by side: every game, but never fewer than the
    // floor and never more than the width cap holds. `fit_floor` above is what
    // guarantees this is at least MIN_VISIBLE_COVERS, so the two clamps can't
    // contradict each other.
    let max_columns = ((width_room + gap) / (cover_w + gap)).floor();
    let columns =
        (game_count as f64).clamp(MIN_VISIBLE_COVERS, max_columns.max(MIN_VISIBLE_COVERS));

    let width = columns * cover_w + (columns - 1.0) * gap + 2.0 * margin;
    let height = cover_h + 2.0 * margin;
    (width.max(1.0), height.max(1.0))
}

/// Rounds the window's corners, by whichever means this Windows has.
///
/// # Why there are two ways
///
/// Windows 11 (build 22000 and up) will do this properly: `DwmSetWindowAttribute`
/// with `DWMWA_WINDOW_CORNER_PREFERENCE` asks the compositor for it, and the
/// compositor draws an anti-aliased corner and keeps the window's shadow. On
/// Windows 10 that attribute does not exist and the call comes back
/// `E_INVALIDARG` — so this is not a preference that is off, it is a feature
/// that isn't there.
///
/// The Windows 10 way is to clip the window to a region. It works, and it is
/// what the launcher falls back to, but a GDI region is a **one-bit mask**: a
/// pixel is inside the window or it isn't, and there is no third answer. The
/// corner is therefore a little stair-stepped there, and no amount of care here
/// changes that.
///
/// # Why the page doesn't draw them instead
///
/// It can't. Rounding them in CSS needs the window behind the page to be
/// transparent, and it isn't: wry hosts WebView2 in *windowed* mode, where the
/// browser lives in a child HWND with no per-pixel alpha, so the page composites
/// onto opaque white however the window and the controller are configured.
/// (Measured, not assumed: a page background at 99% opacity comes out as an
/// exact 0.99 blend with white.) True transparency would need composition
/// hosting, which is a different way of embedding the browser altogether.
///
/// A radius of 0 leaves the window square — the region is cleared rather than
/// replaced, so this is also how somebody turns the feature off.
#[cfg(windows)]
pub fn roundCorners(window: &tao::window::Window, radius: f64) {
    use tao::platform::windows::WindowExtWindows;
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        ClientToScreen, CreateRoundRectRgn, HRGN, SetWindowRgn,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

    let hwnd = window.hwnd() as HWND;
    let square = radius <= 0.0;

    unsafe {
        // Windows 11 first. S_OK (>= 0) means it took, and there is nothing
        // left to do — the compositor owns the corners from here.
        let preference: i32 = if square {
            DWMWCP_DONOTROUND
        } else {
            DWMWCP_ROUND
        };
        let asked = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &preference as *const i32 as *const core::ffi::c_void,
            size_of::<i32>() as u32,
        );
        if asked >= 0 {
            return;
        }

        // Windows 10. Passing a null region clears any previous one, which is
        // what a radius of 0 should do.
        if square {
            SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
            return;
        }

        // The region is measured from the *window* rect, but what wants
        // rounding is the client area — and on Windows 10 an undecorated window
        // still carries an invisible resize border, so the two do not share an
        // origin. Asking for both and taking the difference is exact, where
        // assuming they are the same would round a rectangle that is a few
        // pixels off in each direction.
        let mut window_rect: RECT = std::mem::zeroed();
        let mut client_rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut window_rect) == 0 || GetClientRect(hwnd, &mut client_rect) == 0
        {
            return;
        }
        let mut origin = POINT { x: 0, y: 0 };
        if ClientToScreen(hwnd, &mut origin) == 0 {
            return;
        }
        let left = origin.x - window_rect.left;
        let top = origin.y - window_rect.top;

        // Physical pixels: the config knob is a CSS length like everything else
        // in `config.toml`, and GDI has never heard of those. The doubling is
        // because CreateRoundRectRgn takes the width and height of the ellipse
        // the corner is a quarter of, not its radius.
        let diameter = (radius * window.scale_factor() * 2.0).round() as i32;
        // Right and bottom are exclusive, hence the +1s: without them the
        // window loses its last column and row of pixels.
        let region: HRGN = CreateRoundRectRgn(
            left,
            top,
            left + client_rect.right + 1,
            top + client_rect.bottom + 1,
            diameter,
            diameter,
        );
        if region.is_null() {
            return;
        }
        // The window owns the region now — deleting it here would take the
        // shape with it.
        SetWindowRgn(hwnd, region, 1);
    }
}

/// Nothing to do off Windows: this crate's window rounding is entirely a
/// Win32 affair, and the rest of the launcher builds without it.
#[cfg(not(windows))]
pub fn roundCorners(_window: &tao::window::Window, _radius: f64) {}

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
pub fn dropTopmost(window: &tao::window::Window) {
    window.set_always_on_top(false);
}
