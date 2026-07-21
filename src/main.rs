// Cartridge launcher shell.
//
// `cargo run` builds the exe into target/, but the launcher is meant to live
// in `output/` next to its assets on the real cartridge, so every run copies
// the freshly built binary and interface/ into `output/` and re-executes
// itself from there. That way paths the app resolves relative to its own
// exe location are always correct, on a dev machine or on the cartridge.
//
// output/images/ and output/games/ are cartridge content, not build
// output: nothing here ever writes into them beyond creating them if
// missing, so cover art and game installs dropped in by hand survive
// every `cargo run`.
//
// No console window: this is a GUI app, not a CLI tool.
#![windows_subsystem = "windows"]

use std::borrow::Cow;
use std::env;
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;
use tao::dpi::{LogicalSize, PhysicalPosition, Position};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::http::{header::CONTENT_TYPE, Request, Response};
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows};

/// Bound as a lock to keep a second instance of the deployed app from
/// opening a second window. Any fixed local-only port works; picked at
/// random out of the dynamic/private range.
const SINGLE_INSTANCE_PORT: u16 = 47831;

// Card layout constants. These mirror the CSS in interface/index.html
// (.card width, the 600x900 cover aspect ratio, .card .caption height,
// #grid gap/padding) so the window can be sized to fit its content exactly
// before the webview ever paints, instead of guessing a fixed size.
const CARD_WIDTH: u32 = 200;
const CARD_IMAGE_HEIGHT: u32 = CARD_WIDTH * 900 / 600;
const CARD_CAPTION_HEIGHT: u32 = 30; // 10px margin-top + 20px line-height
const GRID_GAP: u32 = 32;
const GRID_PADDING_X: u32 = 40; // #grid's own left/right padding
const WINDOW_MARGIN_Y: u32 = 56; // clears the close button and adds breathing room

/// Default `launcher.ini`, baked into the exe so the deployed copy is
/// self-contained even with no repo around (e.g. on a real cartridge).
const DEFAULT_INI: &str = include_str!("../launcher.ini");

/// Size the borderless window needs to fit `game_count` cards in a single
/// row with no clipping or extra dead space.
fn compute_window_size(game_count: usize, show_captions: bool) -> (u32, u32) {
    let n = game_count.max(1) as u32;
    let card_height = CARD_IMAGE_HEIGHT + if show_captions { CARD_CAPTION_HEIGHT } else { 0 };
    let width = 2 * GRID_PADDING_X + n * CARD_WIDTH + n.saturating_sub(1) * GRID_GAP;
    let height = card_height + 2 * WINDOW_MARGIN_Y;
    (width, height)
}

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

/// Reads `output/launcher.ini`, writing the bundled default first if it's
/// not already there. Never overwrites an existing file, so user edits
/// stick across every `cargo run`.
fn load_or_create_config(output_dir: &Path) -> Config {
    let config_path = output_dir.join("launcher.ini");
    if !config_path.exists() {
        fs::write(&config_path, DEFAULT_INI).expect("failed to write default launcher.ini");
    }

    let mut show_captions = false;
    if let Ok(contents) = fs::read_to_string(&config_path) {
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

enum UserEvent {
    CloseRequested,
}

fn main() -> wry::Result<()> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_dir = manifest_dir.join("output");
    let current_exe = env::current_exe().expect("failed to resolve current exe path");

    let running_from_output = current_exe
        .parent()
        .and_then(|p| p.canonicalize().ok())
        == output_dir.canonicalize().ok();

    if !running_from_output {
        // Don't stomp on a copy of the exe that's currently running.
        if instance_already_running() {
            return Ok(());
        }
        deploy_and_relaunch(manifest_dir, &output_dir, &current_exe);
        return Ok(());
    }

    run_app(&output_dir)
}

/// True if another instance already holds the single-instance lock.
fn instance_already_running() -> bool {
    match TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        Ok(listener) => {
            drop(listener);
            false
        }
        Err(_) => true,
    }
}

