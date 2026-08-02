// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.
//
// ── The launcher's page script ────────────────────────────────────────────
// Served over app:// alongside index.html and style.css. In dev these are read
// live from src/, so edits show up on the next launch with no rebuild.
//
// Everything this file reads about the outside world arrives as four globals
// (__GAMES__, __SHOW_CAPTIONS__, __UI__, __ORDER__) set by Rust's
// init_script. That runs through WebView2's AddScriptToExecuteOnDocumentCreated,
// which fires before the parser produces any DOM, so they are always defined by
// the time this file runs.
//
// PAD, GAP and TOOLBAR_BAND below are duplicated from Rust and MUST stay in
// step with it: window::size picks the window from those same three numbers
// before this page loads, and layout() then fits the covers into it. See
// src/constants.rs, whose header explains why the duplication is deliberate.
// style.css carries the same three as --border-gap / --image-gap /
// --toolbar-band.

// Look-and-feel settings from config.toml (see Rust's init_script). Fall
// back to the same defaults as style.css when a value is absent.
const ui = window.__UI__ || {};

// Border of empty space between the window edge and the covers, the static
// gap between covers, and the toolbar strip along the top. All three feed
// the layout math below and MUST match Rust's window sizing (border_gap /
// image_gap / TOOLBAR_BAND) and style.css.
const PAD = Number.isFinite(ui.borderGap) ? ui.borderGap : 36;
const GAP = Number.isFinite(ui.imageGap) ? ui.imageGap : 32;
const TOOLBAR_BAND = Number.isFinite(ui.toolbarBand) ? ui.toolbarBand : 44;

// Transition lengths, in step with style.css.
const MOVE_MS = 350;   // the chosen cover travelling to the centre
const OUTRO_MS = 320;  // fade to black once the game is up
// Shortest time the loading state stays up before a failure unwinds it.
// Some failures come back in a few milliseconds — a missing file needs no
// waiting at all — and an indicator that flashes and vanishes reads as a glitch
// rather than as an attempt that was made and didn't work. Set in Rust
// (constants::MIN_LOADING_AFTER_FAIL), not in config.toml; the number here
// is only the fallback for a page served without it.
const MIN_LOADING_MS = Number.isFinite(ui.minLoadingAfterFail)
  ? ui.minLoadingAfterFail
  : 1000;

// How close to the edge of the gallery a drag has to get before the row
// starts scrolling under it, and how fast it then goes (px per frame).
const EDGE_ZONE = 60;
const EDGE_SPEED = 14;

// How far under the cover's bottom edge the progress line sits.
const TRACK_GAP = 18;

// ── The palette ────────────────────────────────────────────────────
// Three colours from config.toml carry the whole launcher, in the 60/30/10
// proportions the rule is named for:
//
//   primary    60%  the window behind everything
//   secondary  30%  shadows, borders, the plate behind missing art
//   accent     10%  text, the selected cover, the close button
//
// Two of the shades below are NOT the raw colour, and both for a measured
// reason rather than a stylistic one. An accent chosen to look right as a fill
// is routinely too close to the primary to read as small text on it, and a
// secondary chosen to look right as a shadow is routinely too close to be seen
// as a hairline. So each is lifted just far enough to clear a contrast
// threshold and no further — the palette the owner set, at the strength the
// role actually needs. Change any of the three and every shade follows.
//
// Mixed HERE rather than with CSS color-mix(), which needs Chromium 111+. A
// deployed cartridge can be pinned to a fixed-version WebView2 runtime, and
// this file is already setting these variables — producing concrete values
// costs a few lines and removes the version dependency entirely.

const clamp255 = (n) => Math.max(0, Math.min(255, Math.round(n)));
const hex2 = (n) => clamp255(n).toString(16).padStart(2, "0");
const toHex = (rgb) => "#" + rgb.map(hex2).join("");
const mix = (from, to, t) => from.map((c, i) => c + (to[i] - c) * t);

