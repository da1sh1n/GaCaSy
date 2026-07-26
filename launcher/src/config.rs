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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    /// Writes `contents` as a config.toml in its own temp folder and reads it
    /// back through the real loader.
    fn config_from(name: &str, contents: &str) -> Config {
        let dir = env::temp_dir().join(format!("gacasy-config-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("config.toml"), contents).expect("write config");
        load(&dir)
    }

    #[test]
    fn ring_speed_is_held_to_its_floor() {
        // The floor exists only to keep the ring turning at all: a stopped one
        // says the opposite of what it's there to say.
        assert_eq!(
            config_from("stopped", "loading_ring_speed = 0").loading_ring_speed,
            MIN_LOADING_RING_SPEED
        );
        assert_eq!(
            config_from("crawling", "loading_ring_speed = 0.001").loading_ring_speed,
            MIN_LOADING_RING_SPEED
        );
        // Anything at or above it is left exactly as written, however slow.
        assert_eq!(
            config_from("slow", "loading_ring_speed = 0.06").loading_ring_speed,
            0.06
        );
        assert_eq!(
            config_from("fast", "loading_ring_speed = 1.5").loading_ring_speed,
            1.5
        );
    }

    #[test]
    fn an_unusable_value_costs_only_its_own_setting() {
        let config = config_from(
            "onebad",
            "error_border_width = \"oops\"\nloading_text_gap = 40\n",
        );
        assert_eq!(config.error_border_width, DEFAULT_ERROR_BORDER_WIDTH);
        assert_eq!(config.loading_text_gap, 40.0);
    }

    #[test]
    fn a_file_that_isnt_toml_leaves_every_default_standing() {
        let config = config_from("garbage", "this is not toml at all {{{");
        assert_eq!(config.loading_ring_segments, DEFAULT_LOADING_RING_SEGMENTS);
        assert_eq!(config.loading_ring_color, DEFAULT_LOADING_RING_COLOR);
        assert_eq!(config.loading_ring_speed, DEFAULT_LOADING_RING_SPEED);
    }
}
