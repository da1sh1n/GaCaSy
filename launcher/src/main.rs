// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// Cartridge launcher shell.
//
// The whole UI (HTML/CSS/JS) is baked into this exe with rust-embed and
// served over an `app://` custom protocol straight out of memory — there
// is no bundled web server and nothing ever listens on a port.
//
// Everything the exe reads from disk is cartridge *content* that lives
// beside it in `output/`:
//
//   output/
//     launcher.exe   <- this program
//     config.toml    <- seeded from the baked-in default if missing
//     catalog.json   <- the game list (name / exe / image), seeded likewise
//     images/        <- 600x900 cover art, dropped in by hand
//     games/         <- the actual game installs
//     EBWebView/     <- WebView2's own data folder (its only on-disk crumbs)
//
// `cargo run` builds into target/ and runs in place, resolving `output/`
// as its content directory and refreshing `output/launcher.exe` so the
// shippable copy stays current. It does not relaunch itself, so there is
// exactly one launcher process (WebView2 still spawns its own renderer
// process — that is inherent to the engine and cannot be merged away).
//
// config.toml, catalog.json, images/ and games/ are never overwritten once
// present, so hand-dropped content survives every build.
//
// Window sizing (see the `Window size` constants below): the window wraps the
// covers on both axes — covers aim for a fraction of the screen width and the
// window is just big enough for them plus margins — but two caps (max width
// and max height fraction of the screen) shrink the covers to fit when they'd
// be too big. Rust picks the window size and the CSS in src/index.html fits
// the covers into it; the shared border/image gap numbers (from config.toml,
// mirrored as PAD/GAP in the page) keep the two in step.
//
// No console window: this is a GUI app, not a CLI tool.
#![windows_subsystem = "windows"]

use std::borrow::Cow;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rust_embed::RustEmbed;
use serde::Deserialize;
use tao::dpi::{LogicalSize, PhysicalPosition, Position};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::http::{header::CONTENT_TYPE, Request, Response};
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows};

/// The UI assets, baked into the exe at compile time and served over the
/// `app://` protocol at runtime. They live in `src/` beside the Rust source,
/// so the include list keeps main.rs and the seed files (config.toml,
/// catalog.json) out of the bundle — `is_ui_asset` mirrors it at runtime.
#[derive(RustEmbed)]
#[folder = "src/"]
#[include = "*.html"]
#[include = "*.css"]
#[include = "*.js"]
struct UiAssets;

/// Extensions the `app://` protocol will serve as UI assets. Mirrors the
/// rust-embed include list above so the live-from-`src/` dev path can't hand
/// out main.rs or the seed files.
const UI_ASSET_EXTENSIONS: [&str; 3] = ["html", "css", "js"];

// ── Window size ──────────────────────────────────────────────────────────
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
const IMAGE_WIDTH_FRACTION: f64 = 0.16; // each cover's target width ≈ this × screen width
const MAX_WIDTH_FRACTION: f64 = 0.90; // the cover row may not exceed this × screen width
const MAX_HEIGHT_FRACTION: f64 = 0.80; // the window may not exceed this × screen height
// Native cover resolution, so a cover is never scaled up past it. Assumes the
// 600x900 (2:3) cover art this launcher is built around.
const COVER_NATIVE_WIDTH: f64 = 600.0;
const COVER_NATIVE_HEIGHT: f64 = 900.0;
// Defaults for the look-and-feel knobs exposed in config.toml. They match the
// values baked into src/index.html's CSS, so an existing install with no new
// keys renders exactly as before. The two spacing values (border gap around
// the covers and gap between them) are also used by the window-sizing math
// below and mirrored as PAD/GAP in src/index.html, so the computed size and the
// CSS layout agree — see load_config / window_size.
const DEFAULT_BORDER_GAP: f64 = 36.0; // empty space between window edge and covers
const DEFAULT_IMAGE_GAP: f64 = 32.0; // gap between adjacent covers
const DEFAULT_CORNER_RADIUS: f64 = 14.0; // cover corner rounding
const DEFAULT_BACKGROUND_COLOR: &str = "#1b1229";
const DEFAULT_SHADOW_SIZE: f64 = 24.0; // how far the shadow reaches from the cover
const DEFAULT_SHADOW_FADE: f64 = 0.0; // solid-color reach before it starts fading
const DEFAULT_SHADOW_COLOR: &str = "rgba(0, 0, 0, 0.55)";
// Used only when the monitor size can't be read (logical pixels).
const FALLBACK_WINDOW_W: f64 = 1280.0;
const FALLBACK_WINDOW_H: f64 = 800.0;
// ─────────────────────────────────────────────────────────────────────────