// Any CSS colour string in, [r, g, b] out — via the engine, so named colours
// and rgb()/hsl() all work and there is no parser here to get wrong. Returns
// null for anything the engine refuses, which is the caller's cue to keep its
// stylesheet default rather than mix from a colour nobody can read.
function parseColor(color) {
  // Blank has to be rejected up front, not left to the probe. Assigning "" to
  // a style property REMOVES it rather than failing, so the sentinel below
  // would be wiped and the probe would report whatever it inherits — which
  // under `color-scheme: dark` is white. A config that leaves a colour blank
  // (the way every "derive this" key does) would then get white rather than
  // its fallback.
  if (!color || !color.trim()) return null;

  const probe = document.createElement("span");
  probe.style.display = "none";
  // Set twice: assigning an invalid colour leaves the previous value in place,
  // so a known-bad sentinel first means "unchanged" reliably reads as "the
  // engine rejected it" rather than as a coincidental match.
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

// Perceived lightness, sRGB-linearised. Only ever compared against a midpoint,
// so this decides one thing: whether "ink" means white or near-black.
function luminance([r, g, b]) {
  const channel = (v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

// WCAG's contrast ratio between two colours. Used to decide how far a shade
// has to be lifted, so the thresholds below are the published ones rather than
// numbers that looked about right.
function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// `colour` nudged toward `target` in small steps, stopping at the first one
// that reads clearly enough against `against`. Returns the raw colour untouched
// when it already does — a palette that needs no help gets none.
//
// Each candidate is rounded to 8 bits BEFORE it is measured, so the ratio
// checked here is the ratio of the colour that actually ships. Measuring the
// un-rounded mix can pick a shade that clears the threshold by a hair and then
// falls back under it once written as hex.
function liftUntil(colour, target, against, minRatio) {
  for (let t = 0; t <= 1.0001; t += 0.04) {
    const candidate = mix(colour, target, t).map(clamp255);
    if (contrast(candidate, against) >= minRatio) return candidate;
  }
  return target.map(clamp255);
}

// The three, each falling back to its stylesheet default if the engine won't
// read what the config said.
const primary = parseColor(ui.primaryColor || "") || [25, 19, 37];
const secondary = parseColor(ui.secondaryColor || "") || [61, 31, 55];
const accent = parseColor(ui.accentColor || "") || [146, 94, 55];

// The two ends of the ramp. Not pure #ffffff/#000000: an absolute endpoint
// flattens the top and reads harsher than the field it sits on.
const INK_LIGHT = [242, 242, 240];
const INK_DARK = [18, 18, 20];

// Which way "lighter" runs. On a dark primary a shade is lifted toward white,
// on a pale one toward near-black — so a light palette works without any
// further edits.
//
// The threshold is where the two inks are equally readable, not the middle of
// the range. Solving the contrast ratio for the crossover between the two
// constants above puts it near 0.18. An intuitive-looking 0.5 would hand
// mid-tones the light ink well past the point where the dark one is easier to
// read — a mid grey at #808080 sits at 0.22 and genuinely wants dark text.
const lightPrimary = luminance(primary) > 0.18;
const ink = lightPrimary ? INK_DARK : INK_LIGHT;

// Text is the accent, lifted until it clears AA for body copy (4.5:1). A
// caramel that looks right filling the close button measures 3.34:1 against a
// dark violet — fine for a control, unreadable as a 13px label in a display
// face. This keeps the hue and buys back the legibility.
const text = liftUntil(accent, ink, primary, 4.5);
// Borders are the secondary, lifted toward the accent until a hairline is
// actually visible. A plum picked as a shadow measures 1.25:1 on its own
// violet, which draws nothing at all.
const line = liftUntil(secondary, accent, primary, 2.0);

// Push the visual knobs into the CSS variables the stylesheet reads.
const rootStyle = document.documentElement.style;
const setVar = (name, value) => rootStyle.setProperty(name, value);

setVar("--primary", toHex(primary));
setVar("--secondary", toHex(secondary));
setVar("--accent", toHex(accent));
setVar("--text", toHex(text));
setVar("--line", toHex(line));
// The plate behind a cover whose art didn't load: a fill, so the secondary at
// full strength is exactly right.
setVar("--plate", toHex(secondary));
// The primary's channels on their own, so the stylesheet can build translucent
// versions of it — the veil over an unselected cover, the grip pill, the track
// behind the progress line — without this file naming each one.
setVar("--primary-rgb", primary.map(clamp255).join(", "));
// Which way round the ramp runs, for the handful of rules that can't express
// themselves as a mix — see style.css's :root[data-ink] block.
document.documentElement.dataset.ink = lightPrimary ? "dark" : "light";

if (Number.isFinite(ui.borderGap)) setVar("--border-gap", ui.borderGap + "px");
if (Number.isFinite(ui.imageGap)) setVar("--image-gap", ui.imageGap + "px");
if (Number.isFinite(ui.cornerRadius)) setVar("--corner-radius", ui.cornerRadius + "px");
if (Number.isFinite(ui.toolbarBand)) setVar("--toolbar-band", ui.toolbarBand + "px");
// Both blank by default, meaning "take it from the palette". The toolbar reads
// as text so it gets the text shade; the scrollbar is a hairline so it gets the
// border one. Naming either in config.toml still wins.
setVar("--toolbar-color", (ui.toolbarColor || "").trim() || toHex(text));
setVar("--scrollbar-color", (ui.scrollbarColor || "").trim() || toHex(line));
if (ui.overlayColor) setVar("--overlay-color", ui.overlayColor);
// The progress line and its status text, over the darkened screen. Blank takes
// them from the palette: the line at full accent (it is a fill), the text at
// the readable shade. They were `#ffffff` and white-at-40%, which is only ever
// right over a dark scrim.
setVar("--loading-ring-color", (ui.loadingRingColor || "").trim() || toHex(accent));
setVar("--loading-text-color", (ui.loadingTextColor || "").trim() || toHex(text));
if (Number.isFinite(ui.loadingTextGap)) setVar("--loading-text-gap", ui.loadingTextGap + "px");
if (ui.errorBorderColor) setVar("--error-border-color", ui.errorBorderColor);
if (Number.isFinite(ui.errorBorderWidth)) setVar("--error-border-width", ui.errorBorderWidth + "px");
if (ui.errorTextColor) setVar("--error-text-color", ui.errorTextColor);
if (ui.missingSignColor) setVar("--missing-sign-color", ui.missingSignColor);
if (Number.isFinite(ui.missingDim)) setVar("--missing-dim", ui.missingDim);
// Symmetric shadow bounded by `size`: it's solid for `fade` px out from the
// cover edge (the box-shadow spread), then blurs to nothing over the rest,
// reaching exactly `size` and no further. blur = size - fade.
const shadowSize = Number.isFinite(ui.shadowSize) ? ui.shadowSize : 24;
const shadowFade = Number.isFinite(ui.shadowFade) ? ui.shadowFade : 0;
// The shadow is the secondary — that is what the 30% of the palette is for. It
// used to be its own `shadow_color` key, which meant the one colour most
// responsible for the launcher's depth was set independently of everything
// around it and drifted the moment the background changed.
const shadowColor = toHex(secondary);
const spread = Math.max(0, Math.min(shadowFade, shadowSize));
const blur = Math.max(0, shadowSize - spread);
setVar("--shadow", `0 0 ${blur}px ${spread}px ${shadowColor}`);

const games = window.__GAMES__ || [];
const showCaptions = window.__SHOW_CAPTIONS__ === true;
const stored = window.__ORDER__ || {};
const grid = document.getElementById("grid");
const gallery = document.getElementById("gallery");
const toolbarLeft = document.getElementById("toolbar-left");
const modeGroup = document.getElementById("mode");
const modePill = document.getElementById("mode-pill");
const modeButtons = Array.from(modeGroup.querySelectorAll("button"));
const arrangeBtn = document.getElementById("arrange");
const searchBox = document.getElementById("search");
const nameplate = document.getElementById("nameplate");
const scrollbar = document.getElementById("scrollbar");
const thumb = document.getElementById("scrollbar-thumb");
const scrim = document.getElementById("scrim");
// Not named `status`: that would shadow window.status.
const statusLine = document.getElementById("status");

const send = (message) => window.ipc.postMessage(message);

// An empty catalog is a normal state, not a failure — a cartridge nobody
// has put a game on yet. Say so, rather than showing an empty window that
// looks like the page failed to load. The toolbar goes with the covers:
// there is nothing to order and nothing to search.
//
// Everything below still runs unchanged: the card loop, layout() and the
// preload barrier all iterate an empty list and do nothing.
// The toolbar's CONTENTS go, not the row. The close button lives in that row
// now, and an empty cartridge still has to be closeable — hiding the bar
// outright, the way this used to, would take the only way out with it.
if (games.length === 0) {
  gallery.style.display = "none";
  toolbarLeft.style.visibility = "hidden";
  searchBox.style.visibility = "hidden";
  document.body.classList.add("empty");
} else if (showCaptions) {
  document.body.classList.add("captioned");
}

// "idle" or "launching". One game at a time: while a launch is in flight
// every other cover is on its way off screen anyway.
let state = "idle";
// When the current launch began, for MIN_LOADING_MS.
let launchedAt = 0;
// Whether the covers are being arranged by hand rather than offered.
let arranging = false;
// Whether the whole row is wider than the window — measured unfiltered, so
// it is a fact about the cartridge rather than about what has been typed.
let overflowing = false;

const cards = [];
const imgs = games.map((game, index) => {
  const card = document.createElement("button");
  card.className = "card";
  card.type = "button";

  const cover = document.createElement("span");
  cover.className = "cover";

  const img = document.createElement("img");
  img.src = game.image;
  img.alt = game.name;
  cover.appendChild(img);

  const sign = document.createElement("span");
  sign.className = "sign";
  cover.appendChild(sign);

  // Only ever visible while arranging; built here so entering that mode is
  // a class on the body rather than a pass over the DOM.
  const grip = document.createElement("span");
  grip.className = "grip";
  grip.innerHTML =
    '<svg width="18" height="4" viewBox="0 0 18 4" aria-hidden="true">' +
    '<circle cx="2" cy="2" r="1.6" /><circle cx="9" cy="2" r="1.6" />' +
    '<circle cx="16" cy="2" r="1.6" /></svg>';
  cover.appendChild(grip);
  card.appendChild(cover);

  // No per-card caption any more: the name of whichever cover is selected is
  // shown once, on the shared line under the row. See #nameplate.

  const note = document.createElement("span");
  note.className = "note";
  card.appendChild(note);

  // A game whose exe isn't on the cartridge can't be chosen at all: Rust
  // checked at startup, so this is settled before the player touches
  // anything rather than being discovered by a click that does nothing.
  if (game.available === false) {
    card.classList.add("unavailable");
    card.disabled = true;
    card.title = game.name + " — missing " + game.exe;
    note.textContent = "Game files missing";
    // No select-on-hover here, and none is possible: a disabled button
    // receives no pointer events at all. The arrow keys skip it for the same
    // reason, so an absent game is never the selected one — its own sign and
    // note already say what it is, and its name stays in the tooltip.
  } else {
    card.title = game.name;
    card.addEventListener("click", () => beginLaunch(index));
    card.addEventListener("pointerenter", () => select(index));
    // Focus and hover feed the same one index — see select(). Without this a
    // cover reached with the arrow keys would be outlined but not lifted, and
    // the name line would still be describing whatever the mouse last passed
    // over, which is the wrong cover to name.
    card.addEventListener("focus", () => select(index));

    // Cover art that can't be loaded — wrong path in catalog.json, file
    // never copied, a format the webview won't decode — otherwise leaves a
    // bare dark rectangle that reads as placeholder art. Borrow the missing
    // game's dim-and-sign treatment to say the cover is the thing that's
    // absent. The card stays clickable: the game itself is on the cartridge
    // and still runs. Only set for a game that IS available — one whose exe
    // is missing already carries the more important message.
    img.addEventListener("error", () => {
      card.classList.add("unavailable", "no-cover");
      note.textContent = "Cover missing";
      // Clearing alt so the webview doesn't lay the alt text out where the
      // picture would have been — it spills out of the cover's box and over
      // the card above it. The sign and the note carry the meaning now.
      img.alt = "";
    });
  }

  card.addEventListener("pointerdown", (event) => beginDrag(event, index));

  grid.appendChild(card);
  cards.push(card);
  return img;
});

// ── Which cover is being pointed at ────────────────────────────────
// One index, fed by both the mouse and the keyboard, and the only thing that
// decides which cover is lifted and clear of its veil and whose name is on the
// line under the row.
//
// Deliberately ONE index rather than a hover state and a focus state kept
// separately: those two can disagree, and when they do the row lifts one cover
// while the name line describes another. Focus sets it (see the `focus`
// listener on each card, which the arrow-key handler triggers by calling
// .focus()), hover sets it, last one wins. The focus RING is still drawn
// independently by :focus-visible, so a keyboard user keeps the outline that
// tells them where the keyboard is — the two coexist instead of competing.

let selected = -1;
// Whether the selected cover's own note is occupying the name line's spot.
// Settled on selection change; read on every scroll frame.
let noteCoversName = false;

function select(index) {
  if (index === selected) return;
  if (index >= 0 && (!cards[index] || cards[index].disabled)) return;

  if (cards[selected]) cards[selected].classList.remove("selected");
  selected = index;
  if (cards[selected]) cards[selected].classList.add("selected");

  nameplate.textContent = index >= 0 ? games[index].name : "";

  yieldToNote();
  placeNameplate();
}

// A cover that is missing or failed already has a message in exactly the spot
// the name would take, and that message is the more important of the two.
//
// Settled here rather than inside placeNameplate(), which runs on every scroll
// event: a getComputedStyle() call there would force a style flush per frame
// on a webview with no GPU to spare. Called again when a launch fails, because
// that is the one time a note appears on a cover that is already selected.
function yieldToNote() {
  const note = cards[selected] && cards[selected].querySelector(".note");
  noteCoversName = !!note && note.textContent !== "" &&
    getComputedStyle(note).display !== "none";
  nameplate.style.visibility = noteCoversName ? "hidden" : "";
}

// The name sits under the cover it names, not in the middle of the window.
//
// Centred on the window was the first attempt and it actively misinforms: with
// the leftmost cover selected the name appears under the middle one, which is
// the one thing a label must never do. It follows the cover instead, clamped
// to the window so a long name at either end stays on screen.
//
// Also called on scroll, because the row moves under a stationary selection —
// so it stays cheap: two layout reads and a write, no style queries. Whether
// the name is showing at all is settled in select().
function placeNameplate() {
  if (selected < 0 || !cards[selected] || noteCoversName) return;

  const rect = cards[selected].getBoundingClientRect();
  const half = nameplate.offsetWidth / 2;
  const centre = rect.left + rect.width / 2;
  // Clamped inside the border gap, so a long name on the cover at either end
  // slides along rather than running off the window.
  const clamped = Math.max(PAD + half, Math.min(window.innerWidth - PAD - half, centre));
  nameplate.style.left = (clamped - half).toFixed(1) + "px";
}

// The name line lives in the border gap under the row, which is a fixed
// height the covers are not allowed to eat into. Rather than pick a font size
// and hope it fits, take it from the gap that is actually there: a cartridge
// with a tight border_gap gets a smaller line instead of a clipped one.
function sizeNameplate() {
  // Must agree with #nameplate's `height` in style.css, which is the box this
  // is sizing text to fit.
  const band = Math.max(0, PAD - 17);
  setVar("--nameplate-size", Math.max(11, Math.min(22, band * 0.85)).toFixed(1) + "px");
}

// Fit every cover to the height of the window under the toolbar, at its
// native aspect ratio and never scaled up past native.
//
// Note what is NOT in here: the number of games. Covers used to get 1/n of
// the width each, which meant a big catalog shrank every cover in it until
// none of them could be read. Now the row simply runs off the side and
// #gallery scrolls to the rest — the covers you can see are always the size
// they were meant to be. Rust's window::size reproduces this from the same
// three numbers to pick the window it all has to fit in.
function layout() {
  // One PAD, not two. TOOLBAR_BAND is the toolbar's height plus the gap under
  // it, so the top of the window is already accounted for; the single margin
  // is the bottom one, where the name line and the scrollbar live. Must match
  // window::size's height_room exactly or the covers won't fit the window Rust
  // picked for them.
  const availH = window.innerHeight - PAD - TOOLBAR_BAND;

  // The empty state's ghost plates, at the size a real cover would have been.
  // Native 600x900 rather than a measured image, because with no games there
  // is no image to measure — the same two numbers constants.rs sizes the
  // window from (COVER_NATIVE_WIDTH / COVER_NATIVE_HEIGHT).
  if (games.length === 0) {
    const scale = Math.min(1, availH / 900);
    setVar("--plate-width", Math.floor(600 * scale) + "px");
    setVar("--plate-height", Math.floor(900 * scale) + "px");
    return;
  }

  imgs.forEach((img, index) => {
    const nW = img.naturalWidth || 600;
    const nH = img.naturalHeight || 900;
    const scale = Math.min(1, availH / nH);
    const width = Math.floor(nW * scale);
    img.style.width = width + "px";
    img.style.height = Math.floor(nH * scale) + "px";
    // Keep the "missing" sign's stroke proportional to the cover it's
    // drawn on, so it reads the same on a small screen as a large one.
    cards[index].style.setProperty("--sign-stroke", Math.max(3, Math.round(width * 0.018)) + "px");
  });

  updateOverflow();
  updateScrollbar();
  placeNameplate();
}

// Whether every cover fits at once, and so whether a search box is worth
// offering.
//
// Added up from the cover widths rather than read off grid.scrollWidth,
// because scrollWidth only counts what the search has left showing. Narrow
// the results to two covers and a measured row would say "everything fits"
// and take away the box you need to get back.
function updateOverflow() {
  let total = 2 * PAD + Math.max(0, imgs.length - 1) * GAP;
  imgs.forEach((img) => { total += img.offsetWidth; });

  overflowing = total > gallery.clientWidth + 1;
  updateSearchVisibility();
}

// The one place the search box is shown or hidden. Two conditions, and both
// have to go through here: an inline `display` set anywhere else would beat
// any stylesheet rule the other one tried to use.
function updateSearchVisibility() {
  searchBox.style.display = overflowing && !arranging ? "block" : "none";
}

// ── Scrolling ──────────────────────────────────────────────────────

function updateScrollbar() {
  const visible = gallery.clientWidth;
  const total = gallery.scrollWidth;
  // This one IS the live row: the bar describes where you are in what is
  // currently on screen, so a filtered row that fits has nothing to say.
  const scrollable = total > visible + 1;
  document.body.classList.toggle("scrollable", scrollable);
  if (!scrollable) return;

  const ratio = visible / total;
  thumb.style.width = (ratio * 100).toFixed(3) + "%";
  thumb.style.left = ((gallery.scrollLeft / total) * 100).toFixed(3) + "%";
}

gallery.addEventListener("scroll", () => {
  updateScrollbar();
  // The selection has not changed but the cover it points at has moved.
  placeNameplate();
});

// A wheel is the obvious way to move a row of covers and points the wrong
// way for it. Only when there is somewhere to go, so an ordinary scroll
// over a row that fits does nothing rather than fighting the page.
gallery.addEventListener("wheel", (event) => {
  if (gallery.scrollWidth <= gallery.clientWidth) return;
  if (event.deltaY === 0) return;
  gallery.scrollLeft += event.deltaY;
  event.preventDefault();
}, { passive: false });

// Dragging the bar itself. Cheap to add and the first thing a mouse
// reaches for once there is a bar to reach for.
thumb.addEventListener("pointerdown", (event) => {
  const startX = event.clientX;
  const startScroll = gallery.scrollLeft;
  const track = scrollbar.clientWidth;
  const move = (moved) => {
    // A pixel of bar is worth scrollWidth/track pixels of row.
    gallery.scrollLeft = startScroll + (moved.clientX - startX) * (gallery.scrollWidth / track);
  };
  const stop = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", stop);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", stop);
  event.preventDefault();
});

// A single line of buttons, which Tab alone walks badly. Left/right move
// along it and bring the next cover into view.
document.addEventListener("keydown", (event) => {
  if (state !== "idle") return;
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  if (document.activeElement === searchBox) return;
  // The order control uses the same keys to move between its segments. It
  // stops the event itself, so this is the belt to that pair of braces —
  // focus resting on the group rather than on one of its buttons would
  // otherwise fall through to here and walk the row instead.
  if (modeGroup.contains(document.activeElement)) return;

  const reachable = order.filter(
    (id) => !cards[id].classList.contains("hidden") && !cards[id].disabled
  );
  if (reachable.length === 0) return;

  const at = reachable.indexOf(cards.indexOf(document.activeElement));
  const step = event.key === "ArrowRight" ? 1 : -1;
  // From nowhere in the row, either arrow enters it at the near end.
  const next = at === -1
    ? (step === 1 ? 0 : reachable.length - 1)
    : Math.min(reachable.length - 1, Math.max(0, at + step));

  cards[reachable[next]].focus();
  cards[reachable[next]].scrollIntoView({ block: "nearest", inline: "nearest" });
  event.preventDefault();
});

// ── What order the covers are in ───────────────────────────────────

// The ids, left to right, as currently shown.
let order = [];
let mode = typeof stored.mode === "string" ? stored.mode : "usage";

// A stored id list turned into a complete permutation of 0..games.length.
// The same rule as Rust's order::normalize — see that module for why it is
// written down in both places.
function normalizeOrder(list) {
  const seen = new Array(games.length).fill(false);
  const result = [];
  for (const id of Array.isArray(list) ? list : []) {
    if (Number.isInteger(id) && id >= 0 && id < games.length && !seen[id]) {
      seen[id] = true;
      result.push(id);
    }
  }
  games.forEach((_, id) => { if (!seen[id]) result.push(id); });
  return result;
}

function computeOrder() {
  if (mode === "catalog") return games.map((_, id) => id);
  if (mode === "alphabetic") {
    return games
      .map((_, id) => id)
      .sort((a, b) => games[a].name.localeCompare(
        games[b].name, undefined, { sensitivity: "base", numeric: true }
      ));
  }
  return normalizeOrder(mode === "user" ? stored.user : stored.usage);
}

// Re-appending a card that is already in #grid moves it, so this reorders
// the row without rebuilding anything. The cards[] and imgs[] arrays are
// deliberately NOT touched: they stay indexed by catalog id, which is what
// `launch:<id>` and __launchOutcome(id, …) speak. Reordering the DOM rather
// than shuffling an index also keeps Tab order the same as reading order.
function applyOrder() {
  order = computeOrder();
  order.forEach((id) => grid.appendChild(cards[id]));
  document.body.classList.toggle("user-order", mode === "user");
  updateScrollbar();
}

// ── The order control ──────────────────────────────────────────────

const MODES = modeButtons.map((button) => button.dataset.mode);
if (!MODES.includes(mode)) mode = "usage";

// Every segment is the width of the widest label, so the pill that marks the
// active one is a single fixed box that only ever moves. Measured rather than
// guessed: the labels are text and their width depends on the font the system
// actually resolved, not on what this file expects it to be.
//
// Measured with the track's own width cleared first, because a second pass
// (the toolbar reflowing when the search box appears) would otherwise measure
// the equalised widths it set last time and slowly ratchet them upward.
function sizeSegments() {
  setVar("--segment-width", "auto");
  const widest = modeButtons.reduce((most, b) => Math.max(most, b.offsetWidth), 0);
  setVar("--segment-width", widest + "px");
  moveModePill();
}

// The pill's position, in whole segments from the left. A transform, so the
// slide runs on the compositor and never touches layout.
function moveModePill() {
  const at = MODES.indexOf(mode);
  const width = modeButtons[0] ? modeButtons[0].offsetWidth : 0;
  setVar("--segment-offset", Math.max(0, at) * width + "px");
}

function setMode(next, announce) {
  // Switching to "user" with nothing stored starts from the row as it is
  // on screen, so arranging by hand begins where you were looking rather
  // than from a jump back to catalog order.
  //
  // Only on a real switch. At startup `order` has not been computed yet, so
  // this would seed the stored order from an empty array — and a launcher
  // that wrote an empty user_order to the cartridge every time it opened in
  // "user" mode would be answering a question nobody asked.
  if (announce && next === "user" && (!Array.isArray(stored.user) || stored.user.length === 0)) {
    stored.user = order.slice();
    send("order:" + stored.user.join(","));
  }
  mode = next;
  modeButtons.forEach((button) => {
    button.setAttribute("aria-checked", String(button.dataset.mode === mode));
    // Only the active segment is a tab stop, which is how a radiogroup is
    // meant to behave: Tab reaches the control, the arrow keys move within it.
    button.tabIndex = button.dataset.mode === mode ? 0 : -1;
  });
  moveModePill();
  if (mode !== "user") setArranging(false);
  applyOrder();
  if (announce) send("mode:" + mode);
}

modeButtons.forEach((button) => {
  button.addEventListener("click", () => setMode(button.dataset.mode, true));
});

// Left/right inside the group move between segments, as they do in any radio
// group. This listener also stops the event: the document-level handler below
// moves the SELECTED COVER on the same keys, and without this both would run
// and one arrow press would change the order mode and jump along the row.
modeGroup.addEventListener("keydown", (event) => {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  const at = MODES.indexOf(mode);
  const step = event.key === "ArrowRight" ? 1 : -1;
  const next = (at + step + MODES.length) % MODES.length;
  setMode(MODES[next], true);
  modeButtons[next].focus();
  event.preventDefault();
  event.stopPropagation();
});

// ── Finding a cover ────────────────────────────────────────────────

// Case- and accent-insensitive, so "pokemon" finds "Pokémon".
const plain = (text) =>
  text.normalize("NFD").replace(/\p{Diacritic}/gu, "").toLowerCase();

const searchable = games.map((game) => plain(game.name));

function applyFilter() {
  const needle = plain(searchBox.value.trim());
  cards.forEach((card, id) => {
    card.classList.toggle("hidden", needle !== "" && !searchable[id].includes(needle));
  });
  gallery.scrollLeft = 0;
  updateScrollbar();
}

searchBox.addEventListener("input", applyFilter);
searchBox.addEventListener("keydown", (event) => {
  // Escape clears rather than closing the launcher, which is what it would
  // otherwise be reaching for.
  if (event.key === "Escape" && searchBox.value !== "") {
    searchBox.value = "";
    applyFilter();
    event.stopPropagation();
  }
});

// ── Arranging the row by hand ──────────────────────────────────────

function setArranging(on) {
  arranging = on;
  document.body.classList.toggle("arranging", on);
  arrangeBtn.setAttribute("aria-pressed", String(on));

  // A missing game's cover still holds a place in the row and still has to
  // be draggable. A disabled button receives no pointer events at all, so
  // it has to stop being disabled for as long as a press means "pick this
  // up" rather than "start this" — beginLaunch's own guard is what keeps it
  // unplayable in the meantime.
  cards.forEach((card, id) => {
    card.disabled = on ? false : games[id].available === false;
  });

  // Arranging a filtered row would write an order for covers that aren't
  // all on screen. Clearing the search — and putting it away for the
  // duration — is the honest way out of that.
  if (on && searchBox.value !== "") {
    searchBox.value = "";
    applyFilter();
  }
  updateSearchVisibility();
}

arrangeBtn.addEventListener("click", () => setArranging(!arranging));

// Where each visible card sat when the drag began, and where the dragged
// one has got to. Null whenever nothing is being dragged.
let drag = null;
let edgeTimer = null;

function beginDrag(event, id) {
  if (!arranging || state !== "idle" || event.button !== 0 || drag) return;

  // Every visible card's slot, in display order, measured once. They are
  // viewport coordinates and the row may scroll underneath them during the
  // drag — which cancels out, because every slot moves by the same amount
  // and only their differences are ever compared.
  const visible = order.filter((other) => !cards[other].classList.contains("hidden"));
  const slots = visible.map((other) => {
    const rect = cards[other].getBoundingClientRect();
    return { id: other, width: rect.width, centre: rect.left + rect.width / 2 };
  });
  const from = visible.indexOf(id);
  if (from === -1) return;

  drag = {
    id,
    card: cards[id],
    pointerId: event.pointerId,
    startX: event.clientX,
    pointerX: event.clientX,
    startScroll: gallery.scrollLeft,
    slots,
    from,
    to: from,
    moved: false,
  };
  drag.card.setPointerCapture(event.pointerId);
  drag.card.classList.add("dragging");
  event.preventDefault();
}

document.addEventListener("pointermove", (event) => {
  if (!drag || event.pointerId !== drag.pointerId) return;
  drag.pointerX = event.clientX;
  if (Math.abs(event.clientX - drag.startX) > 3) drag.moved = true;
  updateDrag();
  autoScroll();
});

// Everything that has to be true of the row for the pointer's current
// position. Called from pointermove and again on every frame the row is
// auto-scrolling — during which the pointer is stationary and sends no
// events of its own, but the row moving under it still changes where the
// card is being dropped.
function updateDrag() {
  const dx = drag.pointerX - drag.startX;

  // The card follows the pointer. The scroll delta is added because the
  // card's own resting position has slid with the row, and the pointer
  // hasn't.
  const scrolled = gallery.scrollLeft - drag.startScroll;
  drag.card.style.transform = `translateX(${dx + scrolled}px)`;

  // Where it has got to, expressed in the frame the slots were measured in
  // — which is the frame from before the row scrolled. The slots have since
  // slid `scrolled` px to the left of where they were recorded, so bringing
  // the card back into their frame means adding it rather than subtracting
  // it. This is what lets a drag held against the edge keep advancing while
  // fresh covers scroll in under it.
  const centre = drag.slots[drag.from].centre + dx + scrolled;
  let to = drag.from;
  while (to > 0 && centre < drag.slots[to - 1].centre) to--;
  while (to < drag.slots.length - 1 && centre > drag.slots[to + 1].centre) to++;

  if (to !== drag.to) {
    drag.to = to;
    shiftSlots();
  }
}

// Opens a gap at `drag.to` by sliding everything between there and the
// card's original slot across by exactly the space the card took up —
// which is what closing up behind it and opening up in front of it comes
// to, whatever the covers' individual widths.
function shiftSlots() {
  const shift = drag.slots[drag.from].width + GAP;
  drag.slots.forEach((slot, at) => {
    if (at === drag.from) return;
    let offset = 0;
    if (drag.from < drag.to && at > drag.from && at <= drag.to) offset = -shift;
    if (drag.from > drag.to && at >= drag.to && at < drag.from) offset = shift;
    cards[slot.id].style.transform = offset ? `translateX(${offset}px)` : "";
  });
}

// Dragging towards a cover that is off the side of the window has to bring
// it into reach; without this the row could only ever be rearranged within
// one screenful of itself.
//
// The direction is re-read each frame rather than captured when the loop
// starts, so crossing from one edge zone to the other turns the scroll
// around instead of leaving it running the wrong way.
function autoScroll() {
  if (edgeTimer !== null) return;

  const tick = () => {
    if (!drag) { edgeTimer = null; return; }

    const rect = gallery.getBoundingClientRect();
    let step = 0;
    if (drag.pointerX < rect.left + EDGE_ZONE) step = -EDGE_SPEED;
    if (drag.pointerX > rect.right - EDGE_ZONE) step = EDGE_SPEED;
    if (step === 0) { edgeTimer = null; return; }

    const before = gallery.scrollLeft;
    gallery.scrollLeft += step;
    // Already at the end: nothing moved, so there is nothing to keep
    // spinning a frame loop for.
    if (gallery.scrollLeft === before) { edgeTimer = null; return; }

    updateDrag();
    edgeTimer = requestAnimationFrame(tick);
  };
  edgeTimer = requestAnimationFrame(tick);
}

document.addEventListener("pointerup", (event) => {
  if (!drag || event.pointerId !== drag.pointerId) return;

  const { id, card, from, to, slots, moved } = drag;
  drag = null;
  if (edgeTimer !== null) { cancelAnimationFrame(edgeTimer); edgeTimer = null; }

  card.classList.remove("dragging");
  // Every card goes back to no transform of its own; the real move is the
  // DOM reorder below, which puts them where the offsets were pretending
  // they already were.
  slots.forEach((slot) => { cards[slot.id].style.transform = ""; });

  if (!moved || from === to) return;

  // The visible row rearranged, then folded back into the full order so
  // that covers hidden by a filter keep their places. (Arrange mode clears
  // the search, so in practice there are none — this is what makes the
  // fold-back correct rather than merely unused.)
  const visible = slots.map((slot) => slot.id);
  visible.splice(from, 1);
  visible.splice(to, 0, id);

  let at = 0;
  stored.user = order.map((other) =>
    slots.some((slot) => slot.id === other) ? visible[at++] : other
  );

  applyOrder();
  send("order:" + stored.user.join(","));
});

// ── Launching ──────────────────────────────────────────────────────

// The transform that takes a card from where it sits in the row to the
// middle of the window. The cover keeps the size it already had: it was
// sized to fit this window, and resizing it on the way to the centre reads
// as a glitch rather than as emphasis. The spinner doesn't compete for the
// space either — it sits over the top of the darkened screen.
//
// Measured from the <img> rather than the card so a caption underneath
// doesn't shift the centring, with the transform origin on the image's
// centre so the outro's scale-up pushes off the same point.
function centreTransform(card, img) {
  const cardRect = card.getBoundingClientRect();
  const rect = img.getBoundingClientRect();
  const dx = window.innerWidth / 2 - (rect.left + rect.width / 2);
  const dy = window.innerHeight / 2 - (rect.top + rect.height / 2);

  return {
    origin: `${rect.left + rect.width / 2 - cardRect.left}px ${rect.top + rect.height / 2 - cardRect.top}px`,
    transform: `translate(${dx}px, ${dy}px)`,
  };
}

// Freezes every card where it currently sits. Without this, the other
// covers leaving the flex row would drag the chosen one sideways mid-flight.
// Viewport coordinates, so a scrolled row pins exactly where it looks.
function pinCards() {
  const rects = cards.map((card) => card.getBoundingClientRect());
  cards.forEach((card, index) => {
    const rect = rects[index];
    card.style.left = rect.left + "px";
    card.style.top = rect.top + "px";
    card.style.width = rect.width + "px";
    card.style.height = rect.height + "px";
    card.classList.add("pinned");
  });
}

function unpinCards() {
  cards.forEach((card) => {
    card.classList.remove("pinned", "dimmed", "chosen");
    card.style.left = "";
    card.style.top = "";
    card.style.width = "";
    card.style.height = "";
    card.style.transform = "";
    card.style.transformOrigin = "";
  });
}

function beginLaunch(index) {
  // While the covers are being arranged a press picks one up; it does not
  // start it. This is also what keeps a cover for a missing game unplayable
  // during arranging, when its button is temporarily not disabled.
  if (arranging) return;
  if (state !== "idle") return;
  const card = cards[index];
  const img = imgs[index];
  if (!card || card.disabled) return;
  state = "launching";
  launchedAt = Date.now();

  // A retry clears the last failure: the message under the cover belongs
  // to the attempt that produced it, not to the game.
  card.classList.remove("failed");
  card.querySelector(".note").textContent = "";

  pinCards();
  // Flush the pinned layout before anything animates, so the browser has a
  // "from" position to interpolate out of rather than jumping.
  void document.body.offsetWidth;

  const target = centreTransform(card, img);

  // The progress line is exactly the width of the cover it belongs to, and
  // sits just under where that cover is about to come to rest. The cover
  // keeps its current size through the flight (resizing it reads as a
  // glitch), so its measured box is also its final box — which is what lets
  // this be worked out now, before anything has moved.
  const imgRect = img.getBoundingClientRect();
  setVar("--track-width", Math.round(imgRect.width) + "px");
  setVar("--track-top",
    Math.round(window.innerHeight / 2 + imgRect.height / 2 + TRACK_GAP) + "px");

  cards.forEach((other, i) => {
    if (i !== index) other.classList.add("dimmed");
  });
  card.classList.add("chosen");
  card.style.transformOrigin = target.origin;
  card.style.transform = target.transform;

  statusLine.textContent = "Starting " + games[index].name + "…";
  document.body.classList.add("launching");

  send("launch:" + index);
}

// Called from Rust once the launch has resolved: `ok` means the game's
// window is up (see launch.rs), and the launcher's job is done.
window.__launchOutcome = function (index, ok, message) {
  if (state !== "launching") return;
  const card = cards[index];
  if (!card) return;

  if (ok) {
    document.body.classList.add("finishing");
    card.style.transform = card.style.transform + " scale(1.06)";
    // Rust closes on its own deadline as well, so a hiccup here can never
    // leave the launcher sitting in front of a running game.
    setTimeout(() => send("close"), OUTRO_MS);
    return;
  }

  // Unwind: the covers come back and the failure is reported in the row,
  // where the player can simply choose it again. Not before the loading
  // state has been up long enough to have been seen, though.
  const held = Math.max(0, MIN_LOADING_MS - (Date.now() - launchedAt));
  setTimeout(() => {
    document.body.classList.remove("launching");
    cards.forEach((other) => other.classList.remove("dimmed"));
    card.style.transform = "";
    setTimeout(() => {
      unpinCards();
      card.classList.add("failed");
      card.querySelector(".note").textContent = message || "Failed to start";
      // The note has just appeared under a cover that is very likely the
      // selected one — it was clicked. Let it have the line.
      yieldToNote();
      placeNameplate();
      state = "idle";
    }, MOVE_MS);
  }, held);
};

// ── Startup ────────────────────────────────────────────────────────

const pending = imgs
  .filter((img) => !img.complete)
  .map((img) => new Promise((res) => { img.onload = img.onerror = res; }));

// `false`: this is the stored mode being applied, not a change to report. The
// launcher writing back the setting it just read would be noise in the file
// and a disk write on every start.
setMode(mode, false);
sizeSegments();
sizeNameplate();

// The first cover in the row, so the shelf opens with something already
// offered rather than uniformly veiled. `order[0]` and not id 0: which cover
// is first depends on the order mode, and "first" means what is on the left.
if (order.length > 0) select(order.find((id) => !cards[id].disabled) ?? -1);

Promise.all(pending).then(layout);
layout(); // first pass in case images are already cached
window.addEventListener("resize", () => {
  if (state === "idle") {
    layout();
    sizeSegments();
    sizeNameplate();
  }
});

// The name line is set in BackOut, which arrives over app:// after this runs.
// Re-measure once it lands: a display face is wider than the fallback and the
// segment widths were measured before it existed.
if (document.fonts && document.fonts.ready) {
  document.fonts.ready.then(() => {
    sizeSegments();
    sizeNameplate();
  });
}
