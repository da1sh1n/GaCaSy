# GaCaSy Launcher — Cartridge Side (developer reference)

Part of the three-app **GaCaSy** game-cartridge system. This document covers the
**launcher**: the app that lives on the cartridge, shows its games, and launches them.
The PC-side companion is documented in [`../listener/structure.md`](../listener/structure.md)
and the setup tool that puts both in place in
[`../installer/structure.md`](../installer/structure.md); the user-facing overview and
install/use steps are in [`../README.md`](../README.md).

This side already exists — treat this doc as a *reference to remember*.

## Role

The launcher is the app the player sees. It lives on the cartridge, ships as a single
`launcher.exe` beside its content, and is a self-contained webview shell — no bundled
web server, nothing listening on a port. Written in Rust using `wry` (webview) + `tao`
(windowing); the whole UI (HTML/CSS/JS) is baked into the exe with `rust-embed` and
served over a custom `app://` protocol straight from memory.

## Deployed layout

Everything the launcher reads at runtime lives in an `output/` folder beside the exe:

```text
output/
  launcher.exe     <- the program
  config.toml      <- look and feel; seeded from a baked-in default if missing
  catalog.json     <- the game list (name / exe / image); seeded likewise
  assets/
    images/        <- cover art (600x900, 2:3), dropped in by hand
    EBWebView/     <- WebView2's own data folder (its only on-disk crumbs)
  games/           <- the actual game installs
  logs/            <- launcher.log (every launch attempt) + <game>/out.log, err.log
```

