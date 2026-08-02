# GaCaSy

> ## ⚠️ Status: vibecoded
>
> **Every line of this project so far is vibecoded.** It was written fast, with an LLM,
> to get the shape of the thing working — not carefully, not to any standard, and not
> reviewed line by line.
>
> It runs, but treat it accordingly: assume bugs, assume rough edges, assume things that
> only work because nothing has poked at them yet.
>
> I intend to go over the whole codebase myself later — read it end to end, clean it up,
> refactor it into my own style, and check that it is actually correct as far as I know
> how. Until that pass is done, this disclaimer stands.

**Games. Cartridge. System.** — turn any storage device into a game cartridge that
*just works* when you plug it in.

GaCaSy is made of three apps:

- **Launcher** — lives *on the cartridge*. A clean, full-screen wall of cover art;
  click a cover to launch the game.
- **Listener** — lives *on your PC*. When you plug in a GaCaSy cartridge, it recognizes
  it and starts that cartridge's launcher automatically — like slotting a cartridge into
  a console. On Windows that's a small background app; on Linux nothing runs at all until
  you plug something in. *(Windows build works; Linux not started.)*
- **Installer** — the one file you download. It turns blank media into a cartridge,
  installs the listener, and edits cartridges you already made. Everything else is carried
  inside it: no downloads, no prerequisites. *(Built; not yet tried on real media.)*

All three build and run on Windows. The manual setup below still works and is still the
documented path until the installer has been through an end-to-end run on a real drive.

> Developers: each app documents itself — [launcher/structure.md](launcher/structure.md)
> (cartridge side), [listener/structure.md](listener/structure.md) (PC side), and
> [installer/structure.md](installer/structure.md) (setup-side spec).

---

## Working tree

```text
GaCaSy/
  launcher/          The cartridge-side app (Rust + webview)
    src/             App code, the UI, and the seed data files
      main.rs          Entry point; one file per job beside it (ui, launch,
                       config, catalog, assets, window, log, constants, …)
      index.html       The UI's markup   (baked into the exe at build time)
      style.css        Its look          (same)
      app.js           Its behaviour     (same)
      catalog.json     Seed game list — name, exe path, cover image
      config.toml      Seed look & feel
    assets/
      fonts/
        BackOut.woff2  The typeface, also baked into the exe
    licenses/        Third-party licences the launcher's assets ship under:
      OFL-BackOut.txt  BackOut, by Frank Adebiaye with Ariel Martín Pérez
                       (Velvetyne Type Foundry), SIL Open Font License 1.1
    structure.md     Developer reference for the cartridge side
    TODO.md          What's left to build on the cartridge side
    output/          What ships on the cartridge:
      launcher.exe     the app
      games/           your game installs         (you drop these in)
      assets/images/   cover art, 600x900          (you drop these in)
      catalog.json     game list  (seeded from src/)
      config.toml      settings   (seeded from src/)
  listener/          The PC-side app (Windows built, Linux not started)
    src/
      main.rs        Entry point and --check handling
      volume.rs      The shared core: marker, trust check, launch
      marker.rs      Reading a cartridge's .cartridge file
      config.rs      Reading the listener's own config.toml
      log.rs         The activity log
      config.toml    Seed config, embedded in the exe
      trigger/       The only per-OS part
        windows.rs   Resident: hidden window + WM_DEVICECHANGE
        linux.rs     One-shot udev handoff — placeholder, not built
    output/          The deployed listener — what you actually run
      listener.exe   The program
      config.toml    Trusted keys  (seeded from src/; empty = trust all)
      listener.log   What it did, and why it ignored what it ignored
    README.md        What the listener is, in short
    structure.md     Spec for the PC-side listener — including the two
                     execution models (resident on Windows, one-shot on Linux)
    TODO.md          Build order for the listener
  installer/         The setup app (Rust + egui) — one self-contained exe
    build.rs         Stages the payload; fails the build if it's missing
    src/
      main.rs        Entry point and the module map
      app.rs         Wizard state and the create-vs-edit routing rule
      ui/            The screens; ui/mod.rs is the shell and the footer
      payload.rs     The embedded launcher, listener and seed files
      cartridge.rs   The write: copy, catalog, config, launcher, marker
      listener.rs    Job 2 — install folder, key merge, Run entry
      detect.rs      Finding a game's exe, and measuring the folder
      volume.rs      Which drives can be cartridges, and which already are
      copy.rs        The cancellable, measured file copy
      catalog.rs / marker.rs / image.rs / key.rs / work.rs
    structure.md     Reference for the setup side
    TODO.md          What's left — chiefly a run on real media
  Cargo.toml         Workspace tying the three crates together
  README.md          This file
  LICENSE            GNU GPL v3.0-or-later
```

