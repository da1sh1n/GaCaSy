// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The window, the webview in it, and everything the two say to each other.
//!
//! This is the only place the halves of the launcher meet: config and catalog
//! go *out* to the page as globals before its own scripts run, clicks come
//! *back* over IPC, and a launch started here reports its outcome through the
//! event loop and into `window.__launchOutcome`.
//!
//! Nothing here blocks. Waiting on a game is [`crate::launch`]'s job, on a
//! worker thread, because the UI thread has an animation to keep painting.

use std::path::Path;
use std::thread;
use std::time::Instant;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows};

use crate::catalog;
use crate::config::Config;
use crate::constants::*;
use crate::window;
use crate::{assets, config, launch};

enum UserEvent {
    CloseRequested,
    /// How a launch ended, on its way from the worker thread in `launch.rs`
    /// back to the page. `ok` means the game is up — the launcher's cue to
    /// close itself; otherwise `message` is the line to put under the cover.
    LaunchOutcome {
        index: usize,
        ok: bool,
        message: String,
    },
}

/// Opens the launcher window and runs until it closes.
pub fn run(base_dir: &Path) -> wry::Result<()> {
    let config = config::load(base_dir);
    let games = catalog::load(base_dir);
    let init_script = init_script(base_dir, &config, &games);

    // WebView2's user-data folder is the engine's only on-disk footprint. We
    // point it at output/ itself, so the engine drops its (fixed-name)
    // EBWebView cache folder straight in there rather than under an extra
    // wrapper. content::ensure_layout pre-creates that folder so it's present
    // from the first launch.
    let mut web_context = WebContext::new(Some(base_dir.to_path_buf()));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let (window_width, window_height) =
        window::size(&event_loop, games.len(), config.image_gap, config.border_gap);
    let show_console_window = config.show_console_window;

    let window = WindowBuilder::new()
        .with_title("Cartridge Launcher")
        // Logical (CSS) pixels, so the size matches the units the page's CSS
        // uses; the OS scales to this monitor's DPI. The page then fits the
        // covers into this size.
        .with_inner_size(LogicalSize::new(window_width, window_height))
        .with_resizable(false)
        .with_decorations(false)
        .with_always_on_top(false)
        .build(&event_loop)
        .expect("failed to create window");

    window::center(&window);
    // Only now, once it is the size and in the place it will stay: raising a
    // window that is still being moved shows the move.
    window::raise(&window);

    let base_for_launch = base_dir.to_path_buf();
    let base_for_protocol = base_dir.to_path_buf();

    let webview = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_custom_protocol("app".into(), move |_webview_id, request| {
            assets::handle_request(&base_for_protocol, request)
        })
        .with_url("app://localhost")
        .with_initialization_script(init_script)
        .with_additional_browser_args(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
             --disable-gpu --disable-extensions --disable-background-networking \
             --disable-backgrounding-occluded-windows --disable-breakpad \
             --disable-component-update --disable-default-apps --disable-sync \
             --no-default-browser-check --renderer-process-limit=1",
        )
        .with_background_color((0, 0, 0, 255))
        .with_ipc_handler(move |request| {
            let body = request.body().as_str();
            if body == "close" {
                let _ = proxy.send_event(UserEvent::CloseRequested);
                return;
            }
            let Some(index) = body.strip_prefix("launch:") else {
                return;
            };
            let Ok(index) = index.parse::<usize>() else {
                return;
            };
            let Some(game) = games.get(index) else {
                return;
            };

            // This runs on the UI thread, so it only starts the process — the
            // waiting (up to WINDOW_WAIT_MS of it) happens on a worker thread,
            // which reports back through the same proxy the close button uses.
            // Blocking here would freeze the very animation that exists to show
            // the player something is happening.
            match launch::spawn(&base_for_launch, game, index, show_console_window) {
                Ok(child) => {
                    let proxy = proxy.clone();
                    let base = base_for_launch.clone();
                    let game = game.clone();
                    thread::spawn(move || {
                        let (ok, message) = match launch::supervise(&base, &game, child) {
                            launch::Outcome::Started => (true, String::new()),
                            launch::Outcome::Failed(message) => (false, message),
                        };
                        let _ = proxy.send_event(UserEvent::LaunchOutcome { index, ok, message });
                    });
                }
                Err(message) => {
                    let _ = proxy.send_event(UserEvent::LaunchOutcome {
                        index,
                        ok: false,
                        message,
                    });
                }
            }
        })
        .build(&window)?;

    // Set once a game is confirmed up: the page is playing its outro and will
    // ask to close, and this is the deadline by which we close regardless.
    let mut exit_deadline: Option<Instant> = None;
    // Latches on the first request to quit. Control flow is decided at the end
    // of every pass, and tao keeps delivering events (window teardown, redraw
    // bookkeeping) after Exit is asked for — without this latch one of those
    // passes overwrites Exit with Wait, and the launcher lives on as a
    // process with no window, still holding the single-instance mutex.
    let mut exiting = false;
    // The end of the grace period window::raise opened. Cleared once it has been
    // acted on, so the drop happens exactly once.
    let mut topmost_until = Some(Instant::now() + TOPMOST_GRACE);

    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(UserEvent::CloseRequested) => exiting = true,
            Event::UserEvent(UserEvent::LaunchOutcome { index, ok, message }) => {
                // The page decides what this looks like: finish the spinner and
                // close on success, or unwind the transition and mark the cover
                // on failure. Guarded with `&&` so a page without the hook (an
                // older index.html) fails silently instead of throwing.
                let script = format!(
                    "window.__launchOutcome && window.__launchOutcome({index}, {ok}, {});",
                    serde_json::Value::String(message)
                );
                let _ = webview.evaluate_script(&script);
                if ok {
                    exit_deadline = Some(Instant::now() + LAUNCH_EXIT_FALLBACK);
                }
            }
            _ => {}
        }

        // Rebuilt on every pass rather than set once: while a deadline is
        // armed the loop has to wake up at it, and the wake-up itself is what
        // acts on it. (`webview` and `window` are both kept alive here for the
        // window's lifetime; `window` is also what the topmost drop needs.)
        if let Some(deadline) = exit_deadline {
            if Instant::now() >= deadline {
                exiting = true;
            }
        }
        if let Some(deadline) = topmost_until {
            if Instant::now() >= deadline {
                window::drop_topmost(&window);
                topmost_until = None;
            }
        }
        *control_flow = if exiting {
            ControlFlow::Exit
        } else {
            // Whichever is sooner: two independent deadlines can be armed at
            // once, and waking at the later one would let the earlier pass
            // unnoticed until some other event happened to arrive.
            match exit_deadline.into_iter().chain(topmost_until).min() {
                Some(deadline) => ControlFlow::WaitUntil(deadline),
                None => ControlFlow::Wait,
            }
        };
    });
}

