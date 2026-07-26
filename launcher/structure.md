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
  images/          <- cover art (600x900, 2:3), dropped in by hand
  games/           <- the actual game installs
  logs/            <- launcher.log (every launch attempt) + <game>/out.log, err.log
  EBWebView/       <- WebView2's own data folder (its only on-disk crumbs; safe to delete)
  .cartridge       <- identity marker: written by the installer, read by the listener;
                      the launcher never touches it (design, not yet in code)
```

`catalog.json`, `images/` and `games/` are **never overwritten** once present, so
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
    instance.rs    <- the single-instance mutex
    index.html     <- the UI, embedded by rust-embed and served over app://
    config.toml    <- seed config, embedded via include_str! and written to output/
    catalog.json   <- seed game list, same
  structure.md
  output/          <- the deployed cartridge content (see above)
```

One file per job, and `main.rs` is only the front door. Two conventions hold the split
together: **every tunable number lives in `constants.rs`** (the module that owns each one
is named in its section header), and the only file that knows both halves of the app is
`ui.rs`, where config and catalog go out to the page and clicks come back.

Because `src/` holds both source and web assets, `UiAssets`' rust-embed include list
(`*.html`, `*.css`, `*.js`) is what keeps the Rust sources and the two seed files out of
the bundle; `is_ui_asset()` applies the same extension list at runtime so the dev path
can't serve them either. That pair of lists is the one deliberate exception to the
constants rule — `UI_ASSET_EXTENSIONS` stays beside the embed list it mirrors, because
apart they drift.

## How it runs

- **No server, no port.** UI assets are embedded via `rust-embed` and served over the
  `app://` custom protocol. On Windows this resolves to an `http://app.localhost/...`
  origin, which keeps wry's IPC (close / launch clicks) working — a raw `file://`
  origin would crash IPC.
- **Content vs. UI.** `assets::handle_request` serves `images/…` and `games/…` from disk
  beside the exe, and everything else as a UI asset (404 unless it passes
  `is_ui_asset`). In dev it prefers the **live** file from the source `src/` folder
  (`CARGO_MANIFEST_DIR/src/…`) so HTML edits show up on the next launch with no
  rebuild, falling back to the embedded copy on a deployed cartridge. Responses send
  `Cache-Control: no-store` so WebView2 never serves stale HTML/art.
- **Single instance.** A Windows **named mutex** (`Local\GaCaSy.CartridgeLauncher`),
  not a socket. Enforced **only for the deployed exe** (its parent folder is named
  `output/`); skipped under `cargo run` (exe in `target/`) so a rebuild always opens a
  fresh window instead of silently no-opping on the held lock.
- **Window sizing is deterministic.** `window.rs` computes the window size and the CSS
  obeys the same numbers — no measure-and-report round-trip. Each cover's target width is
  `IMAGE_WIDTH_FRACTION × screen width` (capped at the native 600px so never upscaled),
  height follows the 2:3 ratio. A single `cover_scale = min(1, width_room/(n·target_w),
  height_room/target_h)` shrinks covers on both axes together, bounded by
  `MAX_WIDTH_FRACTION` (row ≤ that × screen width) and `MAX_HEIGHT_FRACTION` (window ≤
  that × screen height); all three fractions are in `constants.rs`, like every other
  number the config doesn't expose. The window wraps the covers on both axes; margins/gap are in
  **logical (CSS) px** so they match the page's `PAD`/`GAP`. The page's `layout()`
  reproduces the same fit independently (scaling down only).
- **GUI app**, `#![windows_subsystem = "windows"]` — no console window.

## Launching a game

Clicking a cover posts `launch:<index>` over IPC (the close button posts `close`). What
follows is in `launch.rs`, and the guiding rule is that **"started" means the game's
window is up** — the launcher closes itself on that signal, and closing while the game is
still an invisible process makes a working launch look like a broken one.

1. **Spawn.** `output/<game.exe>`, with the cwd set to **the exe's own folder** — games
   resolve their assets relative to themselves, and the wrong cwd fails in ways that look
   like corruption. stdout/stderr are redirected into `logs/<game>/`.
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
  dims behind `overlay_color`, a segmented ring spins in the middle of the screen, and a
  faint "Starting …" sits along the bottom edge. The ring is one solid stroke with
  `loading_ring_segments` gaps dashed out of it, each gap half the ring's thickness, so
  every piece is cut square across the band rather than being a round dot.