---

## Install

### The launcher (cartridge)

1. **Build it** — from the launcher folder:
   ```sh
   cargo run       # builds and runs in place, refreshing output/launcher.exe
   # or
   cargo build --release
   ```
   Running once creates `output/` and seeds `config.toml` + `catalog.json` if missing.

2. **Add your games** — into `output/`:
   - Put each game's install under `output/games/…`.
   - Put each cover image (600×900, 2:3) under `output/assets/images/…`.
   - List them in `output/catalog.json`:
     ```json
     [
       { "name": "Elden Ring", "exe": "games/elden_ring/elden_ring.exe", "image": "assets/images/elden_ring.png" }
     ]
     ```
     Paths are relative to `output/`. Your edits here are **never overwritten** by a rebuild.

3. **Ship it** — copy the `output/` folder onto the cartridge (any storage device: NVMe,
   SSD, HDD, USB). `launcher.exe` and its content travel together.

### The listener (PC) — *Windows works, Linux not started*

Auto-starts a cartridge's launcher when you plug it in. Windows and Linux, built from one
codebase but working quite differently: on Windows it's a small app running quietly in the
background, on Linux it isn't running at all until the system wakes it on connect.

Until the installer ships, setting it up is manual:

1. `cd listener && cargo run --release -- --check .` — builds it, fills in
   `listener/output/` with `listener.exe` and a `config.toml` (the same way the launcher's
   `output/` works), and exits.
2. Put a `.cartridge` file at the root of the cartridge:

   ```toml
   version = 1
   key = "pick-any-string"
   launcher = "launcher.exe"
   ```

3. Run `output\listener.exe`. It stays in the background — no window, no tray icon — and
   starts the launcher when you plug the cartridge in. `output\listener.log`, right beside
   it, says what it did.

Out of the box the listener trusts **every** cartridge, so step 2 is all the pairing there
is. To restrict it to your own, list their keys in `output\config.toml`:

```toml
keys = ["pick-any-string"]
```

`listener.exe --check E:\` answers "would this cartridge launch?" without plugging anything
in. See [listener/README.md](listener/README.md) and
[listener/structure.md](listener/structure.md) for the rest.

### The installer — *built, not yet tried on real media*

The piece that makes all of the above unnecessary: one file that writes the cartridge and
sets up the listener for you. It carries the launcher, the listener and their seed files
inside itself and downloads nothing.

Build it in two steps — it embeds the other two binaries, so they have to exist first:

```sh
cargo build --release               # launcher + listener
cargo build --release -p installer  # embeds what that produced
```

`target/release/installer.exe` then does three things:

- **Make or edit a cartridge** — pick an **external** drive (internal disks and the one
  Windows is on are not offered), choose a key, add game folders (it finds each game's exe for
  you and asks when it can't be sure), pick covers, and copy. A drive that is already a
  cartridge opens for editing instead: add games, remove games, change the key.
- **Set up this PC** — installs the listener to `%LOCALAPPDATA%\GaCaSy\`, where it keeps its
  config and its log too. Pairs it with your cartridge's key, starts it, and registers it to
  start at login. **Nothing in the installer asks for administrator**, this included.
- **Uninstall** — removes the folder *and* the login entry.

See [installer/structure.md](installer/structure.md) for how it decides all of that.

---

## Use

1. Plug in the cartridge. With the Windows listener running, that is all you do.
2. Otherwise run `launcher.exe` yourself.
3. The launcher opens full of cover art:
   - **Click a cover** to launch that game.
   - **Click the close button** (top-right) to exit.
4. Tweak the look by editing `config.toml` — background color, spacing, corner rounding,
   card shadow, and whether game titles show under the covers. Blank or invalid values
   fall back to sensible defaults.

---

## License

[GNU GPL v3.0-or-later](LICENSE) © 2026 da1sh1n. Free software: use, study, modify,
and share it freely. Any fork or modified version must stay open under the same license
(GPL-3.0-or-later) — the freedom travels with the code.
