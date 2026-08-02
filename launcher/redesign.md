# Launcher frontend redesign

## Context

`launcher/src/index.html` is a 1431-line single file holding the launcher's entire
visual identity. The underlying architecture in `structure.md` is sound and its
commitments are worth keeping, but the surface has three concrete problems:

1. **No game names are visible.** `show_captions = false` in the seeded
   `launcher/src/config.toml:33`, so names appear only in OS tooltips. A player
   looking at a shelf of covers cannot read what anything is called.
2. **A native `<select>` is doing order-mode duty** (`index.html:658-663`) inside
   an otherwise bespoke, undecorated, transparent console window. The OS popup it
   opens is the single loudest "this is a web page" tell in the product.
3. **Five hard-coded colors fight the configurable background.** `#6ea8ff` focus
   ring, `#b0202f` close hover, `#151515` plate, `#cfcfcf` caption, and the
   `#241830` select popup are absolutes sitting on a `background_color` the
   cartridge owner is explicitly allowed to change. Any background other than a
   dark violet clashes.

The outcome: a launcher that reads as a piece of hardware rather than a web page,
where cover art is the only color on screen, names are legible, and every chrome
color derives from whatever background the owner chose.

**Design read:** cartridge-media game launcher for the person who just plugged in
the drive, physical-object / console-shelf language, native CSS in a wry webview
with no framework and no GPU.
**Dials:** `VARIANCE 5` (the single row is the idea; asymmetry would fight it),
`MOTION 6` (launch is the emotional moment), `DENSITY 2` (art gallery).

**On the line numbers below:** they were read on 2026-07-31 and other work on the
launcher may have moved them since. File paths, CSS selectors, config key names,
and Rust constant names are the stable handles and are what to search on. Treat
line numbers as hints, not addresses.

---

## What deliberately does not change

These are load-bearing and staying exactly as they are. Stated up front because
the redesign is easy to over-reach.

- **`layout()` and the Rust sizing contract.** Every new element is either inside
  the existing `2 * border_gap` vertical slack or absolutely positioned. Cover
  sizing math, `constants::TOOLBAR_BAND = 44.0`, `--toolbar-band`, and the
  `TOOLBAR_BAND` JS constant are untouched. `window.rs` needs no edit.
- **Covers never shrink for a long catalog** (`structure.md:122-144`).
- **One row, horizontal scroll, engine scrollbar suppressed.**
- **The four order modes, drag-arrange, edge auto-scroll, search-on-overflow.**
- **The IPC surface.** All four messages (`close`, `launch:`, `mode:`, `order:`)
  and `window.__launchOutcome` keep their exact shapes.
- **`catalog::Game`.** Still `name`, `exe`, `image` plus computed `available`. No
  new fields, so no installer changes and no new `is_contained()` surface.

---

## 1. Palette: graphite body, cover art is the only color

The reseeded default. Cover art is arbitrary and colorful, so the chrome goes
monochrome and gets out of its way. Amber stays reserved for *error*, red for
*missing*, so the accent must not be either.

Edit `launcher/src/config.toml` only (see §5c for why `constants.rs` defaults are
a separate, riskier question):

| key | from | to | why |
|---|---|---|---|
| `background_color` | `#3D1F37` | `#141416` | Graphite. Near-black without being `#000000`, reads as a device chassis. |
| `shadow_color` | `#925E37` | `rgba(0, 0, 0, 0.6)` | The tan was an opaque glow. On graphite a soft dark bloom deepens the field around each cover instead. |
| `shadow_size` | `20` | `28` | |
| `accent_color` | *(new)* | `#F2F2F0` | Near-white. Focus rings, active segment, launch hairline. Owner can tint it. |
| `show_captions` | `false` | `true` | Now costs one shared line, not 30 labels (§3). |

### Derived tokens, computed in JS

The previously hard-coded colors become a ramp derived from `background_color` at
startup, in the existing `setVar` pass (`index.html:728-761`):

