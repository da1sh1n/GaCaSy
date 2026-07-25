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
//     config.ini     <- seeded from the baked-in default if missing
//     catalog.json   <- the game list (name / exe / image), seeded likewise
//     images/        <- 600x900 cover art, dropped in by hand
//     games/         <- the actual game installs
//     ui/            <- WebView2's own data folder (its only on-disk crumbs)
//
// `cargo run` builds into target/ and runs in place, resolving `output/`
// as its content directory and refreshing `output/launcher.exe` so the
// shippable copy stays current. It does not relaunch itself, so there is
// exactly one launcher process (WebView2 still spawns its own renderer
// process — that is inherent to the engine and cannot be merged away).
//
// config.ini, catalog.json, images/ and games/ are never overwritten once
// present, so hand-dropped content survives every build.
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

/// The UI assets (index.html and anything beside it), baked into the exe
/// at compile time and served over the `app://` protocol at runtime.
#[derive(RustEmbed)]
#[folder = "ui/"]
struct UiAssets;

/// Baked-in defaults so a fresh `output/` can be seeded with no repo around
/// (e.g. on a real cartridge).
const DEFAULT_INI: &str = include_str!("../config.ini");
const DEFAULT_CATALOG: &str = include_str!("../catalog.json");

// Card layout constants. These mirror the CSS in ui/index.html (.card width,
// the 600x900 cover aspect ratio, .card .caption height, #grid gap/padding)
// so the window can be sized to fit its content exactly before the webview
// ever paints, instead of guessing a fixed size.
const CARD_WIDTH: u32 = 200;
const CARD_IMAGE_HEIGHT: u32 = CARD_WIDTH * 900 / 600;
const CARD_CAPTION_HEIGHT: u32 = 30; // 10px margin-top + 20px line-height
const GRID_GAP: u32 = 32;
const GRID_PADDING_X: u32 = 40; // #grid's own left/right padding
const WINDOW_MARGIN_Y: u32 = 56; // clears the close button and adds breathing room

#[derive(Deserialize, Clone)]
#[allow(dead_code)] // `name` and `image` are only read on the JS side.
struct Game {
    name: String,
    exe: String,
    image: String,
}

struct Config {
    show_captions: bool,
}

enum UserEvent {
    CloseRequested,
}

fn main() -> wry::Result<()> {
    let base_dir = resolve_base_dir();
    ensure_layout(&base_dir);

    // Nothing listens on a port: a second launch is fended off with a named
    // mutex that the OS releases automatically when this process dies.
    let _instance = match acquire_single_instance() {
        Some(guard) => guard,
        None => return Ok(()),
    };

    run_app(&base_dir)
}

/// The folder holding `launcher.exe` and all cartridge content.
///
/// When the deployed exe runs (its parent folder is named `output`), that
/// folder is the base. Under `cargo run` the exe lives in target/, so the
/// base is the repo's own `output/`.
fn resolve_base_dir() -> PathBuf {
    let exe = env::current_exe().expect("failed to resolve current exe path");
    let exe_dir = exe
        .parent()
        .expect("current exe has no parent directory")
        .to_path_buf();

    if exe_dir.file_name().and_then(|n| n.to_str()) == Some("output") {
        exe_dir
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("output")
    }
}

/// Creates the content folders, seeds config.ini/catalog.json if they're
/// missing, and refreshes the deployed exe. Existing content is never
/// touched, so hand-dropped covers, games and edits survive every build.
fn ensure_layout(base: &Path) {
    for sub in ["games", "images", "ui"] {
        fs::create_dir_all(base.join(sub))
            .unwrap_or_else(|e| panic!("failed to create output/{sub}/: {e}"));
    }
    seed_if_missing(&base.join("config.ini"), DEFAULT_INI);
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

/// Size the borderless window needs to fit `game_count` cards in a single
/// row with no clipping or extra dead space.
fn compute_window_size(game_count: usize, show_captions: bool) -> (u32, u32) {
    let n = game_count.max(1) as u32;
    let card_height = CARD_IMAGE_HEIGHT + if show_captions { CARD_CAPTION_HEIGHT } else { 0 };
    let width = 2 * GRID_PADDING_X + n * CARD_WIDTH + n.saturating_sub(1) * GRID_GAP;
    let height = card_height + 2 * WINDOW_MARGIN_Y;
    (width, height)
}

/// Reads config.ini (already seeded by `ensure_layout`).
fn load_config(base_dir: &Path) -> Config {
    let mut show_captions = false;
    if let Ok(contents) = fs::read_to_string(base_dir.join("config.ini")) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "show_captions" {
                    show_captions = value.trim().eq_ignore_ascii_case("true");
                }
            }
        }
    }
    Config { show_captions }
}

fn run_app(base_dir: &Path) -> wry::Result<()> {
    let config = load_config(base_dir);

    let catalog_path = base_dir.join("catalog.json");
    let catalog_json = fs::read_to_string(&catalog_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", catalog_path.display()));
    let games: Vec<Game> = serde_json::from_str(&catalog_json)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", catalog_path.display()));

    // Hands the parsed game list (and config) to the page as globals before
    // its own scripts run, since fetching catalog.json via `fetch()` would
    // hit CORS restrictions.
    let init_script = format!(
        "window.__GAMES__ = {catalog_json}; window.__SHOW_CAPTIONS__ = {};",
        config.show_captions
    );

    // WebView2's user-data folder is the engine's only on-disk footprint;
    // parking it in output/ui/ keeps its cache out of the content folders.
    let mut web_context = WebContext::new(Some(base_dir.join("ui")));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let (window_width, window_height) = compute_window_size(games.len(), config.show_captions);

    let window = WindowBuilder::new()
        .with_title("Cartridge Launcher")
        // Logical, not physical: these numbers are CSS pixels from
        // ui/index.html, so the OS must scale them to this monitor's DPI
        // itself, same as the webview scales its CSS.
        .with_inner_size(LogicalSize::new(window_width as f64, window_height as f64))
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

/// Serves the UI from the baked-in `ui/` assets, and `images/...` /
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

    // Everything else is a UI asset baked into the exe.
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
        .body(body)
        .unwrap()
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