- **Failed.** The transition unwinds, the covers come back, and that one keeps a border
  in `error_border_color` with a short message under it. It stays clickable — choosing it
  again retries, which clears the mark. The unwind waits until the loading state has been
  up for `MIN_LOADING_AFTER_FAIL` (`constants.rs`, 1 s, measured from the click and sent
  to the page as `__UI__.minLoadingAfterFail`): a missing file fails in milliseconds, and
  a ring that flashes and vanishes reads as a glitch rather than as an attempt that was
  made and didn't work. Not a `config.toml` knob — change it in `constants.rs`.
- **Missing.** A game whose exe isn't on the cartridge is settled before the player
  touches anything: Rust checks each `exe` at startup and passes `available` to the page,
  which dims that cover to `missing_dim`, draws a sign over it in `missing_sign_color`
  and disables the button. Checked once, so a cartridge that changes under a running
  launcher needs a restart.

## Logs

The launcher has no console, so `logs/` is the only place a failure can be explained:

- `logs/launcher.log` — every attempt, pid, and the **full** OS error text (the UI only
  ever shows one short sentence). Appended, rewritten from scratch past 1 MB.
- `logs/<game>/out.log`, `logs/<game>/err.log` — that game's own console output,
  truncated per launch so they always describe the current run. `<game>` is the catalog
  name reduced to `[a-z0-9-]`.

## Data files

- **`catalog.json`** — an array of `{ name, exe, image }`. `exe` and `image` are paths
  relative to `output/` (e.g. `games/bg3/bg3.exe`, `images/bg3.png`). Injected into the
  page as `window.__GAMES__` (fetching it would hit CORS) — rebuilt rather than passed
  through verbatim, so each entry can carry `available` (see *Launching a game*).
- **`config.toml`** — real TOML, parsed with the `toml` crate. `config::load()` reads it
  as a `toml::Table` and pulls one key at a time rather than deserializing into a
  struct, so unknown keys and wrong-typed values cost only that setting (it falls back
  to its default) and an older config still works; only a file that isn't valid TOML at
  all drops every setting to defaults. Knobs: `show_captions` (bool), `border_gap`,
  `image_gap`, `corner_radius`, `shadow_size`, `shadow_fade`, `error_border_width`,
  `missing_dim`, `loading_ring_segments`, `loading_ring_speed` (turns per second, floored
  at 0.05), `loading_text_gap` (non-negative numbers), and `background_color`,
  `shadow_color`, `overlay_color`, `loading_ring_color`, `loading_text_color`,
  `error_border_color`, `error_text_color`, `missing_sign_color` (quoted CSS color
  strings). Every one of them is handed to the page as a CSS variable.

## Role in cartridge identification

**None today.** The launcher carries no key and writes no marker — it is simply the app the
listener starts. The identity lives in the `.cartridge` marker at the volume root, written
by the installer and checked by the listener against its own trusted key list; the full
contract is in
[`../listener/structure.md`](../listener/structure.md#cartridge-identification-system).

Under **v2** that changes shape rather than coming back: once `launcher.exe` is officially
code-signed, **the exe's signature becomes the identity**. The listener then verifies the
binary directly and `.cartridge` is retired — so the launcher's only contribution to trust
is being a signed binary, never a secret it has to carry.

## Key source files

- `launcher/src/*.rs` — one file per job; the full map is under *Source layout* above.
  `main.rs` is the front door, `ui.rs` where the two halves meet, `constants.rs` where
  every tunable number lives.
- `launcher/src/index.html` — the embedded UI (layout, launch transition, launch states).
- `launcher/src/config.toml`, `launcher/src/catalog.json` — the baked-in seeds copied
  into `output/` on first run.
- `launcher/Cargo.toml` — deps: `serde`, `serde_json`, `toml`, `rust-embed`
  (`include-exclude` feature), `tao`, `wry`, `windows-sys` (Windows only).

## Status

- [x] Webview shell, `app://` protocol, deterministic sizing, catalog + config
      (done-ish, ongoing polish).
- [x] Launching: cwd at the exe, waiting for the game's window before closing, the
      launch transition, the missing/failed cover states, and `logs/`. Remaining
      nice-to-haves are in [`TODO.md`](TODO.md).
- [ ] v2: official code-signing of `launcher.exe`, so the listener verifies the exe's
      signature and the `.cartridge` marker can be retired entirely. This is the
      launcher's only remaining identity work.