```
--plate      background mixed 6%  toward ink   (was #151515)
--text       background mixed 88% toward ink   (was #cfcfcf)
--line       background mixed 18% toward ink   (control borders)
--veil       background at variable opacity    (replaces filter: brightness)
--accent     from config accent_color          (was #6ea8ff)
```

Two implementation notes that matter:

- **Compute the mixes in JS, emit plain hex.** Do not use CSS `color-mix()`.
  WebView2 can be pinned to a fixed-version runtime on a deployed cartridge, and
  `color-mix()` needs Chromium 111+. JS is already setting these variables, so
  producing concrete values there costs ~15 lines and removes the dependency.
- **"Ink" is direction-aware.** Compute the background's relative luminance and
  pick white or near-black as the mix target, so a cartridge owner who sets a
  *light* background still gets a readable launcher. This is the whole point of
  the derivation and is what makes `structure.md:40-45` ("whoever owns the
  cartridge owns its config") actually hold.

`toolbar_color` and `scrollbar_color` stay as live config keys driving their
existing variables. They are not folded into the ramp: existing cartridges set
them, and silently overriding those would break the same ownership rule.

---

## 2. Toolbar: real controls instead of OS chrome

Replace the `<select>` at `index.html:658-663` with a segmented control.

```
+----------------------------------------------------------+
| [ Recent | A-Z | Cartridge | Mine ]   ..     Search    X  |
+----------------------------------------------------------+
```

- **Four equal-width segments** in a track, longest label (`Cartridge`) setting
  the shared width. Equal widths mean the active pill is one fixed size and only
  `translateX` changes, so the indicator animates on the compositor. Never
  animate its `width`.
- **Markup:** `div[role="radiogroup"]` containing `button[role="radio"]` with
  `aria-checked`. Sends the same `mode:<name>` IPC message.
- **Keyboard:** the global ArrowLeft/Right handler (`index.html` keydown listener)
  currently skips only when `#search` has focus. It must also skip when focus is
  inside the radiogroup, or arrow keys will move covers and segments at once.
- **Close button moves into the toolbar row** as the last flex item. This deletes
  `position: fixed` on `#close-btn` and the `margin-right: 34px` hack on `#search`
  (`index.html:221`) that existed only to clear it. Its `#b0202f` hover becomes a
  derived `--line` surface fill.
- **Keep close visible during launch.** Today `body.launching #toolbar` fades the
  whole bar while the fixed close button survives, which is the escape hatch for a
  hung launch. Change the rule to fade `#toolbar-left` and `#search` only.
- **Keep close visible on an empty cartridge.** The toolbar is currently hidden
  outright for a zero-game catalog. Hide its *contents* instead so the row and its
  close button remain.
- **Shape lock.** Chrome radii are five different values today (4, 5, 8, 12, 14,
  16). Introduce one `--control-radius: 8px` for the segmented track, pill,
  arrange toggle, search, and close. Covers keep `--corner-radius`, the window
  keeps `--window-corner-radius`; both stay config-driven.

---

## 3. The shelf: focus-weighted, with one shared name line

The core visual change, and the reason names become readable.

- **JS maintains a `selected` index**, starting at 0. Pointer hover sets it,
  ArrowLeft/Right moves it. The selected card gets `.selected`.
- **Non-selected covers recede** behind a `::after` veil on `.cover`: a solid
  `var(--background-color)` overlay whose `opacity` transitions between 0 and
  ~0.45. This replaces `filter: brightness()` entirely. Under `--disable-gpu`
  (`ui.rs:130`), a filter on a full-size image is software-rendered on every
  frame of the transition; an opacity fade on a solid overlay is not. It also
  tints toward the background rather than toward black, so it works on any
  configured background.
- **The same mechanism serves `.unavailable`**, at a higher veil opacity driven by
  the existing `missing_dim` key. The `.sign` circle-slash stays as a real
  semantic state, shrunk from 38% to ~26% of cover width.
- **One shared name line**, absolutely positioned in the gallery's bottom padding,
  showing the selected cover's name. Not per-card. This is what keeps `layout()`
  untouched: card height stays exactly image height, the row stays vertically
  centered, and there is no 30-label visual noise. `show_captions` keeps its key
  and now governs this line.
- **Selected cover lifts**: larger shadow plus the existing `translateY(-6px)`.
  Motivated by hierarchy, which is the one thing that needs communicating here.
- **Focus ring** becomes `2px solid var(--accent)` with radius tracking
  `--corner-radius`, replacing the off-palette `#6ea8ff`.

---

## 4. Launch, states, and motion

**Hairline instead of the ring.** Delete `#ring` and its `spin` keyframe. Under
the centered pinned cover, draw a 2px track at exactly the cover's width (JS
already measures the img in `centreTransform`, `index.html:1287`) containing a
35%-wide segment animating `translateX` inside an `overflow: hidden` parent. Pure
transform, no layout, no filter. `#status` positions from the track rather than
from `--ring-size`; `loading_text_gap` stays meaningful.

