// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## CONFIG TO CSS VARIABLES ##########

const ui = window.__UI__ || {};
const root_style = document.documentElement.style;
const num = (value, fallback) => (Number.isFinite(value) ? value : fallback);

export const setVar = (name, value) => root_style.setProperty(name, value);

// Duplicated from Rust and MUST stay in step: window::size picks the window
// from these three before this page loads. See src/constants.rs.
export const PAD = num(ui.borderGap, 36);
export const GAP = num(ui.imageGap, 32);
export const TOOLBAR_BAND = num(ui.toolbarBand, 44);

export const MOVE_MS = 350;
export const OUTRO_MS = 320;
// A failure that comes back in milliseconds would otherwise flash and vanish,
// reading as a glitch rather than as an attempt that was made.
export const MIN_LOADING_MS = num(ui.minLoadingAfterFail, 1000);
export const EDGE_ZONE = 60;
export const EDGE_SPEED = 14;
export const TRACK_GAP = 18;

// ========== The Palette ==========
// Mixed here rather than with CSS color-mix(), which needs Chromium 111+ — a
// deployed cartridge can be pinned to a fixed-version WebView2 runtime.

const clamp255 = (n) => Math.max(0, Math.min(255, Math.round(n)));
const hex2 = (n) => clamp255(n).toString(16).padStart(2, "0");
const toHex = (rgb) => "#" + rgb.map(hex2).join("");
const mix = (from, to, t) => from.map((c, i) => c + (to[i] - c) * t);

// Parsed by the engine, so named colours and rgb()/hsl() all work. Null for
// anything it refuses, which is the caller's cue to keep its stylesheet default.
function parseColor(color) {
  // Blank must be rejected up front: assigning "" REMOVES the property rather
  // than failing, so the sentinel below would be wiped and the probe would
  // report whatever it inherits.
  if (!color || !color.trim()) return null;

  const probe = document.createElement("span");
  probe.style.display = "none";
  // A known-bad sentinel first, because an invalid colour leaves the previous
  // value in place rather than erroring.
  probe.style.color = "rgb(1, 2, 3)";
  probe.style.color = color;
  document.body.appendChild(probe);
  const computed = getComputedStyle(probe).color;
  probe.remove();

  const parts = computed.match(/-?[\d.]+/g);
  if (!parts || parts.length < 3) return null;
  const rgb = parts.slice(0, 3).map(Number);
  return rgb.join() === "1,2,3" && color.trim() !== "rgb(1, 2, 3)" ? null : rgb;
}

function luminance([r, g, b]) {
  const channel = (v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// Nudged toward `target` until it reads clearly against `against`. Each
// candidate is rounded to 8 bits BEFORE it is measured, so the ratio checked is
// the ratio of the colour that actually ships.
function liftUntil(colour, target, against, min_ratio) {
  for (let t = 0; t <= 1.0001; t += 0.04) {
    const candidate = mix(colour, target, t).map(clamp255);
    if (contrast(candidate, against) >= min_ratio) return candidate;
  }
  return target.map(clamp255);
}

const primary = parseColor(ui.primaryColor || "") || [25, 19, 37];
const secondary = parseColor(ui.secondaryColor || "") || [61, 31, 55];
const accent = parseColor(ui.accentColor || "") || [146, 94, 55];

// Not pure white/black: an absolute endpoint flattens the top and reads harsher
// than the field it sits on.
const INK_LIGHT = [242, 242, 240];
const INK_DARK = [18, 18, 20];

// 0.18 is where the two inks are equally readable — solved from the constants
// above, not the middle of the range. A mid grey at #808080 sits at 0.22 and
// genuinely wants dark text.
const light_primary = luminance(primary) > 0.18;
const ink = light_primary ? INK_DARK : INK_LIGHT;

// An accent picked to look right as a fill is routinely too close to the
// primary to read as small text; a secondary picked as a shadow too close to
// show as a hairline. Each is lifted just far enough and no further.
const text = liftUntil(accent, ink, primary, 4.5);
const line = liftUntil(secondary, accent, primary, 2.0);

setVar("--primary", toHex(primary));
setVar("--secondary", toHex(secondary));
setVar("--accent", toHex(accent));
setVar("--text", toHex(text));
setVar("--line", toHex(line));
setVar("--plate", toHex(secondary));
// The channels alone, so the stylesheet can build translucent versions without
// this file naming each one.
setVar("--primary-rgb", primary.map(clamp255).join(", "));
document.documentElement.dataset.ink = light_primary ? "dark" : "light";

// ========== The Type Grid ==========
// Departure Mono is a pixel face: crisp at 11px and multiples of it, mushy
// between. Rounded against DEVICE pixels, which is the whole reason this is
// computed here — an 11px CSS rule is 13.75 device px at 125% scaling.

const dpr = window.devicePixelRatio || 1;
export const TYPE_UNIT = Math.round(11 * dpr) / dpr;
setVar("--type-unit", TYPE_UNIT.toFixed(3) + "px");

// ========== The Remaining Knobs ==========

const px = (name, value) => { if (Number.isFinite(value)) setVar(name, value + "px"); };
// Blank means "take it from the palette".
const orPalette = (name, value, fallback) => setVar(name, (value || "").trim() || fallback);

px("--border-gap", ui.borderGap);
px("--image-gap", ui.imageGap);
px("--corner-radius", ui.cornerRadius);
px("--toolbar-band", ui.toolbarBand);
px("--loading-text-gap", ui.loadingTextGap);
px("--error-border-width", ui.errorBorderWidth);

orPalette("--toolbar-color", ui.toolbarColor, toHex(text));
orPalette("--scrollbar-color", ui.scrollbarColor, toHex(line));
orPalette("--loading-ring-color", ui.loadingRingColor, toHex(accent));
orPalette("--loading-text-color", ui.loadingTextColor, toHex(text));

if (ui.overlayColor) setVar("--overlay-color", ui.overlayColor);
if (ui.errorBorderColor) setVar("--error-border-color", ui.errorBorderColor);
if (ui.errorTextColor) setVar("--error-text-color", ui.errorTextColor);
if (ui.missingSignColor) setVar("--missing-sign-color", ui.missingSignColor);
if (Number.isFinite(ui.missingDim)) setVar("--missing-dim", ui.missingDim);

// The shadow is the secondary — that is what the 30% of the palette is for.
// Solid for `fade` px out from the cover edge, then blurred to nothing,
// reaching exactly `size` and no further.
const shadow_size = num(ui.shadowSize, 24);
const spread = Math.max(0, Math.min(num(ui.shadowFade, 0), shadow_size));
setVar("--shadow", `0 0 ${shadow_size - spread}px ${spread}px ${toHex(secondary)}`);
