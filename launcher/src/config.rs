// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Reading `config.toml`, and the three keys the launcher writes back to it.
//!
//! The file is read as a plain table, key by key, rather than deserialized into
//! a struct: one wrong-typed value then costs only that setting instead of
//! rejecting the whole file, and a config written before a knob existed still
//! works. A file that isn't valid TOML *at all* is the one case that falls back
//! to every default.
//!
//! Every default lives in [`crate::constants`]; nothing here invents a number.
//!
//! # Reading and writing are not symmetric
//!
//! Almost everything here flows one way — TOML in, never out. The exceptions are
//! `order_mode`, `usage_order` and `user_order`, which the launcher owns and
//! keeps up to date as games are played and covers arranged. Those go back
//! through [`store`], which edits the one key in place with `toml_edit` and
//! leaves every comment, blank line and unrelated value exactly as it was. This
//! file is mostly comments written for a person, and a launcher that reformatted
//! it as a side effect of somebody starting a game would be answering a question
//! nobody asked.

use std::fs;
use std::path::Path;

use crate::constants::*;
use crate::log;
use crate::order;

pub struct Config {
    pub show_captions: bool,
    // Look-and-feel knobs, all read from config.toml (with the DEFAULT_*
    // fallbacks in `constants`). Numeric values are CSS pixels; colors are any
    // CSS color string. border_gap and image_gap also feed the window-sizing
    // math in `window`.
    pub border_gap: f64,
    pub image_gap: f64,
    pub corner_radius: f64,
    pub window_corner_radius: f64,
    /// The palette, 60 / 30 / 10. See [`crate::constants`] for what each one
    /// covers; the page works the in-between shades out from these three.
    pub primary_color: String,
    pub secondary_color: String,
    pub accent_color: String,
    pub shadow_size: f64,
    pub shadow_fade: f64,
    pub error_border_color: String,
    pub error_border_width: f64,
    pub error_text_color: String,
    pub missing_sign_color: String,
    pub missing_dim: f64,
    pub overlay_color: String,
    pub loading_ring_color: String,
    pub loading_text_color: String,
    pub loading_text_gap: f64,
    pub toolbar_color: String,
    pub scrollbar_color: String,
    pub show_console_window: bool,
    // The order the covers are shown in. Unlike everything above, these three
    // are written back by the launcher as well as read — see `store` and
    // [`crate::order`]. The two id lists are stored exactly as the file had
    // them; `order::normalize` is what makes them usable, at the point of use.
    pub order_mode: String,
    pub usage_order: Vec<usize>,
    pub user_order: Vec<usize>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            show_captions: false,
            border_gap: DEFAULT_BORDER_GAP,
            image_gap: DEFAULT_IMAGE_GAP,
            corner_radius: DEFAULT_CORNER_RADIUS,
            window_corner_radius: DEFAULT_WINDOW_CORNER_RADIUS,
            primary_color: DEFAULT_PRIMARY_COLOR.to_string(),
            secondary_color: DEFAULT_SECONDARY_COLOR.to_string(),
            accent_color: DEFAULT_ACCENT_COLOR.to_string(),
            shadow_size: DEFAULT_SHADOW_SIZE,
            shadow_fade: DEFAULT_SHADOW_FADE,
            error_border_color: DEFAULT_ERROR_BORDER_COLOR.to_string(),
            error_border_width: DEFAULT_ERROR_BORDER_WIDTH,
            error_text_color: DEFAULT_ERROR_TEXT_COLOR.to_string(),
            missing_sign_color: DEFAULT_MISSING_SIGN_COLOR.to_string(),
            missing_dim: DEFAULT_MISSING_DIM,
            overlay_color: DEFAULT_OVERLAY_COLOR.to_string(),
            loading_ring_color: DEFAULT_LOADING_RING_COLOR.to_string(),
            loading_text_color: DEFAULT_LOADING_TEXT_COLOR.to_string(),
            loading_text_gap: DEFAULT_LOADING_TEXT_GAP,
            toolbar_color: DEFAULT_TOOLBAR_COLOR.to_string(),
            scrollbar_color: DEFAULT_SCROLLBAR_COLOR.to_string(),
            // Off by default: a console game's window is ugly but harmless, and
            // hiding it is one fewer thing standing between "chose a cover" and
            // "game's on screen".
            show_console_window: false,
            order_mode: DEFAULT_ORDER_MODE.to_string(),
            // Empty is the honest starting state, and `order::normalize` turns
            // it into plain catalog order — so a cartridge nobody has played
            // yet shows its covers the way its author listed them.
            usage_order: Vec::new(),
            user_order: Vec::new(),
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
    set_f64(
        &mut config.window_corner_radius,
        table.get("window_corner_radius"),
    );
    // The palette, with the two keys it replaced standing in where they were
    // set. A cartridge written before the trio existed said `background_color`
    // and `shadow_color`; those meant the same two surfaces, so reading them
    // here is what keeps such a cartridge looking as it did. The new names win
    // when both are present.
    set_color(&mut config.primary_color, table.get("background_color"));
    set_color(&mut config.primary_color, table.get("primary_color"));
    set_color(&mut config.secondary_color, table.get("shadow_color"));
    set_color(&mut config.secondary_color, table.get("secondary_color"));
    set_color(&mut config.accent_color, table.get("accent_color"));
    set_f64(&mut config.shadow_size, table.get("shadow_size"));
    set_f64(&mut config.shadow_fade, table.get("shadow_fade"));
    set_color(&mut config.error_border_color, table.get("error_border_color"));
    set_f64(&mut config.error_border_width, table.get("error_border_width"));
    set_color(&mut config.error_text_color, table.get("error_text_color"));
    set_color(&mut config.missing_sign_color, table.get("missing_sign_color"));
    set_f64(&mut config.missing_dim, table.get("missing_dim"));
    set_color(&mut config.overlay_color, table.get("overlay_color"));
    set_color(&mut config.loading_ring_color, table.get("loading_ring_color"));
    set_color(&mut config.loading_text_color, table.get("loading_text_color"));
    set_f64(&mut config.loading_text_gap, table.get("loading_text_gap"));
    set_color(&mut config.toolbar_color, table.get("toolbar_color"));
    set_color(&mut config.scrollbar_color, table.get("scrollbar_color"));
    if let Some(value) = table.get("show_console_window").and_then(|v| v.as_bool()) {
        config.show_console_window = value;
    }
    set_mode(&mut config.order_mode, table.get("order_mode"));
    set_ids(&mut config.usage_order, table.get("usage_order"));
    set_ids(&mut config.user_order, table.get("user_order"));

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

/// Overwrites `slot` only if `value` names one of [`order::MODES`]. A typo
/// (`"alphabetical"`, say) leaves the default in force rather than picking one
/// of the four arbitrarily — the same "one bad value costs one setting" rule
/// the rest of this file follows.
fn set_mode(slot: &mut String, value: Option<&toml::Value>) {
    if let Some(name) = value.and_then(|v| v.as_str()) {
        let name = name.trim();
        if order::is_mode(name) {
            *slot = name.to_string();
        }
    }
}

/// Reads an id list, keeping the entries that are non-negative integers and
/// silently dropping the rest.
///
/// No bounds or duplicate checking here: how many games there are is not this
/// module's business, and the list is only ever *used* through
/// [`order::normalize`], which repairs it against the real count. This is just
/// "which numbers were actually in the file".
fn set_ids(slot: &mut Vec<usize>, value: Option<&toml::Value>) {
    let Some(array) = value.and_then(|v| v.as_array()) else {
        return;
    };
    *slot = array
        .iter()
        .filter_map(|entry| entry.as_integer())
        .filter_map(|id| usize::try_from(id).ok())
        .collect();
}

/// An id list as [`store`] wants it: a TOML array of integers, on one line.
///
/// A cartridge's whole catalog fits comfortably on one line, and an order is
/// read as a sequence — `[2, 0, 1]` says what it means at a glance in a way the
/// same numbers down a column do not.
pub fn ids(list: &[usize]) -> toml_edit::Value {
    let mut array: toml_edit::Array = list.iter().map(|&id| id as i64).collect();
    array.fmt();
    array.into()
}

/// Writes one top-level key back to `config.toml`, leaving every comment, blank
/// line and unrelated value exactly as it was.
///
/// Only the three order keys ever come through here (see the module docs). A
/// key already in the file is edited in place, keeping the comment above it; one
/// that isn't — a cartridge written before the key existed, where `sync_defaults`
/// has only left a commented-out line — is appended with its description, which
/// is the same treatment a knob gets the first time it is set.
///
/// Never fatal, and never even complained about beyond a log line. A cartridge
/// can perfectly well sit on a write-protected stick, and a launcher that
/// refused to start a game because it could not record having started one would
/// have its priorities backwards.
pub fn store(base_dir: &Path, key: &str, value: toml_edit::Value) {
    let path = base_dir.join("config.toml");
    let Ok(contents) = fs::read_to_string(&path) else {
        log::line(base_dir, &format!("could not read config.toml to set {key}"));
        return;
    };
    let Ok(mut doc) = contents.parse::<toml_edit::DocumentMut>() else {
        // The same file `load` gave up on and ran from defaults. Rewriting it
        // would mean guessing at what the author meant; leave it for them.
        log::line(
            base_dir,
            &format!("config.toml is not valid TOML, leaving {key} unwritten"),
        );
        return;
    };

    let existed = doc.contains_key(key);
    doc[key] = toml_edit::value(value);
    if !existed {
        // Appended at the bottom, under the same one-line description
        // `sync_defaults` would have used — so a key the launcher sets for the
        // first time arrives looking like the hand-written ones above it rather
        // than tacked on. A blank line first, for the same reason.
        let description = known_settings()
            .into_iter()
            .find(|(name, _, _)| *name == key)
            .map(|(_, description, _)| description);
        if let (Some(description), Some(mut new_key)) = (description, doc.key_mut(key)) {
            new_key
                .leaf_decor_mut()
                .set_prefix(format!("\n# {description}\n"));
        }
    }

    if let Err(error) = fs::write(&path, doc.to_string()) {
        log::line(base_dir, &format!("could not write {key} to config.toml: {error}"));
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
            "Show the selected game's name on one line under the row.",
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
            "window_corner_radius",
            "How rounded the window's own corners are (0 = square).",
            DEFAULT_WINDOW_CORNER_RADIUS.to_string(),
        ),
        (
            "primary_color",
            "60% of the palette: the window behind everything.",
            format!("\"{DEFAULT_PRIMARY_COLOR}\""),
        ),
        (
            "secondary_color",
            "30%: shadows, borders, and the plate behind missing cover art.",
            format!("\"{DEFAULT_SECONDARY_COLOR}\""),
        ),
        (
            "accent_color",
            "10%: text, the selected cover, and the close button.",
            format!("\"{DEFAULT_ACCENT_COLOR}\""),
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
            "overlay_color",
            "Screen darkening while the chosen game starts up.",
            format!("\"{DEFAULT_OVERLAY_COLOR}\""),
        ),
        (
            "loading_ring_color",
            "Progress line under a game that's starting; blank derives it from accent_color.",
            format!("\"{DEFAULT_LOADING_RING_COLOR}\""),
        ),
        (
            "loading_text_color",
            "Status line under the progress line; blank derives it from accent_color.",
            format!("\"{DEFAULT_LOADING_TEXT_COLOR}\""),
        ),
        (
            "loading_text_gap",
            "Pixels between the progress line and the text under it.",
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
            "toolbar_color",
            "Toolbar text and outlines; blank derives them from accent_color.",
            format!("\"{DEFAULT_TOOLBAR_COLOR}\""),
        ),
        (
            "scrollbar_color",
            "The bar under a row too long to fit; blank derives it from secondary_color.",
            format!("\"{DEFAULT_SCROLLBAR_COLOR}\""),
        ),
        (
            "show_console_window",
            "Show the console window a console-mode game would normally open.",
            "false".to_string(),
        ),
        (
            "order_mode",
            "Cover order: \"usage\", \"alphabetic\", \"catalog\" or \"user\".",
            format!("\"{DEFAULT_ORDER_MODE}\""),
        ),
        (
            "usage_order",
            "Most recently played first; the launcher keeps this up to date.",
            "[]".to_string(),
        ),
        (
            "user_order",
            "Hand-arranged order, used when order_mode = \"user\".",
            "[]".to_string(),
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
