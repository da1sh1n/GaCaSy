// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Reading `config.toml`.
//!
//! The file is read as a plain table, key by key, rather than deserialized into
//! a struct: one wrong-typed value then costs only that setting instead of
//! rejecting the whole file, and a config written before a knob existed still
//! works. A file that isn't valid TOML *at all* is the one case that falls back
//! to every default.
//!
//! Every default lives in [`crate::constants`]; nothing here invents a number.

use std::fs;
use std::path::Path;

use crate::constants::*;

pub struct Config {
    pub show_captions: bool,
    // Look-and-feel knobs, all read from config.toml (with the DEFAULT_*
    // fallbacks in `constants`). Numeric values are CSS pixels; colors are any
    // CSS color string. border_gap and image_gap also feed the window-sizing
    // math in `window`.
    pub border_gap: f64,
    pub image_gap: f64,
    pub corner_radius: f64,
    pub background_color: String,
    pub shadow_size: f64,
    pub shadow_fade: f64,
    pub shadow_color: String,
    pub error_border_color: String,
    pub error_border_width: f64,
    pub error_text_color: String,
    pub missing_sign_color: String,
    pub missing_dim: f64,
    pub overlay_color: String,
    pub loading_ring_color: String,
    pub loading_text_color: String,
    pub loading_ring_segments: f64,
    pub loading_ring_speed: f64,
    pub loading_text_gap: f64,
    pub show_console_window: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            show_captions: false,
            border_gap: DEFAULT_BORDER_GAP,
            image_gap: DEFAULT_IMAGE_GAP,
            corner_radius: DEFAULT_CORNER_RADIUS,
            background_color: DEFAULT_BACKGROUND_COLOR.to_string(),
            shadow_size: DEFAULT_SHADOW_SIZE,
            shadow_fade: DEFAULT_SHADOW_FADE,
            shadow_color: DEFAULT_SHADOW_COLOR.to_string(),
            error_border_color: DEFAULT_ERROR_BORDER_COLOR.to_string(),
            error_border_width: DEFAULT_ERROR_BORDER_WIDTH,
            error_text_color: DEFAULT_ERROR_TEXT_COLOR.to_string(),
            missing_sign_color: DEFAULT_MISSING_SIGN_COLOR.to_string(),
            missing_dim: DEFAULT_MISSING_DIM,
            overlay_color: DEFAULT_OVERLAY_COLOR.to_string(),
            loading_ring_color: DEFAULT_LOADING_RING_COLOR.to_string(),
            loading_text_color: DEFAULT_LOADING_TEXT_COLOR.to_string(),
            loading_ring_segments: DEFAULT_LOADING_RING_SEGMENTS,
            loading_ring_speed: DEFAULT_LOADING_RING_SPEED,
            loading_text_gap: DEFAULT_LOADING_TEXT_GAP,
            // Off by default: a console game's window is ugly but harmless, and
            // hiding it is one fewer thing standing between "chose a cover" and
            // "game's on screen".
            show_console_window: false,
        }
    }
}