/// Everything the page needs before its own scripts run, as three globals.
///
/// Handed over this way rather than fetched: `fetch()`ing catalog.json from the
/// page would hit CORS restrictions. serde_json handles escaping, so a color or
/// a game name can hold anything without breaking out of the script.
fn init_script(base_dir: &Path, config: &Config, games: &[catalog::Game]) -> String {
    let ui_settings = serde_json::json!({
        "borderGap": config.border_gap,
        "imageGap": config.image_gap,
        "cornerRadius": config.corner_radius,
        "backgroundColor": config.background_color,
        "shadowSize": config.shadow_size,
        "shadowFade": config.shadow_fade,
        "shadowColor": config.shadow_color,
        "errorBorderColor": config.error_border_color,
        "errorBorderWidth": config.error_border_width,
        "errorTextColor": config.error_text_color,
        "missingSignColor": config.missing_sign_color,
        "missingDim": config.missing_dim,
        "overlayColor": config.overlay_color,
        "loadingRingColor": config.loading_ring_color,
        "loadingTextColor": config.loading_text_color,
        "loadingRingSegments": config.loading_ring_segments,
        "loadingRingSpeed": config.loading_ring_speed,
        "loadingTextGap": config.loading_text_gap,
        // Not a config.toml knob: a timing of the launcher's own, sent along
        // with the look-and-feel so the page reads one object. Milliseconds,
        // because that is what the page's timers take.
        "minLoadingAfterFail": MIN_LOADING_AFTER_FAIL.as_millis() as u64,
    });

    format!(
        "window.__GAMES__ = {}; window.__SHOW_CAPTIONS__ = {}; window.__UI__ = {};",
        catalog::payload(base_dir, games),
        config.show_captions,
        ui_settings
    )
}
