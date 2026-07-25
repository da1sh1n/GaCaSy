# GaCaSy

**Games. Cartridge. System.** — turn any storage device into a game cartridge that
*just works* when you plug it in.

GaCaSy is made of three apps:

- **Launcher** — lives *on the cartridge*. A clean, full-screen wall of cover art;
  click a cover to launch the game.
- **Listener** — lives *on your PC*. When you plug in a GaCaSy cartridge, it recognizes
  it and starts that cartridge's launcher automatically — like slotting a cartridge into
  a console. On Windows that's a small background app; on Linux nothing runs at all until
  you plug something in. *(Coming soon; not built yet.)*
- **Installer** — the one file you download. It turns blank media into a cartridge,
  installs the listener, and edits cartridges you already made. *(Coming soon; not built
  yet.)*

Today the **launcher works**. The **listener and installer are upcoming**, so for now you
copy files onto the cartridge and start the launcher yourself.

> Developers: each app documents itself — [launcher/structure.md](launcher/structure.md)
> (cartridge side), [listener/structure.md](listener/structure.md) (PC-side spec), and
> [installer/structure.md](installer/structure.md) (setup-side spec).

---

## Working tree

```text
GaCaSy/
  launcher/          The cartridge-side app (Rust + webview)
    src/             App code, the UI, and the seed data files
      main.rs          The Rust shell
      index.html       The UI (baked into the exe at build time)
      catalog.json     Seed game list — name, exe path, cover image
      config.toml      Seed look & feel
    structure.md     Developer reference for the cartridge side
    TODO.md          What's left to build on the cartridge side
    output/          What ships on the cartridge:
      launcher.exe     the app
      games/           your game installs         (you drop these in)
      images/          cover art, 600x900          (you drop these in)
      catalog.json     game list  (seeded from src/)
      config.toml      settings   (seeded from src/)
  listener/          The PC-side app (coming soon)
    README.md        What the listener is, in short
    structure.md     Spec for the PC-side listener — including the two
                     execution models (resident on Windows, one-shot on Linux)
    TODO.md          Build order for the listener
  installer/         The setup app (coming soon)
    structure.md     Spec for the setup side — makes cartridges, installs
                     the listener, edits existing cartridges
    TODO.md          Build order for the installer
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
   - Put each cover image (600×900, 2:3) under `output/images/…`.
   - List them in `output/catalog.json`:
     ```json
     [
       { "name": "Elden Ring", "exe": "games/elden_ring/elden_ring.exe", "image": "images/elden_ring.png" }
     ]
     ```
     Paths are relative to `output/`. Your edits here are **never overwritten** by a rebuild.

3. **Ship it** — copy the `output/` folder onto the cartridge (any storage device: NVMe,
   SSD, HDD, USB). `launcher.exe` and its content travel together.

### The listener (PC) — *coming soon*

Auto-starts a cartridge's launcher when you plug it in. Windows and Linux, built from one
codebase but working quite differently: on Windows it's a small app running quietly in the
background, on Linux it isn't running at all until the system wakes it on connect. Not
available yet; see [listener/structure.md](listener/structure.md) for the plan.

### The installer — *coming soon*

The piece that makes all of the above unnecessary: one download that writes the cartridge
and sets up the listener for you. See [installer/structure.md](installer/structure.md).

---

## Use

1. Plug in the cartridge (once the listener ships, this is all you do).
2. Run `launcher.exe` (until then, start it manually).
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