/// Reads config.toml (already seeded by `content::ensure_layout`). Unknown keys
/// and unusable values are ignored, leaving that setting at its default, so an
/// older config.toml (or a typo in one value) still yields a usable launcher.
pub fn load(base_dir: &Path) -> Config {
    let mut config = Config::default();

    let Ok(contents) = fs::read_to_string(base_dir.join("config.toml")) else {
        return config;
    };
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
    set_color(&mut config.error_border_color, table.get("error_border_color"));
    set_f64(&mut config.error_border_width, table.get("error_border_width"));
    set_color(&mut config.error_text_color, table.get("error_text_color"));
    set_color(&mut config.missing_sign_color, table.get("missing_sign_color"));
    set_f64(&mut config.missing_dim, table.get("missing_dim"));
    set_color(&mut config.overlay_color, table.get("overlay_color"));
    set_color(&mut config.loading_ring_color, table.get("loading_ring_color"));
    set_color(&mut config.loading_text_color, table.get("loading_text_color"));
    set_f64(
        &mut config.loading_ring_segments,
        table.get("loading_ring_segments"),
    );
    set_f64(&mut config.loading_ring_speed, table.get("loading_ring_speed"));
    set_f64(&mut config.loading_text_gap, table.get("loading_text_gap"));
    if let Some(value) = table.get("show_console_window").and_then(|v| v.as_bool()) {
        config.show_console_window = value;
    }

    // Held to the floor here rather than in the page, so anything reading the
    // config sees the speed the ring will actually turn at. `set_f64` has
    // already rejected negatives; this catches 0 and the merely-too-slow.
    config.loading_ring_speed = config.loading_ring_speed.max(MIN_LOADING_RING_SPEED);

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

/// Every key `config.toml` can hold: its TOML name, a one-line description,
/// and its default value formatted exactly as it would appear in the file.
/// The only consumer is [`sync_defaults`] — this is not where `load` gets its
/// defaults from, `Default for Config` above still owns those — so a knob
/// that's missing here just doesn't get documented for an old cartridge; it
/// still works.
fn known_settings() -> Vec<(&'static str, &'static str, String)> {
    vec![
        (
            "show_captions",
            "Show the game's name under its cover card.",
            "false".to_string(),
        ),
        (
            "border_gap",
            "Empty space between the window edge and the covers.",
            DEFAULT_BORDER_GAP.to_string(),
        ),
        (
            "image_gap",
            "Gap between adjacent covers.",
            DEFAULT_IMAGE_GAP.to_string(),
        ),
        (
            "corner_radius",
            "How rounded each cover's corners are (0 = square).",
            DEFAULT_CORNER_RADIUS.to_string(),
        ),
        (
            "background_color",
            "Window / page background behind the covers.",
            format!("\"{DEFAULT_BACKGROUND_COLOR}\""),
        ),
        (
            "shadow_size",
            "How far the shadow reaches out from the cover edge.",
            DEFAULT_SHADOW_SIZE.to_string(),
        ),
        (
            "shadow_fade",
            "Solid color for this many px before the shadow starts fading.",
            DEFAULT_SHADOW_FADE.to_string(),
        ),
        (
            "shadow_color",
            "The shadow's color; use the rgba alpha to set how dark it is.",
            format!("\"{DEFAULT_SHADOW_COLOR}\""),
        ),
        (
            "overlay_color",
            "Screen darkening while the chosen game starts up.",
            format!("\"{DEFAULT_OVERLAY_COLOR}\""),
        ),
        (
            "loading_ring_color",
            "Color of the ring that spins while a game starts.",
            format!("\"{DEFAULT_LOADING_RING_COLOR}\""),
        ),
        (
            "loading_text_color",
            "Color of the status line under the loading ring.",
            format!("\"{DEFAULT_LOADING_TEXT_COLOR}\""),
        ),
        (
            "loading_ring_segments",
            "How many pieces the loading ring is cut into.",
            DEFAULT_LOADING_RING_SEGMENTS.to_string(),
        ),
        (
            "loading_ring_speed",
            "How fast the loading ring turns, in turns per second.",
            DEFAULT_LOADING_RING_SPEED.to_string(),
        ),
        (
            "loading_text_gap",
            "Pixels between the loading ring and the text under it.",
            DEFAULT_LOADING_TEXT_GAP.to_string(),
        ),
        (
            "error_border_color",
            "Border color on a cover that failed to launch.",
            format!("\"{DEFAULT_ERROR_BORDER_COLOR}\""),
        ),
        (
            "error_border_width",
            "Width of that border.",
            DEFAULT_ERROR_BORDER_WIDTH.to_string(),
        ),
        (
            "error_text_color",
            "Color of the failure message under the cover.",
            format!("\"{DEFAULT_ERROR_TEXT_COLOR}\""),
        ),
        (
            "missing_sign_color",
            "Sign color over a game whose exe isn't on the cartridge.",
            format!("\"{DEFAULT_MISSING_SIGN_COLOR}\""),
        ),
        (
            "missing_dim",
            "Brightness multiplier for a missing game's cover (1 = untouched, 0 = black).",
            DEFAULT_MISSING_DIM.to_string(),
        ),
        (
            "show_console_window",
            "Show the console window a console-mode game would normally open.",
            "false".to_string(),
        ),
    ]
}

/// Appends a commented-out, already-in-effect line for every known setting
/// missing from `config.toml` — the case where the cartridge was set up
/// before that setting existed. Nothing about the running launcher changes
/// (the lines are comments, and each one already names the default that was
/// silently in force); this only makes the setting discoverable, and
/// uncommenting the line is how you go back and pick something else.
///
/// A no-op if the file can't be read or doesn't parse as TOML, and a no-op
/// once every known key is present — most runs, after the first.
pub fn sync_defaults(config_path: &Path) {
    let Ok(contents) = fs::read_to_string(config_path) else {
        return;
    };
    let Ok(table) = contents.parse::<toml::Table>() else {
        return;
    };

    let missing: Vec<_> = known_settings()
        .into_iter()
        .filter(|(key, _, _)| !table.contains_key(*key))
        .collect();
    if missing.is_empty() {
        return;
    }

    let mut addition = String::from(
        "\n# ── Added since this file was created ───────────────────────────────────\n\
         # These settings didn't exist yet when this config was written. Each is\n\
         # commented out below and already running at the default value shown;\n\
         # uncomment and edit a line to change it.\n",
    );
    for (key, doc, default) in missing {
        addition.push_str(&format!("\n# {doc}\n# {key} = {default}\n"));
    }

    let _ = fs::write(config_path, contents + &addition);
}
