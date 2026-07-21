# Cartridge Game Launcher — Build Plan

A launcher that lives on each NVMe cartridge, shows a grid of game covers, and
launches the picked game. UI is HTML/CSS/SVG; a thin Rust shell (using `wry` +
`tao`) renders it and spawns the game process.

---

## TODO

1. [ ] Set up the Rust project in VS Code (rust-analyzer extension, `cargo new`,
       add `wry`, `tao`, `serde`, `serde_json` as dependencies).
2. [ ] Write `main.rs` — opens the window, loads `index.html`, reads
       `games.json`, and launches the picked `.exe` on click.
3. [ ] Write `index.html` — the styled cover grid.
4. [ ] Write `games.json` for gameA / gameB / gameC as a test.
5. [ ] Write `autorun.inf` so the drive looks like a cartridge and launches in
       one click.
6. [ ] Build, copy the compiled binary into `exe\`, and test end to end on a
       real cartridge.
7. [ ] Decide whether the one-click launch (Option A) is good enough, or
       whether to build the host-side auto-open agent (Option B) later.

---

## Explanation of each TODO item

### 1. Rust project setup
`cargo new launcher` gives you the project skeleton; add `wry`, `tao`, `serde`,
and `serde_json` to `Cargo.toml`. rust-analyzer in VS Code handles autocomplete
and build errors as you go.

### 2. main.rs
Opens a native window with an embedded webview pointed at `index.html`, hands
`games.json`'s contents to the page, and listens for a "launch this game"
message from the page to spawn the chosen `.exe` via `std::process::Command`.
Rust's standard library and `serde` remove most of the manual string/JSON
plumbing C++ would need here.

### 3. index.html
A grid of game cover cards styled with CSS; clicking a card sends the game's
exe path back to Rust to launch. This part is identical in spirit to the C++
version — plain HTML/CSS/SVG, no framework required.

### 4. games.json
The single source of truth for what's on the cartridge — one object per game
with `name`, `exe`, and `image`, paths written relative to the launcher's own
folder so the drive letter never matters.

### 5. autorun.inf
Sets the drive's icon, label, and AutoPlay action so the cartridge launches in
one click. True silent auto-run on insert isn't possible on modern Windows —
Microsoft disabled that for non-optical drives back in 2011.

### 6. Build and test
Build with `cargo build --release`, copy the resulting `.exe` into the
cartridge's `exe\` folder next to `index.html` and `games.json`, then test with
a couple of dummy games to confirm covers load and launching works.

### 7. Option A vs Option B (auto-open)
Option A (above) is one click and needs nothing installed on the host PC.
Option B is a separate small background program installed once per PC that
watches for device insertion and launches the cartridge automatically — only
worth building later if the one click becomes annoying.

---

## Findings from building/testing the first version

- **`file://` URLs crash wry's IPC on Windows.** Loading `index.html` via a
  `file:///C:/...` URL works for display, but the moment the page posts an
  IPC message back (close button, launch click), wry tries to parse that
  page's URL as an `http::Uri` internally — and the `http` crate's parser
  rejects the triple-slash empty-authority `file://` form outright, crashing
  the app. Fixed by serving everything through a custom `app://` protocol
  instead (`with_custom_protocol` + `with_url("app://localhost")`), which on
  Windows resolves to a normal `http://app.localhost/...` origin that parses
  fine. Verified stable over 30s+ and through a real close-button click.
- **Cover art isn't always what its extension claims.** One "cover.png" the
  user tried to use turned out to be an animated **WebP** file, not a
  PNG/APNG. The custom protocol handler now sniffs the actual file bytes
  (PNG/WebP/JPEG magic numbers) rather than trusting the extension, so it's
  served with the correct `Content-Type` either way.
- **`msedgewebview2.exe` is a shared system runtime**, not exclusive to this
  app — Windows' own Search/Start-menu host (`SearchHost.exe`) uses it too.
  When measuring this app's memory, filter processes by their
  `--user-data-dir` (ours lives under `output/interface/.webview2`) rather
  than just matching the process name, or you'll count unrelated system
  processes.
- **Real memory breakdown for this app alone** (3 cards, one of them the
  large animated WebP above): `launcher.exe` itself ~4-28MB (small, varies
  with what's been touched); the WebView2 process tree (browser + GPU +
  network + renderer, ~6 processes) is the bulk of it — around 100MB with
  static covers only, and 600MB+ once the large animated cover's many frames
  get decoded and cached by the renderer. The animated asset, not the
  webview engine itself, is what dominates in that case.
- Killing `launcher.exe` with `Stop-Process -Force` during testing orphans
  its WebView2 child-process tree instead of tearing it down — they don't
  show up under the app anymore but keep running and keep using RAM. A
  normal close (via the in-app close button) tears the tree down properly.

## TODO (not started — documented only, do not implement yet)

### TOP PRIORITY — non-webview prototype rewrites (do these first)

The current version uses HTML + WebView2 (Chromium). That drags in a `.webview2`
runtime-data folder (cache/cookies/IndexedDB/GPU shaders — only the shaders are
actually useful to us), ~100–600 MB RAM, and high CPU/power (fans spin up). The
clutter and the resource cost are both intrinsic to embedding a browser engine.

So before anything else, build **three separate prototype rewrites** that drop
the browser engine entirely, and compare them. Each is its own self-contained
version so we can evaluate side by side:

- [ ] **Slint version** — declarative native UI (`.slint` markup). Handles
      images, rounded corners, and animation natively; mature, purpose-built
      for launcher/kiosk UIs. Own `main.rs`, own `output/` folder.
- [ ] **egui version** — immediate-mode native GPU UI. Smallest/simplest; a row
      of rounded image-texture cards. Animated covers decoded frame-by-frame in
      Rust (APNG/WebP) and cycled as textures. Own `main.rs`, own `output/`.
- [ ] **Blitz version** — keep the existing `index.html`/CSS but render it with
      Stylo (Firefox/Servo's engine) + Vello instead of Chromium. No browser,
      no `.webview2` folder. Experimental; partial HTML/CSS support. Own
      `main.rs`, own `output/`.

Requirements common to all three:
  - Each prototype gets its **own `main.rs`** and its **own `output/` folder**
    (so the three can be built and run independently without clobbering each
    other or the existing WebView2 version).
  - Reuse the existing behavior: borderless/centered/non-resizable window,
    single-instance lock, in-app close button, card grid launching games,
    catalog + images/games sibling folders, captions toggle via `.ini`.
  - Measure and record RAM, CPU/power, on-disk clutter, and how each handles
    the animated cover art — that comparison decides which we keep.

### Remaining items (after the rewrites)

- [ ] Move the game catalog to `catalog.json`, sitting next to `launcher.exe`
      at the `output/` root (sibling of `images/` and `games/`) instead of
      inside `interface/`, matching how images/games already moved out.
- [ ] Investigate the high power draw / fan spin-up reported while the app
      is running. Prime suspect: `--disable-gpu` (added to trim memory) forces
      Chromium to composite and repaint entirely on the CPU, and that's
      expensive for a *continuously animating* cover image — likely fighting
      with the memory trim from the same change. May need to re-enable GPU
      compositing (trading some memory for much lower CPU/power use) and/or
      cap or pause offscreen/inactive card animations instead.