**Empty state.** Replace the bare centered `#empty` text with three ghost plates
at the real cover size (reusing `layout()`'s computed dimensions) plus the message
beneath. An empty shelf should look like an empty shelf.

**Reduced motion.** There is no `prefers-reduced-motion` handling anywhere today,
and `MOTION 6` makes it mandatory. Add a block that disables the hover lift, the
launch flight (the chosen cover appears centered instead of travelling), the
hairline sweep (static filled track; `#status` carries the message), and the scrim
fade. The launch *sequence* still runs, it just does not animate.

**Typography.** Ship Geist (400, 500) as the UI face and Geist Mono (400) for
`#status` and the error `.note` only, where machine-status voice fits the hardware
metaphor. Latin subsets, ~50-60KB total. Geist is OFL-1.1, which is fine to bundle
with GPL-3.0 as embedded data, but the license text must ship: add
`launcher/licenses/OFL-Geist.txt` and reference it from `README.md`.

---

## 5. Rust edits

### 5a. Serve woff2 (`launcher/src/assets.rs`)

Three edits, all in one file:

- `:34` add `#[include = "*.woff2"]` to the `UiAssets` derive.
- `:41` `UI_ASSET_EXTENSIONS: [&str; 3]` becomes `[&str; 4]` with `"woff2"`.
  The length literal is explicit, so forgetting it is a compile error.
- `:137` add `Some("woff2") => "font/woff2",` to `mime_type_for`.

**Gotcha worth calling out:** `is_ui_asset` gates at `:67`, *before* the dev
live-file path at `:75`. But forgetting the rust-embed include is invisible under
`cargo run`, because the dev path reads `src/*.woff2` straight off disk and works
perfectly. The font 404 only reproduces on the deployed `output/launcher.exe`.
Test there.

### 5b. Split the file

`index.html` becomes `index.html` + `style.css` + `app.js`. **Zero Rust changes:**
`*.css` and `*.js` are already in the rust-embed include (`:33-34`), the extension
allowlist (`:41`), and the MIME map (`:135-136`).

`with_initialization_script` (`ui.rs:127`) maps to WebView2's
`AddScriptToExecuteOnDocumentCreated`, which runs before the parser produces any
DOM, so `window.__UI__` and friends are guaranteed defined before `app.js` runs
regardless of placement. `defer` is needed for DOM readiness only. Put
`<script src="app.js"></script>` last in `<body>`, matching today's placement.

Add a comment at the top of both new files pointing at `constants.rs`: the
`PAD` / `GAP` / `TOOLBAR_BAND` duplication contract now spans two more files.

### 5c. Config keys

**Add `accent_color`**, following `scrollbar_color` everywhere it appears:
`constants.rs:119` (new `DEFAULT_ACCENT_COLOR`), `config.rs:61` (field), `:96`
(Default), `:153` (`set_color` in `load`), `:404` (`known_settings` tuple),
`ui.rs:316` (`"accentColor"` in the `__UI__` object, before the `toolbarBand` /
`minLoadingAfterFail` pair that the comment there marks as non-config).

**Deprecate `loading_ring_segments` and `loading_ring_speed`.** Unknown keys are
ignored silently: `load()` asks for keys one at a time rather than enumerating the
table (`config.rs:111-113`), and `sync_defaults` only ever *adds* keys the file
lacks (`config.rs:445-448`). So dropping the two `known_settings()` entries is the
entire deprecation. No tombstone concept needed. Remove: `config.rs:57-58`,
`:92-93`, `:146-150`, `:161-164` (the `MIN_LOADING_RING_SPEED` floor, which is
`load()`'s only post-processing step), `:355-364`; `ui.rs:312-313`;
`constants.rs:106-111`; and **`src/config.toml:92-98`**, which is the edit most
likely to be forgotten and would otherwise ship dead keys documented as live.

**Keep the name `loading_ring_color`**, repurposed to color the hairline. Renaming
it would silently revert every cartridge that set it, which is exactly what
`structure.md:44-45` forbids. Reword the description at `config.rs:346-349`,
`constants.rs:104`, and the seed comment at `config.toml:86-90`.

**Reseed `src/config.toml` only, not `constants.rs`.** These are two different
defaults and only one is dangerous. The seed is what a *fresh* cartridge's
config.toml is written with (`content::seed_if_missing`, guarded by
`if !path.exists()`), so changing it affects only cartridges that do not exist
yet. `constants.rs` is the fallback for a config that *omits* the key, so changing
it restyles existing cartridges on update. It also permanently invalidates any
`# background_color = "#1b1229"` comment `sync_defaults` already wrote, since
those are comments and `contains_key` stays false forever. Leave `constants.rs:89`
and `:92` alone.

### 5d. Docs that go stale

`structure.md:71` and `:279` (source layout, now three web files), `:209-210` (the
segmented-ring paragraph), `:244-252` (knob inventory); `README.md:48`; and the
`src/index.html` references in `constants.rs:35`, `:72`, `:77`, `main.rs:45`,
`:61`, `window.rs:10`, `order.rs:29`, `ui.rs:235`.

---

## Verification

1. `cargo run` from `launcher/`. The dev path reads `src/` live, so iterate on
   `style.css` / `app.js` without rebuilding.
2. **Populate the shelf.** `launcher/output/games/` is empty in the repo, so all
   three demo games currently render as `.unavailable`. Drop stub `.exe` files at
   the paths in `output/catalog.json` to see the normal state, and remove one to
   check the missing state.
3. **Walk every state:** selected/unselected veil, hover, keyboard arrow
   navigation, focus ring, search filter, all four order modes, arrange-mode drag
   with edge auto-scroll, launch success (hairline then outro), launch failure
   (hairline holds the 1000ms `MIN_LOADING_AFTER_FAIL` floor, then `.failed`
   border and note), cover-art load error (`no-cover` plate), and the empty state
   via a `[]` catalog.
4. **Palette derivation:** set `background_color` to a light value (`#EDEDE9`) in
   `output/config.toml` and confirm text, plate, and borders invert correctly.
   Then a mid-tone (`#4A5A52`). This is the check that the ink-direction logic
   actually works.
5. **Reduced motion:** Windows Settings > Accessibility > Visual effects >
   Animation effects off. Confirm the launch still completes and no transition
   runs.
6. **Deployed build:** build release and run `output/launcher.exe` from a shell
   whose cwd is not the repo. This is the only way the woff2 embed bug shows up.
7. `cargo test` (`launcher/src/tests.rs`) for the config round-trip and
   `order::normalize`.
8. **Config ownership:** point the launcher at a config.toml containing
   `loading_ring_segments = 12` and an explicit `background_color`, and confirm
   the stale key is ignored without complaint and the explicit background wins.

## Open items

- **Geist Mono earns its place on two small surfaces only** (`#status`, error
  `.note`). If that reads as not worth a third font file, drop it and use Geist
  throughout; the design does not depend on it.
- Cover art in `output/images/hollow_knight.png` is 18.7MB, which suggests it is
  not the 600x900 the layout assumes. Worth checking, but out of scope here.