/// Baked-in defaults so a fresh `output/` can be seeded with no repo around
/// (e.g. on a real cartridge).
const DEFAULT_CONFIG: &str = include_str!("config.toml");
const DEFAULT_CATALOG: &str = include_str!("catalog.json");

#[derive(Deserialize, Clone)]
#[allow(dead_code)] // `name` and `image` are only read on the JS side.
struct Game {
    name: String,
    exe: String,
    image: String,
}

struct Config {
    show_captions: bool,
    // Look-and-feel knobs, all read from config.toml (with the DEFAULT_*
    // fallbacks above). Numeric values are CSS pixels; colors are any CSS
    // color string. border_gap and image_gap also feed the window-sizing math.
    border_gap: f64,
    image_gap: f64,
    corner_radius: f64,
    background_color: String,
    shadow_size: f64,
    shadow_fade: f64,
    shadow_color: String,
}

enum UserEvent {
    CloseRequested,
}

fn main() -> wry::Result<()> {
    let base_dir = resolve_base_dir();
    ensure_layout(&base_dir);

    // Single-instance is enforced only for the shipped launcher (the exe in
    // output/). Under `cargo run` it is deliberately skipped so a rebuild
    // always opens a fresh window instead of silently exiting when an older
    // run is still on screen holding the lock — the classic "my change did
    // nothing" trap during development. Nothing listens on a port: the guard
    // is a named mutex the OS releases when the process dies.
    let _instance = if running_deployed() {
        match acquire_single_instance() {
            Some(guard) => Some(guard),
            None => return Ok(()),
        }
    } else {
        None
    };

    run_app(&base_dir)
}

/// True when this is the deployed launcher (its exe sits in `output/`) rather
/// than a `cargo run` build out of `target/`.
fn running_deployed() -> bool {
    let Ok(exe) = env::current_exe() else {
        return false;
    };
    exe.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some("output")
}

/// The folder holding `launcher.exe` and all cartridge content.
///
/// When the deployed exe runs (its parent folder is named `output`), that
/// folder is the base. Under `cargo run` the exe lives in target/, so the
/// base is the repo's own `output/`.
fn resolve_base_dir() -> PathBuf {
    if running_deployed() {
        let exe = env::current_exe().expect("failed to resolve current exe path");
        exe.parent()
            .expect("current exe has no parent directory")
            .to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("output")
    }
}

/// Creates the content folders, seeds config.toml/catalog.json if they're
/// missing, and refreshes the deployed exe. Existing content is never
/// touched, so hand-dropped covers, games and edits survive every build.
fn ensure_layout(base: &Path) {
    for sub in ["games", "images", "EBWebView"] {
        fs::create_dir_all(base.join(sub))
            .unwrap_or_else(|e| panic!("failed to create output/{sub}/: {e}"));
    }
    seed_if_missing(&base.join("config.toml"), DEFAULT_CONFIG);
    seed_if_missing(&base.join("catalog.json"), DEFAULT_CATALOG);
    refresh_deployed_exe(base);
}

fn seed_if_missing(path: &Path, contents: &str) {
    if !path.exists() {
        fs::write(path, contents)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
    }
}

/// Copies the freshly built exe to `output/launcher.exe` so the shippable
/// copy tracks the source. Skipped when we already are that copy; failure
/// (e.g. a deployed instance holding the file open) is non-fatal.
fn refresh_deployed_exe(base: &Path) {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let dst = base.join("launcher.exe");
    if let (Ok(a), Ok(b)) = (exe.canonicalize(), dst.canonicalize()) {
        if a == b {
            return;
        }
    }
    let _ = fs::copy(&exe, &dst);
}

/// Holds the process-wide single-instance mutex; releasing it (on drop or
/// process exit) frees the name for the next launch.
#[cfg(windows)]
struct InstanceGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }
}

/// Returns `Some(guard)` if this is the first instance, `None` if another
/// is already running. Uses a named mutex rather than a socket so nothing
/// binds a port.
#[cfg(windows)]
fn acquire_single_instance() -> Option<InstanceGuard> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Local\\GaCaSy.CartridgeLauncher\0".encode_utf16().collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if handle.is_null() {
            // Couldn't create the mutex at all; don't block launching.
            return Some(InstanceGuard(handle));
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            windows_sys::Win32::Foundation::CloseHandle(handle);
            return None;
        }
        Some(InstanceGuard(handle))
    }
}

#[cfg(not(windows))]
struct InstanceGuard;

#[cfg(not(windows))]
fn acquire_single_instance() -> Option<InstanceGuard> {
    Some(InstanceGuard)
}

