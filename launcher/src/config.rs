// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Reads `config.toml` into a `Config`, key by key, and writes single keys back
//! without disturbing the rest of the file. Holds the table of every setting,
//! its default and its description.

// ########## CONFIG.TOML ##########

use std::fs;
use std::path::Path;

use crate::constants::*;
use crate::log;
use crate::order;

// ========== The Settings Table ==========

/// How one setting is read out of the TOML table, carrying its default.
enum Kind {
    /// A `true` / `false`.
    Flag(bool),
    /// A non-negative CSS pixel count, written as an integer or a float.
    Number(f64),
    /// A proportion, clamped into 0..=1 rather than rejected above it: "more
    /// solid than solid" has an obvious intent, and dropping it would silently
    /// leave the default instead of honouring it.
    Unit(f64),
    /// Any non-blank CSS color string.
    Color(&'static str),
    /// A string that has to name one of a fixed set. A typo costs this one
    /// setting and leaves the default, rather than picking arbitrarily.
    OneOf(&'static str, &'static [&'static str]),
    /// An array of catalog ids. Bounds and duplicates are not this module's
    /// business — `order::normalize` repairs the list against the real game
    /// count at the point of use.
    Ids,
}

/// One key `config.toml` can hold: its TOML name, the one-line description
/// written above it in the file, and how its value is read.
struct Setting {
    name: &'static str,
    description: &'static str,
    kind: Kind,
}

/// Every setting, in the order they appear in a config file. This is the single
/// list: `Config::default` takes its defaults from it, `load` reads the file
/// through it, and `syncDefaults` documents missing keys from it.
const SETTINGS: &[Setting] = &[
    Setting {
        name: "show_captions",
        description: "Show the selected game's name on one line under the row.",
        kind: Kind::Flag(false),
    },
    Setting {
        name: "border_gap",
        description: "Empty space between the window edge and the covers, on all four sides.",
        kind: Kind::Number(DEFAULT_BORDER_GAP),
    },
    Setting {
        name: "image_gap",
        description: "Gap between adjacent covers.",
        kind: Kind::Number(DEFAULT_IMAGE_GAP),
    },
    Setting {
        name: "corner_radius",
        description: "How rounded each cover's corners are (0 = square).",
        kind: Kind::Number(DEFAULT_CORNER_RADIUS),
    },
    Setting {
        name: "window_corner_radius",
        description: "How rounded the window's own corners are (0 = square).",
        kind: Kind::Number(DEFAULT_WINDOW_CORNER_RADIUS),
    },
    Setting {
        name: "primary_color",
        description: "60% of the palette: the window behind everything.",
        kind: Kind::Color(DEFAULT_PRIMARY_COLOR),
    },
    Setting {
        name: "secondary_color",
        description: "30%: shadows, borders, and the plate behind missing cover art.",
        kind: Kind::Color(DEFAULT_SECONDARY_COLOR),
    },
    Setting {
        name: "accent_color",
        description: "10%: text, the selected cover, and the close button.",
        kind: Kind::Color(DEFAULT_ACCENT_COLOR),
    },
    Setting {
        name: "shadow_size",
        description: "How far the shadow reaches out from the cover edge.",
        kind: Kind::Number(DEFAULT_SHADOW_SIZE),
    },
    Setting {
        name: "shadow_fade",
        description: "Solid color for this many px before the shadow starts fading.",
        kind: Kind::Number(DEFAULT_SHADOW_FADE),
    },
    Setting {
        name: "overlay_color",
        description: "Screen darkening while the chosen game starts up.",
        kind: Kind::Color(DEFAULT_OVERLAY_COLOR),
    },
    Setting {
        name: "loading_ring_color",
        description: "Progress line under a game that's starting; blank derives it from \
                      accent_color.",
        kind: Kind::Color(DEFAULT_LOADING_RING_COLOR),
    },
    Setting {
        name: "loading_text_color",
        description: "Status line under the progress line; blank derives it from accent_color.",
        kind: Kind::Color(DEFAULT_LOADING_TEXT_COLOR),
    },
    Setting {
        name: "loading_text_gap",
        description: "Pixels between the progress line and the text under it.",
        kind: Kind::Number(DEFAULT_LOADING_TEXT_GAP),
    },
    Setting {
        name: "error_border_color",
        description: "Border color on a cover that failed to launch.",
        kind: Kind::Color(DEFAULT_ERROR_BORDER_COLOR),
    },
    Setting {
        name: "error_border_width",
        description: "Width of that border.",
        kind: Kind::Number(DEFAULT_ERROR_BORDER_WIDTH),
    },
    Setting {
        name: "error_text_color",
        description: "Color of the failure message under the cover.",
        kind: Kind::Color(DEFAULT_ERROR_TEXT_COLOR),
    },
    Setting {
        name: "missing_sign_color",
        description: "Sign color over a game whose exe isn't on the cartridge.",
        kind: Kind::Color(DEFAULT_MISSING_SIGN_COLOR),
    },
    Setting {
        name: "missing_dim",
        description: "Brightness multiplier for a missing game's cover (1 = untouched, \
                      0 = black).",
        kind: Kind::Number(DEFAULT_MISSING_DIM),
    },
    Setting {
        name: "toolbar_color",
        description: "Toolbar text and outlines; blank derives them from accent_color.",
        kind: Kind::Color(DEFAULT_TOOLBAR_COLOR),
    },
    Setting {
        name: "scrollbar_color",
        description: "The bar under a row too long to fit; blank derives it from \
                      secondary_color.",
        kind: Kind::Color(DEFAULT_SCROLLBAR_COLOR),
    },
    Setting {
        name: "cursor_color",
        description: "The ring drawn in place of the mouse pointer; blank picks white or black \
                      per cover.",
        kind: Kind::Color(DEFAULT_CURSOR_COLOR),
    },
    Setting {
        name: "background_effect",
        description: "Movement behind the covers: \"simple\", \"particles\" or \"fog\".",
        kind: Kind::OneOf(DEFAULT_BACKGROUND_EFFECT, &BACKGROUND_EFFECTS),
    },
    Setting {
        name: "background_effect_color",
        description: "What that movement is made of; blank derives it from the palette.",
        kind: Kind::Color(DEFAULT_BACKGROUND_EFFECT_COLOR),
    },
    Setting {
        name: "cover_opacity",
        description: "How opaque a cover is while it isn't the one pointed at (1 = solid).",
        kind: Kind::Unit(DEFAULT_COVER_OPACITY),
    },
    Setting {
        name: "show_console_window",
        // Off by default: a console game's window is ugly but harmless, and
        // hiding it is one fewer thing between "chose a cover" and "game's on".
        description: "Show the console window a console-mode game would normally open.",
        kind: Kind::Flag(false),
    },
    Setting {
        name: "order_mode",
        description: "Cover order: \"usage\", \"alphabetic\", \"catalog\" or \"user\".",
        kind: Kind::OneOf(DEFAULT_ORDER_MODE, &order::MODES),
    },
    Setting {
        name: "usage_order",
        description: "Most recently played first; the launcher keeps this up to date.",
        kind: Kind::Ids,
    },
    Setting {
        name: "user_order",
        description: "Hand-arranged order, used when order_mode = \"user\".",
        kind: Kind::Ids,
    },
];

// ========== The Settings Themselves ==========

pub struct Config {
    pub show_captions: bool,
    // Look-and-feel knobs, all read from config.toml. Numeric values are CSS
    // pixels; colors are any CSS color string. border_gap and image_gap also
    // feed the window-sizing math in `window`.
    pub border_gap: f64,
    pub image_gap: f64,
    pub corner_radius: f64,
    pub window_corner_radius: f64,
    /// The palette, 60 / 30 / 10. The page works the in-between shades out from
    /// these three.
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
    pub cursor_color: String,
    /// One of `BACKGROUND_EFFECTS`, and the color the effect is built out of —
    /// blank meaning "work it out from the palette".
    pub background_effect: String,
    pub background_effect_color: String,
    pub cover_opacity: f64,
    pub show_console_window: bool,
    // The cover order. Unlike everything above, these three are written back by
    // the launcher as well as read. The id lists are stored exactly as the file
    // had them; `order::normalize` makes them usable at the point of use.
    pub order_mode: String,
    pub usage_order: Vec<usize>,
    pub user_order: Vec<usize>,
}

impl Default for Config {
    // `default` is fixed by the Default trait, so it keeps rustc's spelling.
    fn default() -> Self {
        // Zeroed, then filled from SETTINGS below. Writing the real defaults
        // here as well would be the second copy of the same table.
        let mut config = Config {
            show_captions: false,
            border_gap: 0.0,
            image_gap: 0.0,
            corner_radius: 0.0,
            window_corner_radius: 0.0,
            primary_color: String::new(),
            secondary_color: String::new(),
            accent_color: String::new(),
            shadow_size: 0.0,
            shadow_fade: 0.0,
            error_border_color: String::new(),
            error_border_width: 0.0,
            error_text_color: String::new(),
            missing_sign_color: String::new(),
            missing_dim: 0.0,
            overlay_color: String::new(),
            loading_ring_color: String::new(),
            loading_text_color: String::new(),
            loading_text_gap: 0.0,
            toolbar_color: String::new(),
            scrollbar_color: String::new(),
            cursor_color: String::new(),
            background_effect: String::new(),
            background_effect_color: String::new(),
            cover_opacity: 0.0,
            show_console_window: false,
            order_mode: String::new(),
            // Empty is the honest starting state, and `order::normalize` turns
            // it into plain catalog order — so a cartridge nobody has played yet
            // shows its covers the way its author listed them.
            usage_order: Vec::new(),
            user_order: Vec::new(),
        };
        for setting in SETTINGS {
            // `None` for the found value means "no file said otherwise", so
            // every slot takes the default out of its own `Kind`.
            config.apply(setting, None);
        }
        config
    }
}

impl Config {
    /// Writes one setting into its field: the value from `found` if it is
    /// usable, and the setting's own default otherwise.
    ///
    /// The match arms are the entire mapping from TOML key to struct field, so
    /// there is no second list of names to keep in step with `SETTINGS`.
    fn apply(&mut self, setting: &Setting, found: Option<&toml::Value>) {
        // Closures rather than three calls each: the readers all need the same
        // two arguments, and this keeps the arms below to one line apiece.
        let flag = || readFlag(&setting.kind, found);
        let number = || readNumber(&setting.kind, found);
        let text = || readText(&setting.kind, found);

        match setting.name {
            "show_captions" => self.show_captions = flag(),
            "border_gap" => self.border_gap = number(),
            "image_gap" => self.image_gap = number(),
            "corner_radius" => self.corner_radius = number(),
            "window_corner_radius" => self.window_corner_radius = number(),
            "primary_color" => self.primary_color = text(),
            "secondary_color" => self.secondary_color = text(),
            "accent_color" => self.accent_color = text(),
            "shadow_size" => self.shadow_size = number(),
            "shadow_fade" => self.shadow_fade = number(),
            "overlay_color" => self.overlay_color = text(),
            "loading_ring_color" => self.loading_ring_color = text(),
            "loading_text_color" => self.loading_text_color = text(),
            "loading_text_gap" => self.loading_text_gap = number(),
            "error_border_color" => self.error_border_color = text(),
            "error_border_width" => self.error_border_width = number(),
            "error_text_color" => self.error_text_color = text(),
            "missing_sign_color" => self.missing_sign_color = text(),
            "missing_dim" => self.missing_dim = number(),
            "toolbar_color" => self.toolbar_color = text(),
            "scrollbar_color" => self.scrollbar_color = text(),
            "cursor_color" => self.cursor_color = text(),
            "background_effect" => self.background_effect = text(),
            "background_effect_color" => self.background_effect_color = text(),
            "cover_opacity" => self.cover_opacity = number(),
            "show_console_window" => self.show_console_window = flag(),
            "order_mode" => self.order_mode = text(),
            "usage_order" => self.usage_order = readIds(found),
            "user_order" => self.user_order = readIds(found),
            // Unreachable while SETTINGS and this match agree. A new entry in
            // one and not the other does nothing rather than failing to build.
            _ => {}
        }
    }
}

// ========== Reading ==========

/// Reads `config.toml` under `base_dir`, already seeded by
/// `content::ensureLayout`. Unknown keys and unusable values are ignored,
/// leaving that setting at its default.
pub fn load(base_dir: &Path) -> Config {
    let mut config = Config::default();

    // Two `let else` bail-outs: a missing file and unparseable TOML both mean
    // "run entirely on defaults", which `config` already is.
    let Ok(contents) = fs::read_to_string(base_dir.join("config.toml")) else {
        return config;
    };
    let Ok(table) = contents.parse::<toml::Table>() else {
        return config;
    };

    for setting in SETTINGS {
        config.apply(setting, table.get(setting.name));
    }

    // The two keys the palette replaced, honoured only where the new name is
    // absent — so a cartridge written before the trio existed still looks as it
    // did, and the new name wins whenever both are present.
    for (old, new) in [
        ("background_color", "primary_color"),
        ("shadow_color", "secondary_color"),
    ] {
        if !table.contains_key(new)
            && let Some(setting) = SETTINGS.iter().find(|s| s.name == new)
        {
            config.apply(setting, table.get(old));
        }
    }

    config
}

/// A boolean, or the setting's default when `found` is missing or wrong-typed.
fn readFlag(kind: &Kind, found: Option<&toml::Value>) -> bool {
    let Kind::Flag(default) = kind else {
        return false;
    };
    found.and_then(|v| v.as_bool()).unwrap_or(*default)
}

/// A non-negative, finite number, or the setting's default. A `Unit` is then
/// clamped to 0..=1 rather than being rejected above it.
fn readNumber(kind: &Kind, found: Option<&toml::Value>) -> f64 {
    let (default, clamp) = match kind {
        Kind::Number(default) => (*default, false),
        Kind::Unit(default) => (*default, true),
        _ => return 0.0,
    };
    // Integer and Float are separate TOML types, and a config may spell the
    // same knob either way.
    let parsed = match found {
        Some(toml::Value::Integer(n)) => *n as f64,
        Some(toml::Value::Float(f)) => *f,
        _ => default,
    };
    // NaN and infinity would poison the layout arithmetic downstream, and a
    // negative gap is not a gap.
    let value = if parsed.is_finite() && parsed >= 0.0 {
        parsed
    } else {
        default
    };
    if clamp { value.clamp(0.0, 1.0) } else { value }
}

/// A trimmed, non-blank string, or the setting's default. For a `OneOf`, a
/// value outside the allowed set is treated exactly like a missing one.
fn readText(kind: &Kind, found: Option<&toml::Value>) -> String {
    let (default, allowed) = match kind {
        Kind::Color(default) => (*default, None),
        Kind::OneOf(default, allowed) => (*default, Some(*allowed)),
        _ => return String::new(),
    };
    found
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        // `is_none_or`: a Color accepts anything non-blank, while a OneOf has
        // to name a member of its list.
        .filter(|text| allowed.is_none_or(|allowed| allowed.contains(text)))
        .unwrap_or(default)
        .to_string()
}

/// An id list, keeping the entries that are non-negative integers and silently
/// dropping the rest. Only ever "which numbers were in the file".
fn readIds(found: Option<&toml::Value>) -> Vec<usize> {
    let Some(array) = found.and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|entry| entry.as_integer())
        // A negative id is not an index; `try_from` drops it along with
        // anything too large for this platform's usize.
        .filter_map(|id| usize::try_from(id).ok())
        .collect()
}

// ========== Writing ==========

/// An id list as `store` wants it: a TOML array of integers, on one line.
/// A whole catalog fits on one line, and `[2, 0, 1]` says what it means at a
/// glance in a way the same numbers down a column do not.
pub fn ids(list: &[usize]) -> toml_edit::Value {
    let mut array: toml_edit::Array = list.iter().map(|&id| id as i64).collect();
    // Resets the per-entry decor `collect` leaves behind, which is what keeps
    // the array on one line.
    array.fmt();
    array.into()
}

/// Writes one top-level key back to `config.toml` under `base_dir`, leaving
/// every comment, blank line and unrelated value exactly as it was. Only the
/// three order keys ever come through here.
///
/// Never fatal, and reported only as a log line: a cartridge can sit on a
/// write-protected stick.
pub fn store(base_dir: &Path, key: &str, value: toml_edit::Value) {
    let path = base_dir.join("config.toml");
    let Ok(contents) = fs::read_to_string(&path) else {
        log::line(
            base_dir,
            &format!("could not read config.toml to set {key}"),
        );
        return;
    };
    // `toml_edit`, not `toml`: this parse keeps whitespace and comments, which
    // is the whole reason a hand-written file survives a write intact.
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
        // A key the launcher sets for the first time is appended under the same
        // description `syncDefaults` would have used, so it arrives looking like
        // the hand-written ones above it rather than tacked on.
        let description = SETTINGS
            .iter()
            .find(|setting| setting.name == key)
            .map(|setting| setting.description);
        if let (Some(description), Some(mut new_key)) = (description, doc.key_mut(key)) {
            // The leading `\n` is the blank line separating it from the key
            // above; the decor is the text `toml_edit` re-emits before the key.
            new_key
                .leaf_decor_mut()
                .set_prefix(format!("\n# {description}\n"));
        }
    }

    if let Err(error) = fs::write(&path, doc.to_string()) {
        log::line(
            base_dir,
            &format!("could not write {key} to config.toml: {error}"),
        );
    }
}

/// Appends a commented-out, already-in-effect line for every setting missing
/// from the file at `config_path` — the case where the cartridge was set up
/// before that setting existed.
///
/// Nothing about the running launcher changes: the lines are comments, and each
/// names the default that was silently in force anyway. This only makes the
/// setting discoverable. A no-op once every key is present, which is most runs.
pub fn syncDefaults(config_path: &Path) {
    let Ok(contents) = fs::read_to_string(config_path) else {
        return;
    };
    let Ok(table) = contents.parse::<toml::Table>() else {
        return;
    };

    let missing: Vec<_> = SETTINGS
        .iter()
        .filter(|setting| !table.contains_key(setting.name))
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
    for setting in missing {
        addition.push_str(&format!(
            "\n# {}\n# {} = {}\n",
            setting.description,
            setting.name,
            defaultText(&setting.kind)
        ));
    }

    // Appended rather than rewritten, so everything the author wrote above the
    // marker is untouched.
    let _ = fs::write(config_path, contents + &addition);
}

/// One setting's default, formatted exactly as it would appear in the file.
/// Only used when documenting a key an old config is missing.
fn defaultText(kind: &Kind) -> String {
    match kind {
        Kind::Flag(value) => value.to_string(),
        Kind::Number(value) | Kind::Unit(value) => value.to_string(),
        // Quoted, because these go into the file as TOML strings.
        Kind::Color(value) | Kind::OneOf(value, _) => format!("\"{value}\""),
        Kind::Ids => "[]".to_string(),
    }
}