Nothing else. There is no identity file beside these: what makes the volume a cartridge is the
signature carried **inside `launcher.exe` itself** (see [Role in cartridge
identification](#role-in-cartridge-identification)).

**`assets/` is not safe to delete, even though half of it is.** `EBWebView/` is
regenerable browser cache and throwing it away costs nothing; `images/` beside it is
irreplaceable cover art. They share a parent so the cartridge root holds only what a person
put there, and the price of that is that the old advice ("delete EBWebView to clear the
cache") now has to name the child rather than the folder.

A cartridge written before this layout keeps `images/` at the root, and nothing migrates it:
the path lives in that cartridge's own `catalog.json`, and `assets::handle_request` serves
both prefixes. New cartridges get `assets/images/` because that is what the installer's
`catalog::IMAGES_DIR` now writes.

`catalog.json`, `assets/images/` and `games/` are **never overwritten** once present, so
hand-dropped content survives every build.

`config.toml` is the exception, because in the repo it has a master and on a cartridge it
doesn't. Under `cargo run`, `launcher/src/config.toml` **is** the config and every run
copies it over `output/config.toml` — edit it in `src/`, since edits to the copy are
overwritten on the next run. On a deployed cartridge it is written once if missing and
never touched again: whoever owns the cartridge owns its config, and an update must not
restyle their launcher out from under them.

Under `cargo run` the crate root (`launcher/`) *is* the base dir, so the deployed
`output/` above corresponds to `launcher/output/` in the repo.

## Source layout

Everything the crate owns lives in `src/` — the Rust shell, the UI, and the two seed
data files baked into the exe:

```text
launcher/
  Cargo.toml
  src/
    main.rs        <- the front door: resolve the base dir, take the lock, hand off to ui
    constants.rs   <- every tunable number in the crate, in one place
    content.rs     <- which folder holds the content, and seeding it on first run
    config.rs      <- reading config.toml, key by key, with defaults under it
    catalog.rs     <- the game list, and marking which games are actually present
    assets.rs      <- the app:// protocol: the embedded UI and disk content
    window.rs      <- how big the window is and where it sits
    ui.rs          <- the window + webview, the IPC, and the event loop
    launch.rs      <- starting a game and deciding whether it came up
    log.rs         <- logs/launcher.log and each game's own output
    order.rs       <- what order the covers are shown in, and repairing a bad one
    instance.rs    <- the single-instance mutex
    index.html     <- the UI's markup, embedded by rust-embed and served over app://
    style.css      <- its look, same
    app.js         <- its behaviour, same
    config.toml    <- seed config, embedded via include_str! and written to output/
    catalog.json   <- seed game list, same
  assets/
    fonts/
      DepartureMono.woff2 <- the typeface, embedded too, served at app://fonts/…
  licenses/
    OFL-DepartureMono.txt <- required by the SIL Open Font License the face ships under
  structure.md
  output/          <- the deployed cartridge content (see above)
```

The font is **compiled into the exe, not shipped on the cartridge**. A font in `output/` is
a file that can be deleted, missed by a hand-copy, or left behind when somebody moves a
cartridge's contents, and the launcher would then quietly fall back to a system face.
Compiled in, it is exactly as present as the code that asks for it — and it is the strongest
form of "works with no network": there is nothing to fetch even in principle. That is why
`assets/fonts/` exists in the repo but not under `output/`.

### The 11px grid

The face is **Departure Mono** (Helena Zhang, OFL-1.1), a pixel monospace, and its own
documentation states the constraint: *set the font size to increments of 11px*. That is the
difference between the type looking drawn and looking smeared, so it is a commitment the
whole stylesheet is written against rather than a preference:

- **No rule in `style.css` states a font-size of its own.** Every one is `var(--type-unit)`,
  and the single exception steps by whole units (`#close-btn`, which sets one ✕ inside a
  30px button).
- **`--type-unit` is computed in `app.js`, not fixed in the CSS**, because the grid is in
  *device* pixels. An 11px CSS rule is 13.75 device pixels at 125% scaling, and that quarter
  pixel is enough to undo it — so the value is rounded to whole device pixels for whatever
  display the window actually opened on. The literal in `:root` is what that comes to at
  100%, kept so a page running without the script is still on the grid.
- **The name line is the only thing that varies**, and it steps rather than scales:
  `sizeNameplate()` gives it one unit or two from the gap actually available. It used to
  interpolate across 11–22px, which under a pixel face is a continuum of blurry with two
  sharp points in it.
- **Tracking is `0` everywhere.** `em` letter-spacing resolves to fractions of a pixel and
  walks glyph origins off the grid the size was chosen to land on.

The cost, which is worth knowing before someone tries to fix it: the next step up is 22px, so
**the chrome is all one size**. Hierarchy in the toolbar comes from the colour ramp and the
pill fill, and there is no size in between to reach for.

The face covers Latin, Cyrillic and Greek (1,186 glyphs), so only CJK and beyond fall through
to the fallback — and the fallbacks are monospace on purpose, since dropping to a proportional
face mid-name reflows the line around the one glyph that was missing.

One file per job, and `main.rs` is only the front door. Two conventions hold the split
together: **every tunable number lives in `constants.rs`** (the module that owns each one
is named in its section header), and the only file that knows both halves of the app is
`ui.rs`, where config and catalog go out to the page and clicks come back.

Because `src/` holds both source and web assets, `UiAssets`' rust-embed include list
(`*.html`, `*.css`, `*.js`) is what keeps the Rust sources and the two seed files out of the
bundle; `is_ui_asset()` applies the same extension list at runtime so the dev path can't
serve them either. That pair of lists is the one deliberate exception to the constants rule
— `UI_ASSET_EXTENSIONS` stays beside the embed list it mirrors, because apart they drift.
The font has a second embed struct of its own (`FontAssets`, over `assets/`), because
rust-embed takes one folder each and the typeface does not belong beside the Rust sources.

**They fail differently, and only one of them fails where you'll see it.** The runtime
list gates both paths, so a missing extension 404s in dev immediately. The rust-embed list
gates only the deployed binary — under `cargo run` the dev path reads the file off disk and
everything works, and the 404 turns up for the first time on a cartridge. Anything added to
one list has to be tested on a built `output/launcher.exe`, not just in dev.

## How it runs

- **No server, no port.** UI assets are embedded via `rust-embed` and served over the
  `app://` custom protocol. On Windows this resolves to an `http://app.localhost/...`
  origin, which keeps wry's IPC (close / launch clicks) working — a raw `file://`
  origin would crash IPC.
- **Content vs. UI.** `assets::handle_request` serves `assets/…`, `images/…` (the older
  spelling, kept so existing cartridges keep working) and `games/…` from disk beside the
  exe, and everything else as a UI asset (404 unless it passes `is_ui_asset`). The one
  exception is `fonts/…`, answered from the embedded `FontAssets` before the disk is
  consulted at all. In dev it prefers the **live** file from the source `src/` folder
  (`CARGO_MANIFEST_DIR/src/…`) so HTML edits show up on the next launch with no
  rebuild, falling back to the embedded copy on a deployed cartridge. Responses send
  `Cache-Control: no-store` so WebView2 never serves stale HTML/art.
- **Single instance.** A Windows **named mutex** (`Local\GaCaSy.CartridgeLauncher`),
  not a socket. Enforced **only for the deployed exe** (its parent folder is named
  `output/`); skipped under `cargo run` (exe in `target/`) so a rebuild always opens a
  fresh window instead of silently no-opping on the held lock.
- **Window sizing is deterministic.** `window.rs` computes the window size and the CSS
  obeys the same numbers — no measure-and-report round-trip. **Each cover wants to be its
  native 600x900**, and the window is built around whatever size the screen's caps leave it.
  **The game count sets the window's width, never a cover's size.** The row holds every
  game up to what `MAX_WIDTH_FRACTION` allows and never fewer than `MIN_VISIBLE_COVERS`
  (3), so 0, 1 and 2 games all open the same window; past that the page scrolls the row
  sideways. A cover shrinks only for the screen — `MAX_HEIGHT_FRACTION` when the display
  is too short for one, or the three-cover floor when it is too narrow — via a single
  `cover_scale = min(1, height_room/900, floor_room/1800)` applied on both axes.

  There used to be an `IMAGE_WIDTH_FRACTION` asking for a fraction of screen width, capped
  at native. It is gone: the fraction was almost always what bound rather than the cap, so
  at 0.16 a 2560px display asked for 410px of a 600px cover and the art was never once shown
  at the size it was drawn at, on any display. The rule this section states was not true
  until the target became the native size.

  **The vertical budget is `TOOLBAR_BAND + cover + one border_gap`** — one margin, at the
  bottom. `TOOLBAR_BAND` is the toolbar's height *plus the gap under it*, so a margin on top
  of it put the same space in twice and left a hole between the controls and the covers.
  The band is therefore the only knob for that gap. Margins, gap and the band are in
  **logical (CSS) px** so they match the page's `PAD` / `GAP` / `TOOLBAR_BAND`; the page's
  `layout()` reproduces the same fit from the window height alone (scaling down only).
- **Rounded corners come from Windows, not the page.** `window::round_corners` asks
  Windows 11 for them (`DwmSetWindowAttribute` / `DWMWA_WINDOW_CORNER_PREFERENCE`, which is
  anti-aliased and keeps the shadow) and falls back to clipping the window to a rounded
  region on Windows 10, where that attribute does not exist. The region is a one-bit mask,
  so the Windows 10 corner is slightly stair-stepped; `window_corner_radius` sets it, and
  Windows 11 ignores the number in favour of the system radius.
  **Do not "simplify" this into a CSS `border-radius`** — it was tried and it cannot work.
  That needs a transparent window, and wry hosts WebView2 in *windowed* mode, where the
  browser is a child HWND with no per-pixel alpha: the page composites onto opaque white
  whatever the window and controller are set to (measured — a 99%-opacity background comes
  back as an exact 0.99 blend with white). Transparency would need composition hosting,
  which is a different way of embedding the browser entirely.
- **GUI app**, `#![windows_subsystem = "windows"]` — no console window.

## The gallery

One row of covers in a horizontal scroll viewport, with a toolbar across the top.

- **Covers are never shrunk to fit a long catalog.** A cartridge with thirty games shows
  the same cover as one with three; the ones that don't fit are scrolled to (wheel,
  arrow keys, or the hand-drawn bar under the row — the engine's own scrollbar is
  suppressed so it can't eat into the height the covers were measured for).
- **Order** comes from `order_mode` in `config.toml`, chosen from the **segmented control**
  at the left of the toolbar: `usage` (last opened first), `alphabetic`, `catalog`, or
  `user`. The page computes all four itself, since changing the mode re-sorts the row live.
  It was a native `<select>` and is deliberately not one any more: the popup is drawn by
  Windows, arriving with its own font, metrics and corner radius in the middle of a window
  drawn from scratch. Its four segments are **equal width**, which is what lets the active
  marker be one fixed box that only ever `translateX`es — never resize it, or the slide
  relayouts on every frame with no GPU to absorb it.
- **Search** appears at the right only once the row overflows — measured by adding up the
  cover widths rather than from `scrollWidth`, so narrowing the results can't take away
  the box you need to clear them. It is also the only control allowed to shrink: the window
  is sized for three covers, never for the toolbar, and at that floor on a small display
  the row is a few tens of pixels short of what the segments, the toggle, a full-width
  search box and the close button want.
- **The close button is in the toolbar row**, as its last item, rather than fixed to the
  window corner. Two consequences worth knowing: the launch transition fades the toolbar's
  *contents* rather than the bar, because closing has to stay possible while a launch hangs;
  and an empty cartridge hides those contents rather than the row, for the same reason.
- **One name line, not thirty captions.** `show_captions` now governs a single line in the
  gap under the row, naming whichever cover is selected and following it along the row. Per
  card it was noise, and it is what keeps every card exactly as tall as its image so the
  sizing contract with `window.rs` needs no adjustment. The one piece of text in the UI
  that is not exactly one `--type-unit` — see [The 11px grid](#the-11px-grid).
- **Selection is one index**, fed by both hover and focus, and it decides three things at
  once: which cover is lifted, which is clear of the veil, and which one the name line
  names. Deliberately not a separate hover state and focus state — those can disagree, and
  when they do the row lifts one cover while the name describes another.
- **Arranging** is a toggle beside the order control, shown only in `user` mode. It puts a
  grip on each cover (drawn *on* the cover, not above it — the window height is already
  committed), makes a press mean "pick this up" rather than "start this", and writes the
  new order out on release.
- **Ids are catalog positions**, and reordering only ever moves DOM nodes: `cards[]` and
  `imgs[]` stay indexed by id, so `launch:<id>` and `__launchOutcome(id, …)` are untouched
  by any of it. `order::normalize` repairs a stored list against the real game count on
  both sides — an id that no longer exists is dropped, a game the list never mentioned
  goes at the end — so neither list has to be complete or tidy.

## Settings the launcher writes

`config.toml` is otherwise read-only to this program, and three keys are the exception:
`order_mode`, `usage_order` and `user_order`. `config::store` edits the one key with
`toml_edit`, leaving every comment and blank line in place — the file is mostly prose
written for a person, and reformatting it as a side effect of somebody starting a game
would be answering a question nobody asked. A failure (a write-protected cartridge, an
unparseable file) is logged and otherwise ignored.

`usage_order` is rewritten when a game is **confirmed up**, not when its cover is clicked:
a launch that failed says nothing about what the player wants to play next. The write
happens while the page runs its outro, so nothing waits on it.

Under `cargo run` the config is mirrored from `src/config.toml` every run — but
`content::mirror_seed_config` carries these three keys across, so the seed owns look and
feel while the launcher owns the order in dev exactly as it does on a cartridge.

## Launching a game

Clicking a cover posts `launch:<id>` over IPC. That is one of four messages the page can
send, and the whole of what it is allowed to ask for — there is deliberately no general
"set this setting" message:

| message | meaning |
| --- | --- |
| `close` | the close button, and the page's own outro once a game is up |
| `launch:<id>` | a cover was chosen |
| `mode:<name>` | the order control changed |
| `order:<a,b,c>` | covers were dragged into a new order |

The last two are the only route by which the page reaches the disk, and both are checked
in `ui.rs` rather than trusted: an unknown mode is refused and an id list is normalized
before it is stored, so a bug in the page can't leave behind a config a later run has to
make sense of. What
follows is in `launch.rs`, and the guiding rule is that **"started" means the game's
window is up** — the launcher closes itself on that signal, and closing while the game is
still an invisible process makes a working launch look like a broken one.

1. **Spawn.** `output/<game.exe>`, with the cwd set to **the exe's own folder** — games
   resolve their assets relative to themselves, and the wrong cwd fails in ways that look
   like corruption. stdout/stderr are redirected into `logs/<game>/`. Unless
   `show_console_window` is set, spawned with `CREATE_NO_WINDOW` so a console-mode exe
   (typically a windowless stand-in cover, since a real game is GUI) doesn't pop a console
   window up — a GUI game has no console to begin with, so this never affects one.
2. **Wait, off the UI thread.** A worker thread calls `WaitForInputIdle` (30 s cap), so
   the page keeps animating. A timeout still counts as started — a slow game is not a
   failed one. `WAIT_FAILED` means a console program, not a failure: that (and any
   non-Windows build) falls back to "still alive after 2 s". Either way the process then
   has to *stay* alive for a moment (400 ms) to be believed: `WaitForInputIdle` reports
   success for a process that has already died, and its exit status isn't necessarily
   posted yet when it returns, so without that pause a game that crashes on startup
   reads as "up". Dying inside the window is a failure, with its exit code in the log.
3. **Report.** The outcome travels back to the event loop as `UserEvent::LaunchOutcome`
   and into the page as `window.__launchOutcome(index, ok, message)`. On success the page
   plays its outro and asks to close; Rust also arms its own ~1.2 s deadline, so a broken
   page can't leave the launcher sitting in front of a running game.

What the player sees, all of it driven by the page:

- **Chosen.** The other covers fade out, the chosen one animates to the centre of the
  window **at the size it already had** (resizing it reads as a glitch), the whole screen
  dims behind `overlay_color`, a **progress line** sweeps directly under the cover, and a
  faint "Starting …" sits below that. The line is exactly the cover's own width, and a
  segment about a third that wide runs across it and wraps. This replaced a segmented ring
  spinning in the middle of the screen, for two reasons: it belongs to the cover, where a
  ring floating in open space only said "something is happening"; and it is a `translateX`
  on a plain div rather than a `rotate` on a dash-stroked SVG, which matters because the
  webview runs with `--disable-gpu` and every frame of the old one was rasterised in
  software. `loading_ring_color` keeps its name and now colours this.
- **Failed.** The transition unwinds, the covers come back, and that one keeps a border
  in `error_border_color` with a short message under it. It stays clickable — choosing it
  again retries, which clears the mark. The unwind waits until the loading state has been
  up for `MIN_LOADING_AFTER_FAIL` (`constants.rs`, 1 s, measured from the click and sent
  to the page as `__UI__.minLoadingAfterFail`): a missing file fails in milliseconds, and
  an indicator that flashes and vanishes reads as a glitch rather than as an attempt that
  was made and didn't work. Not a `config.toml` knob — change it in `constants.rs`.
- **Missing.** A game whose exe isn't on the cartridge is settled before the player
  touches anything: Rust checks each `exe` at startup and passes `available` to the page,
  which veils that cover by `missing_dim`, draws a sign over it in `missing_sign_color`
  and disables the button. Checked once, so a cartridge that changes under a running
  launcher needs a restart.
- **Empty.** A cartridge with no games shows three outlined plates at the size a real cover
  would have been, with the message under them. An empty shelf should look like an empty
  shelf; a line of text in a void reads as a page that failed to load.
- **Reduced motion.** Every one of the above still happens under Windows' "Animation
  effects off", and none of it moves: the chosen cover appears centred rather than flying
  there, the progress line fills and holds still, the lift and the pill's slide are gone.
  The launch *sequence* is untouched, which matters because the page times itself off these
  transitions — a step skipped here would be a launcher that never closed its window.

## Logs

The launcher has no console, so `logs/` is the only place a failure can be explained:

- `logs/launcher.log` — every attempt, pid, and the **full** OS error text (the UI only
  ever shows one short sentence). Appended, rewritten from scratch past 1 MB.
- `logs/<game>/out.log`, `logs/<game>/err.log` — that game's own console output,
  truncated per launch so they always describe the current run. `<game>` is the catalog
  name reduced to `[a-z0-9-]`.

## Data files

- **`catalog.json`** — an array of `{ name, exe, image }`. `exe` and `image` are paths
  relative to `output/` (e.g. `games/bg3/bg3.exe`, `assets/images/bg3.png`). Injected into the
  page as `window.__GAMES__` (fetching it would hit CORS) — rebuilt rather than passed
  through verbatim, so each entry can carry `available` (see *Launching a game*).
- **`config.toml`** — real TOML, parsed with the `toml` crate. `config::load()` reads it
  as a `toml::Table` and pulls one key at a time rather than deserializing into a
  struct, so unknown keys and wrong-typed values cost only that setting (it falls back
  to its default) and an older config still works; only a file that isn't valid TOML at
  all drops every setting to defaults. Knobs: `show_captions`, `show_console_window`
  (bool), `border_gap`, `image_gap`, `corner_radius`, `window_corner_radius`,
  `shadow_size`, `shadow_fade`, `error_border_width`, `missing_dim`, `loading_text_gap`
  (non-negative numbers), and `primary_color`, `secondary_color`, `accent_color`,
  `overlay_color`, `loading_ring_color`, `loading_text_color`, `error_border_color`,
  `error_text_color`, `missing_sign_color`, `toolbar_color`, `scrollbar_color` (quoted CSS
  color strings). Every one of them but `show_console_window` is handed to the page as a
  CSS variable; that one only ever matters to Rust, at the moment a game is spawned (see
  *Launching a game*).

  **The palette is three colours, 60 / 30 / 10.** `primary_color` (the window),
  `secondary_color` (shadows, borders, the plate behind missing art) and `accent_color`
  (text, the selected cover, the close button) carry everything that is not cover art or
  one of the two semantic states.

  Two of the shades the page actually draws are **not** the raw colour, and both for a
  measured reason rather than a stylistic one. An accent chosen to look right as a *fill*
  is routinely too close to the primary to read as small text on it — the shipped caramel
  measures 3.34:1 against the shipped violet, fine for a control (3:1) and under AA for body
  copy (4.5:1). A secondary chosen to look right as a *shadow* is routinely too faint to see
  as a hairline — the shipped plum is 1.25:1 on its own violet. So `app.js` lifts each one,
  in small steps, until it clears its threshold and no further: the palette the owner set,
  at the strength the role needs. Set three colours, get a coherent launcher.

  Which direction "lighter" runs is decided from the primary's own luminance, so a pale
  palette works with no further edits. `toolbar_color`, `scrollbar_color`,
  `loading_ring_color` and `loading_text_color` default to blank meaning "take it from the
  palette"; naming any of them still wins. The mixing happens in `app.js` rather than via
  CSS `color-mix()`, which needs Chromium 111+ and a deployed cartridge can be pinned to a
  fixed-version WebView2 runtime.

  **`background_color` and `shadow_color` were what primary and secondary replaced.** A
  cartridge that still names them has them read straight into the two they became, so it
  keeps the look it had.

  **`loading_ring_segments` and `loading_ring_speed` are gone**, with the ring they
  described. No tombstone was needed: `load()` asks for keys one at a time rather than
  enumerating the table, and `sync_defaults` only ever *adds* keys a file lacks, so a
  cartridge that still sets them is simply not asked and nothing complains.

  A deployed `config.toml` is written once and never rewritten, so a knob added after a
  cartridge's config existed would otherwise apply its default with nothing in the file
  to reveal that. `config::sync_defaults()` runs on every startup and appends a
  commented-out `# key = default` line (with a short description) for any known setting
  the file doesn't mention — inert until uncommented, so it changes nothing about how the
  launcher already behaved, only what's discoverable in the file.

## Role in cartridge identification

**This exe is the identity.** `launcher.exe` carries a minisign signature appended past the
end of the image (see the [`sigblock`](../sigblock/) crate), and a volume is a cartridge
exactly when that signature verifies against a key the listener was built to trust *and*
declares itself a launcher. There is no marker file and no key on the disk; the full contract
is in [`../listener/structure.md`](../listener/structure.md#trust) and
[`../SIGNING.md`](../SIGNING.md).

The launcher's part in that is entirely passive: it is signed, and it is read. It carries no
secret, verifies nothing, and is never asked what it is — even its *version* is taken from the
signed comment rather than by running it with `--version`, so the listener has finished asking
questions before this program starts.

What is **not** covered by that signature is this program's own input. `catalog.json`,
`config.toml`, `assets/` and `games/` sit unsigned on the same disk, and nothing could sign
them, so the launcher treats them as untrusted: `catalog::is_contained` refuses any `exe` or
`image` path that could escape the cartridge, and `launch::spawn` re-checks before it spawns
anything. `Path::join` *discards* the base when handed `C:\…` or `\\host\share\…`, so without
that check a signed, genuine launcher would happily start any executable on the machine.

## Key source files

- `launcher/src/*.rs` — one file per job; the full map is under *Source layout* above.
  `main.rs` is the front door, `ui.rs` where the two halves meet, `constants.rs` where
  every tunable number lives.
- `launcher/src/index.html`, `style.css`, `app.js` — the embedded UI (toolbar, scrolling
  gallery, ordering, search, arranging, launch transition, launch states), split markup /
  look / behaviour. `style.css` and `app.js` each carry a header naming the three spacing
  numbers they duplicate from `constants.rs`.
- `launcher/src/config.toml`, `launcher/src/catalog.json` — the baked-in seeds copied
  into `output/` on first run.
- `launcher/Cargo.toml` — deps: `serde`, `serde_json`, `toml` (reading) and `toml_edit`
  (writing the three order keys back without disturbing the file), `rust-embed`
  (`include-exclude` feature), `tao`, `wry`, `windows-sys` (Windows only).

## Status

- [x] Webview shell, `app://` protocol, deterministic sizing, catalog + config
      (done-ish, ongoing polish).
- [x] Launching: cwd at the exe, waiting for the game's window before closing, the
      launch transition, the missing/failed cover states, and `logs/`. Remaining
      nice-to-haves are in [`TODO.md`](TODO.md).
- [x] The gallery: one-size covers, horizontal scrolling, four order modes, drag-arrange,
      search, and the three order keys written back to `config.toml`.
- [x] Code-signing: the exe's signature is the cartridge's identity, and the `.cartridge`
      marker is retired. This was the launcher's only remaining identity work.
- [x] Exercised end to end on real media — see
      [`../installer/structure.md`](../installer/structure.md#status--roadmap), which owns
      that item for the whole system.