/// The window size in logical (CSS) pixels — it wraps the covers on both
/// axes, with the covers scaled to satisfy the width and height caps.
///
/// Logical pixels so these numbers, and the gap/margin, are the same units the
/// CSS uses — the computed size and the page layout stay in step at any DPI.
/// (The caps are still true fractions of the screen: the logical screen size
/// is the physical size divided by the same scale factor.)
fn window_size<T>(
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

/// Reads config.toml (already seeded by `ensure_layout`). Unknown keys and
/// unusable values are ignored, leaving that setting at its default, so an
/// older config.toml (or a typo in one value) still yields a usable launcher.
fn load_config(base_dir: &Path) -> Config {
    let mut config = Config {
        show_captions: false,
        border_gap: DEFAULT_BORDER_GAP,
        image_gap: DEFAULT_IMAGE_GAP,
        corner_radius: DEFAULT_CORNER_RADIUS,
        background_color: DEFAULT_BACKGROUND_COLOR.to_string(),
        shadow_size: DEFAULT_SHADOW_SIZE,
        shadow_fade: DEFAULT_SHADOW_FADE,
        shadow_color: DEFAULT_SHADOW_COLOR.to_string(),
    };

    let Ok(contents) = fs::read_to_string(base_dir.join("config.toml")) else {
        return config;
    };
    // Read as a plain table, key by key, rather than deserialized into a
    // struct: one wrong-typed value then costs only that setting instead of
    // rejecting the whole file. A file that isn't valid TOML at all is the one
    // case that falls back to every default.
    let Ok(table) = contents.parse::<toml::Table>() else {
        return config;
    };

    if let Some(value) = table.get("show_captions").and_then(|v| v.as_bool()) {
        config.show_captions = value;
    }
    set_f64(&mut config.border_gap, table.get("border_gap"));
    set_f64(&mut config.image_gap, table.get("image_gap"));
    set_f64(&mut config.corner_radius, table.get("corner_radius"));
    set_color(&mut config.background_color, table.get("background_color"));
    set_f64(&mut config.shadow_size, table.get("shadow_size"));
    set_f64(&mut config.shadow_fade, table.get("shadow_fade"));
    set_color(&mut config.shadow_color, table.get("shadow_color"));

    config
}

/// Overwrites `slot` only if `value` is a non-negative TOML number (written
/// either as an integer or a float), so a missing or garbled entry leaves the
/// default in place rather than zeroing it.
fn set_f64(slot: &mut f64, value: Option<&toml::Value>) {
    let parsed = match value {
        Some(toml::Value::Integer(n)) => *n as f64,
        Some(toml::Value::Float(f)) => *f,
        _ => return,
    };
    if parsed.is_finite() && parsed >= 0.0 {
        *slot = parsed;
    }
}

/// Overwrites `slot` only if `value` is a non-blank string, so a missing,
/// empty or wrong-typed entry keeps the default color.
fn set_color(slot: &mut String, value: Option<&toml::Value>) {
    if let Some(color) = value.and_then(|v| v.as_str()) {
        if !color.trim().is_empty() {
            *slot = color.trim().to_string();
        }
    }
}

fn run_app(base_dir: &Path) -> wry::Result<()> {
    let config = load_config(base_dir);

    let catalog_path = base_dir.join("catalog.json");
    let catalog_json = fs::read_to_string(&catalog_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", catalog_path.display()));
    let games: Vec<Game> = serde_json::from_str(&catalog_json)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", catalog_path.display()));

    // Hands the parsed game list and config to the page as globals before
    // its own scripts run, since fetching catalog.json via `fetch()` would
    // hit CORS restrictions. The page lays itself out responsively to fill
    // whatever window size we pick below.
    // Pack the look-and-feel knobs into one object the page reads before its
    // own scripts run. serde_json handles escaping the color strings safely.
    let ui_settings = serde_json::json!({
        "borderGap": config.border_gap,
        "imageGap": config.image_gap,
        "cornerRadius": config.corner_radius,
        "backgroundColor": config.background_color,
        "shadowSize": config.shadow_size,
        "shadowFade": config.shadow_fade,
        "shadowColor": config.shadow_color,
    });
    let init_script = format!(
        "window.__GAMES__ = {catalog_json}; window.__SHOW_CAPTIONS__ = {}; window.__UI__ = {};",
        config.show_captions, ui_settings
    );

    // WebView2's user-data folder is the engine's only on-disk footprint. We
    // point it at output/ itself, so the engine drops its (fixed-name)
    // EBWebView cache folder straight in there rather than under an extra
    // wrapper. ensure_layout pre-creates that folder so it's present from the
    // first launch.
    let mut web_context = WebContext::new(Some(base_dir.to_path_buf()));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let (window_width, window_height) =
        window_size(&event_loop, games.len(), config.image_gap, config.border_gap);

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

    center_window(&window);

    let base_for_launch = base_dir.to_path_buf();
    let base_for_protocol = base_dir.to_path_buf();

    let _webview = WebViewBuilder::new_with_web_context(&mut web_context)
        // Served over a custom `app://` protocol rather than a raw
        // `file://` URL: on Windows this resolves to a proper
        // `http://app.localhost/...` origin, which keeps IPC messages
        // (close/launch clicks) working. A `file:///C:/...` origin, by
        // contrast, isn't a URI wry's IPC plumbing can parse and crashes
        // the moment the page posts a message back.
        .with_custom_protocol("app".into(), move |_webview_id, request| {
            handle_app_request(&base_for_protocol, request)
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
            } else if let Some(index) = body.strip_prefix("launch:") {
                if let Ok(index) = index.parse::<usize>() {
                    if let Some(game) = games.get(index) {
                        let _ = Command::new(base_for_launch.join(&game.exe))
                            .current_dir(&base_for_launch)
                            .spawn();
                    }
                }
            }
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        // Keep the webview alive for the window's lifetime.
        let _ = &_webview;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            Event::UserEvent(UserEvent::CloseRequested) => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    });
}

/// Serves the UI from the baked-in `src/` assets, and `images/...` /
/// `games/...` straight from the content folder beside the exe, so paths in
/// catalog.json and `<img>` tags are just relative to the launcher's folder.
fn handle_app_request(base_dir: &Path, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let mut path = request.uri().path().trim_start_matches('/');
    if path.is_empty() {
        path = "index.html";
    }

    // Cartridge content lives on disk beside the exe.
    if path.starts_with("images/") || path.starts_with("games/") {
        return match fs::read(base_dir.join(path)) {
            Ok(bytes) => {
                let mime = mime_type_for(Path::new(path), &bytes);
                ok_response(mime, Cow::Owned(bytes))
            }
            Err(_) => not_found(),
        };
    }

    // Everything else is a UI asset, and only the web files count as one:
    // `src/` also holds main.rs and the config.toml / catalog.json seeds, and
    // neither the live path below nor rust-embed should ever hand those out.
    if !is_ui_asset(path) {
        return not_found();
    }

    // Prefer the live file from the source `src/` folder when it exists —
    // under `cargo run` that's the repo, so edits show up on the next launch
    // with no rebuild. When it's absent (the deployed cartridge has no source
    // tree), fall back to the copy baked into the exe by rust-embed.
    let source_ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(path);
    if let Ok(bytes) = fs::read(&source_ui) {
        let mime = mime_type_for(Path::new(path), &bytes);
        return ok_response(mime, Cow::Owned(bytes));
    }

    match UiAssets::get(path) {
        Some(file) => {
            let bytes = file.data.into_owned();
            let mime = mime_type_for(Path::new(path), &bytes);
            ok_response(mime, Cow::Owned(bytes))
        }
        None => not_found(),
    }
}

fn ok_response(mime: &'static str, body: Cow<'static, [u8]>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .header(CONTENT_TYPE, mime)
        // Never let WebView2 cache app:// responses in its data folder:
        // otherwise it serves a stale index.html forever and edits (or
        // swapped-in cover art) silently don't show up.
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap()
}

/// Whether an `app://` path names one of the web files that make up the UI.
/// Mirrors the rust-embed include list on `UiAssets`, so the dev (live from
/// `src/`) and deployed (embedded) paths serve exactly the same set.
fn is_ui_asset(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| UI_ASSET_EXTENSIONS.contains(&ext))
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(404)
        .body(Cow::Borrowed(&[][..]))
        .unwrap()
}

/// Sniffs the actual file content for images instead of trusting the
/// extension: cover art dropped into `images/` isn't always what its name
/// claims (e.g. an animated cover saved as `.png` that's actually WebP).
fn mime_type_for(path: &Path, content: &[u8]) -> &'static str {
    if content.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if content.len() >= 12 && &content[0..4] == b"RIFF" && &content[8..12] == b"WEBP" {
        return "image/webp";
    }
    if content.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "image/jpeg";
    }

    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html",
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

fn center_window(window: &tao::window::Window) {
    let Some(monitor) = window.primary_monitor() else {
        return;
    };
    let monitor_size = monitor.size();
    let window_size = window.outer_size();

    let x = (monitor_size.width as i32 - window_size.width as i32) / 2;
    let y = (monitor_size.height as i32 - window_size.height as i32) / 2;

    window.set_outer_position(Position::Physical(PhysicalPosition::new(x, y)));
}