/// Copies the built exe and `interface/` into `output/`, then spawns the
/// deployed exe as an independent process and exits this one.
///
/// `output/images/` and `output/games/` are cartridge content the user
/// manages by hand (covers, game installs) — they're created if missing
/// but never touched otherwise.
fn deploy_and_relaunch(manifest_dir: &Path, output_dir: &Path, current_exe: &Path) {
    let interface_dir = output_dir.join("interface");
    fs::create_dir_all(&interface_dir).expect("failed to create output/interface/ directory");
    fs::create_dir_all(output_dir.join("images")).expect("failed to create output/images/");
    fs::create_dir_all(output_dir.join("games")).expect("failed to create output/games/");

    let src_interface_dir = manifest_dir.join("interface");
    for asset in ["index.html", "games.json"] {
        let src = src_interface_dir.join(asset);
        let dst = interface_dir.join(asset);
        fs::copy(&src, &dst).unwrap_or_else(|e| {
            panic!("failed to copy {} to output/interface/: {e}", src.display())
        });
    }

    let exe_name = current_exe
        .file_name()
        .expect("current exe path has no file name");
    let deployed_exe = output_dir.join(exe_name);
    fs::copy(current_exe, &deployed_exe).expect("failed to copy exe into output/");

    Command::new(&deployed_exe)
        .current_dir(output_dir)
        .spawn()
        .expect("failed to launch deployed exe from output/");
}

fn run_app(output_dir: &Path) -> wry::Result<()> {
    // Held for the process lifetime: as long as this listener is bound, a
    // second launch of the deployed exe will fail to bind and exit quietly
    // instead of opening a second window.
    let _single_instance_lock = match TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        Ok(listener) => listener,
        Err(_) => return Ok(()),
    };

    let interface_dir = output_dir.join("interface");
    let config = load_or_create_config(output_dir);

    let games_json_path = interface_dir.join("games.json");
    let games_json = fs::read_to_string(&games_json_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", games_json_path.display()));
    let games: Vec<Game> = serde_json::from_str(&games_json)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", games_json_path.display()));

    // Exposes the parsed game list (and config) to the page as globals
    // before its own scripts run, since fetching games.json via `fetch()`
    // would hit CORS restrictions.
    let init_script = format!(
        "window.__GAMES__ = {games_json}; window.__SHOW_CAPTIONS__ = {};",
        config.show_captions
    );

    // Keep the WebView2 runtime's own data folder under interface/ too, so
    // output/ holds nothing but the exe, interface/, images/ and games/.
    let mut web_context = WebContext::new(Some(interface_dir.join(".webview2")));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let (window_width, window_height) = compute_window_size(games.len(), config.show_captions);

    let window = WindowBuilder::new()
        .with_title("Cartridge Launcher")
        // Logical, not physical: these numbers are CSS pixels from
        // interface/index.html, so the OS must scale them to this
        // monitor's DPI itself, same as the webview scales its CSS.
        .with_inner_size(LogicalSize::new(window_width as f64, window_height as f64))
        .with_resizable(false)
        .with_decorations(false)
        .with_always_on_top(false)
        .build(&event_loop)
        .expect("failed to create window");

    center_window(&window);

    let output_dir_for_launch = output_dir.to_path_buf();
    let output_dir_for_protocol = output_dir.to_path_buf();

    let _webview = WebViewBuilder::new_with_web_context(&mut web_context)
        // Served over a custom `app://` protocol rather than a raw
        // `file://` URL: on Windows this resolves to a proper
        // `http://app.localhost/...` origin, which keeps IPC messages
        // (close/launch clicks) working. A `file:///C:/...` origin, by
        // contrast, isn't a URI wry's IPC plumbing can parse and crashes
        // the moment the page posts a message back.
        .with_custom_protocol("app".into(), move |_webview_id, request| {
            handle_app_request(&output_dir_for_protocol, request)
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
                        let _ = Command::new(output_dir_for_launch.join(&game.exe))
                            .current_dir(&output_dir_for_launch)
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

/// Serves `interface/index.html` at `/` and everything else (`/images/...`,
/// `/games/...`) straight from `output/`, so paths in games.json and
/// `<img>` tags are just relative to the launcher's own folder.
fn handle_app_request(output_dir: &Path, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let url_path = request.uri().path();
    let relative_path = if url_path == "/" {
        "interface/index.html"
    } else {
        url_path.trim_start_matches('/')
    };
    let file_path = output_dir.join(relative_path);

    match fs::read(&file_path) {
        Ok(content) => {
            let mime = mime_type_for(&file_path, &content);
            Response::builder()
                .header(CONTENT_TYPE, mime)
                .body(Cow::Owned(content))
                .unwrap()
        }
        Err(_) => Response::builder()
            .status(404)
            .body(Cow::Borrowed(&[][..]))
            .unwrap(),
    }
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
