# GaCaSy

**Games. Cartridge. System.** — turn any storage device into a game cartridge that
*just works* when you plug it in.

GaCaSy is made of two apps:

- **Launcher** — lives *on the cartridge*. A clean, full-screen wall of cover art;
  click a cover to launch the game.
- **Listener** — runs *on your PC* in the background. When you plug in a GaCaSy
  cartridge, it recognizes it and starts that cartridge's launcher automatically —
  like slotting a cartridge into a console. *(Coming soon; not built yet.)*

Today the **launcher works**. The **listener is upcoming**, so for now you start the
launcher yourself.

> Developers: each app documents itself — [launcher/structure.md](launcher/structure.md)
> (cartridge side) and [listener/structure.md](listener/structure.md) (PC-side spec).

---

## Working tree

```
GaCaSy/
  launcher/          The cartridge-side app (Rust + webview)
    src/             App code
    ui/              The UI (baked into the exe at build time)
    catalog.json     Your game list — name, exe path, cover image
    config.ini       Look & feel (and, later, the cartridge key)
    structure.md     Developer reference for the cartridge side
    output/          What ships on the cartridge:
      launcher.exe     the app
      games/           your game installs         (you drop these in)
      images/          cover art, 600x900          (you drop these in)
      catalog.json     game list  (copied here)
      config.ini       settings   (copied here)
  listener/          The PC-side app (coming soon)
    structure.md     Spec for the PC-side listener
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
   Running once creates `output/` and seeds `config.ini` + `catalog.json` if missing.

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

A background service (Windows + Linux) that auto-starts a cartridge's launcher on
connect. Not available yet; see [listener/structure.md](listener/structure.md) for the plan.

---

## Use

1. Plug in the cartridge (once the listener ships, this is all you do).
2. Run `launcher.exe` (until then, start it manually).
3. The launcher opens full of cover art:
   - **Click a cover** to launch that game.
   - **Click the close button** (top-right) to exit.
4. Tweak the look by editing `config.ini` — background color, spacing, corner rounding,
   card shadow, and whether game titles show under the covers. Blank or invalid values
   fall back to sensible defaults.

---

## License

[GNU GPL v3.0-or-later](LICENSE) © 2026 da1sh1n. Free software: use, study, modify,
and share it freely. Any fork or modified version must stay open under the same license
(GPL-3.0-or-later) — the freedom travels with the code.
