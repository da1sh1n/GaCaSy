# GaCaSy Launcher — Cartridge Side (developer reference)

Part of the two-app **GaCaSy** game-cartridge system. This document covers the
**launcher**: the app that lives on the cartridge, shows its games, and launches them.
The PC-side companion is documented in [`../listener/structure.md`](../listener/structure.md);
the user-facing overview and install/use steps are in [`../README.md`](../README.md).

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
  config.ini       <- look-and-feel + identity; seeded from a baked-in default if missing
  catalog.json     <- the game list (name / exe / image); seeded likewise
  images/          <- cover art (600x900, 2:3), dropped in by hand
  games/           <- the actual game installs
  ui/              <- WebView2's own data folder (its only on-disk crumbs; safe to delete)
  .cartridge       <- identity marker read by the PC listener (design, not yet in code)
```

`config.ini`, `catalog.json`, `images/` and `games/` are **never overwritten** once
present, so hand-dropped content survives every build.

Under `cargo run` the crate root (`launcher/`) *is* the base dir, so the deployed
`output/` above corresponds to `launcher/output/` in the repo.

## How it runs

- **No server, no port.** UI assets are embedded via `rust-embed` and served over the
  `app://` custom protocol. On Windows this resolves to an `http://app.localhost/...`
  origin, which keeps wry's IPC (close / launch clicks) working — a raw `file://`
  origin would crash IPC.
- **Content vs. UI.** `handle_app_request` serves `images/…` and `games/…` from disk
  beside the exe, and everything else as a UI asset. In dev it prefers the **live**
  file from the source `ui/` folder (`CARGO_MANIFEST_DIR/ui/…`) so HTML edits show up
  on the next launch with no rebuild, falling back to the embedded copy on a deployed
  cartridge. Responses send `Cache-Control: no-store` so WebView2 never serves stale
  HTML/art.
- **Single instance.** A Windows **named mutex** (`Local\GaCaSy.CartridgeLauncher`),
  not a socket. Enforced **only for the deployed exe** (its parent folder is named
  `output/`); skipped under `cargo run` (exe in `target/`) so a rebuild always opens a
  fresh window instead of silently no-opping on the held lock.
- **Window sizing is deterministic.** Rust computes the window size and the CSS obeys
  the same numbers — no measure-and-report round-trip. Each cover's target width is
  `IMAGE_WIDTH_FRACTION × screen width` (capped at the native 600px so never upscaled),
  height follows the 2:3 ratio. A single `cover_scale = min(1, width_room/(n·target_w),
  height_room/target_h)` shrinks covers on both axes together, bounded by
  `MAX_WIDTH_FRACTION` (row ≤ that × screen width) and `MAX_HEIGHT_FRACTION` (window ≤
  that × screen height). The window wraps the covers on both axes; margins/gap are in
  **logical (CSS) px** so they match the page's `PAD`/`GAP`. The page's `layout()`
  reproduces the same fit independently (scaling down only).
- **Launching a game.** Clicking a cover posts `launch:<index>` over IPC; Rust spawns
  `output/<game.exe>` with the cwd set to `output/`. The close button posts `close`.
- **GUI app**, `#![windows_subsystem = "windows"]` — no console window.

## Data files

- **`catalog.json`** — an array of `{ name, exe, image }`. `exe` and `image` are paths
  relative to `output/` (e.g. `games/bg3/bg3.exe`, `images/bg3.png`). Injected into the
  page as `window.__GAMES__` (fetching it would hit CORS).
- **`config.ini`** — `key = value` lines, `#` comments. Unknown keys and unparseable
  values are ignored (fall back to defaults), so an older config still works. Knobs:
  `show_captions`, `border_gap`, `image_gap`, `corner_radius`, `background_color`,
  `shadow_size`, `shadow_fade`, `shadow_color`.

## Role in cartridge identification

The launcher is the *identified* half of the handshake the listener performs (full flow
in [`../listener/structure.md`](../listener/structure.md#cartridge-identification-system)).
Its obligations:

- Carry an identity **`key`** in `config.ini`.
- Write a **`.cartridge`** marker file at the volume root holding that same key.

> Neither the `config.ini` `key` nor the `.cartridge` file exists in the code yet — both
> are part of the identification design and are added when that system is implemented.

## Key source files

- `launcher/src/main.rs` — the whole Rust shell (protocol handler, sizing, IPC, seeding).
- `launcher/ui/index.html` — the embedded UI (layout + launch/close JS).
- `launcher/Cargo.toml` — deps: `serde`, `serde_json`, `rust-embed`, `tao`, `wry`,
  `windows-sys` (Windows only).

## Status

- [x] Webview shell, `app://` protocol, deterministic sizing, catalog + config,
      game launching (done-ish, ongoing polish).
- [ ] Add `key` to `config.ini` and write the `.cartridge` marker.
- [ ] v2: official code-signing of `launcher.exe` (so the listener can verify a
      signature instead of a shared key).
