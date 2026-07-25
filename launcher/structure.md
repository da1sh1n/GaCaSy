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
  EBWebView/       <- WebView2's own data folder (its only on-disk crumbs; safe to delete)
  .cartridge       <- identity marker: written by the installer, read by the listener;
                      the launcher never touches it (design, not yet in code)
```

`config.toml`, `catalog.json`, `images/` and `games/` are **never overwritten** once
present, so hand-dropped content survives every build.

Under `cargo run` the crate root (`launcher/`) *is* the base dir, so the deployed
`output/` above corresponds to `launcher/output/` in the repo.

## Source layout

Everything the crate owns lives in `src/` — the Rust shell, the UI, and the two seed
data files baked into the exe:

```text
launcher/
  Cargo.toml
  src/
    main.rs        <- the whole Rust shell
    index.html     <- the UI, embedded by rust-embed and served over app://
    config.toml    <- seed config, embedded via include_str! and written to output/
    catalog.json   <- seed game list, same
  structure.md
  output/          <- the deployed cartridge content (see above)
```

Because `src/` holds both source and web assets, `UiAssets`' rust-embed include list
(`*.html`, `*.css`, `*.js`) is what keeps `main.rs` and the two seed files out of the
bundle; `is_ui_asset()` applies the same extension list at runtime so the dev path
can't serve them either.

## How it runs

- **No server, no port.** UI assets are embedded via `rust-embed` and served over the
  `app://` custom protocol. On Windows this resolves to an `http://app.localhost/...`
  origin, which keeps wry's IPC (close / launch clicks) working — a raw `file://`
  origin would crash IPC.
- **Content vs. UI.** `handle_app_request` serves `images/…` and `games/…` from disk
  beside the exe, and everything else as a UI asset (404 unless it passes
  `is_ui_asset`). In dev it prefers the **live** file from the source `src/` folder
  (`CARGO_MANIFEST_DIR/src/…`) so HTML edits show up on the next launch with no
  rebuild, falling back to the embedded copy on a deployed cartridge. Responses send
  `Cache-Control: no-store` so WebView2 never serves stale HTML/art.
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
- **`config.toml`** — real TOML, parsed with the `toml` crate. `load_config()` reads it
  as a `toml::Table` and pulls one key at a time rather than deserializing into a
  struct, so unknown keys and wrong-typed values cost only that setting (it falls back
  to its default) and an older config still works; only a file that isn't valid TOML at
  all drops every setting to defaults. Knobs: `show_captions` (bool), `border_gap`,
  `image_gap`, `corner_radius`, `shadow_size`, `shadow_fade` (non-negative numbers), and
  `background_color`, `shadow_color` (quoted CSS color strings).

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

- `launcher/src/main.rs` — the whole Rust shell (protocol handler, sizing, IPC, seeding).
- `launcher/src/index.html` — the embedded UI (layout + launch/close JS).
- `launcher/src/config.toml`, `launcher/src/catalog.json` — the baked-in seeds copied
  into `output/` on first run.
- `launcher/Cargo.toml` — deps: `serde`, `serde_json`, `toml`, `rust-embed`
  (`include-exclude` feature), `tao`, `wry`, `windows-sys` (Windows only).

## Status

- [x] Webview shell, `app://` protocol, deterministic sizing, catalog + config,
      game launching (done-ish, ongoing polish).
- [ ] v2: official code-signing of `launcher.exe`, so the listener verifies the exe's
      signature and the `.cartridge` marker can be retired entirely. This is the
      launcher's only remaining identity work.
