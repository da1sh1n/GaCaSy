# GaCaSy Installer — Setup Side (spec to build)

Part of the **GaCaSy** game-cartridge system. This document covers the **installer**: the
single file an end user downloads, which turns blank media into a cartridge, installs the
PC-side listener, and edits cartridges that already exist. The cartridge-side app is
documented in [`../launcher/structure.md`](../launcher/structure.md), the PC-side service in
[`../listener/structure.md`](../listener/structure.md); the user-facing overview is in
[`../README.md`](../README.md).

> **This side does not exist yet.** Treat this doc as a *spec to build by*.

## Purpose

The installer is the only thing a user has to obtain. Everything else — `launcher.exe` on
the cartridge, `listener.exe` on the PC, the config and catalog files — is *placed by it*.
It never downloads anything: **one self-contained exe, no internet, no prerequisites, no
side-by-side files.**

## Shape

- **Its own crate**, `installer/`, sibling to `launcher/` and `listener/`. A root
  `Cargo.toml` workspace should tie the three together — none exists today
  (`launcher/Cargo.toml` is a standalone crate).
- **`eframe` / `egui`** for the UI: pure Rust, statically linked, no runtime dependency.
  This is the reason it is not a webview like the launcher — a WebView2-based installer that
  finds the runtime missing has no way to bootstrap itself with no internet.
- `#![windows_subsystem = "windows"]`, no console window (same as the launcher).
- **Windows for v1.** The Linux equivalents of every Windows-shaped step (`Program Files`,
  registry autostart, `launcher.exe`) are noted under [Future](#future) so the design
  doesn't paint itself into a corner.
- **Requires elevation** — a UAC manifest, for the `Program Files` write in job 2.

### Embedded payload

The installer carries its outputs inside itself, via `include_bytes!` / `rust-embed`:

```text
installer.exe
  ├─ launcher.exe      ← written onto the cartridge
  ├─ listener.exe      ← written into Program Files
  ├─ config.toml       ← seed, from launcher/src/config.toml
  └─ catalog.json      ← seed, from launcher/src/catalog.json
```

Build-ordering consequence: **launcher and listener must be built `--release` before the
installer.** The build should fail loudly with a clear message when a payload artifact is
missing, rather than producing an installer that ships nothing.

## Why not part of the listener

The installer is deliberately *not* a second mode of `listener.exe`. It has to embed
`launcher.exe` regardless, so embedding the listener alongside it costs no extra machinery —
whereas bundling would push the whole wizard UI into a process that has to stay small and
cross-platform: one that runs at every login on Windows, and that udev fires and kills per
connection on Linux, where `Program Files` means nothing and a wizard has nobody to show
itself to.

## Flow

Which job runs is decided by the target volume:

```text
volume picked  →  has .cartridge?  →  yes → edit mode  (add / remove games, change key)
                        │ no
                        └─────────→ create mode (key → games → exes → images → copy)
```

Installing the listener (job 2) is independent of both and can be run on its own.

## Job 1 — Create a cartridge

1. **Pick the target volume.** Enumerate mounted volumes (same "any mounted storage volume"
   rule the listener uses — NVMe / SSD / HDD / USB alike, never a specific USB id). A volume
   *with* a `.cartridge` at its root routes to [job 3](#job-3--edit-an-existing-cartridge);
   one *without* lands here.
2. **Choose the key.** Required before installation can start — see [Keys](#keys).
3. **Add games.** The user picks one or more game folders.
4. **Per game:** auto-find the executable (below), then pick a cover image.
5. **Review**, check free space, then copy — with a progress bar on a worker thread. Game
   folders run to many GB; the UI must stay responsive and the copy must be cancellable.
6. **Write the cartridge layout.**

### Resulting layout

Matches the launcher's deployed layout
([`../launcher/structure.md`](../launcher/structure.md#deployed-layout)) exactly:

```text
<volume>/
  launcher.exe     <- the app, from the embedded payload
  config.toml      <- look and feel only; the key is not here
  catalog.json     <- the game list the installer just built
  images/          <- one cover per game
  games/           <- the copied game installs
  .cartridge       <- identity marker at the volume root
```

`EBWebView/` is **not** created by the installer — the launcher makes it on first run.

### Executable auto-detection

The fiddliest part of job 1. For each chosen game folder:

- Recursively collect every `*.exe`.
- **Reject** known non-game names: `unins*`, `*setup*`, `vcredist*`, `dxsetup`, `directx*`,
  `*crashhandler*` (e.g. `UnityCrashHandler64.exe`), and anything under `redist/`,
  `_CommonRedist/` or `Engine/Binaries/ThirdParty/`.
- **Score** the survivors: shallower path wins, a name matching the folder name wins, larger
  file wins.
- One clear winner → preselect it. Ambiguous, or nothing left → the user **must** pick
  manually.
- The user can **always** override the pick, including when detection succeeded.

### Cover images

One image per game, chosen by the user, copied to `images/<slug>.png`. The launcher's native
cover size is **600×900 (2:3)** — `COVER_NATIVE_WIDTH` / `COVER_NATIVE_HEIGHT` in
[`../launcher/src/constants.rs`](../launcher/src/constants.rs). v1 copies the file as-is and **warns**
on a non-2:3 ratio rather than resizing it, which keeps the exe small and dependency-free.

### Catalog writing

`catalog.json` is the array of `{ name, exe, image }` the launcher deserializes into its
`Game` struct. Paths are **relative to the cartridge root** (`games/bg3/bg3.exe`,
`images/bg3.png`), and `name` defaults to the game folder's name, editable by the user.

## Job 2 — Install the listener

Steps 1 and 2 are the same everywhere. **Step 3 is not** — the listener's two builds have
different process lifetimes (see
[Execution models](../listener/structure.md#execution-models)), so "make it run" means two
unrelated things:

1. Copy the embedded listener binary into place — `C:\Program Files\GaCaSy\listener.exe` on
   Windows.
   > The listener keeps its exe, config and log in one folder, and `Program Files` is the
   > one location where that can't hold: the user it runs as can't write there, so its log
   > falls back to `%LOCALAPPDATA%\GaCaSy\listener.log`. If the installer would rather keep
   > all three together, `%LOCALAPPDATA%\Programs\GaCaSy\` is writable and needs no
   > elevation at all — which would also remove the only reason job 2 requires admin.
2. Write its `config.toml` beside it. If one already exists, **append** the new key to its
   `keys` list rather than overwriting the file — see [Keys](#keys). Note that an *empty*
   `keys` list means the listener trusts **every** cartridge
   ([why](../listener/structure.md#an-empty-keys-list-trusts-everything)), so writing the key
   is what narrows the PC down to the cartridges the user actually made — the installer is
   tightening a default that starts open, not opening one that starts shut.
3. **Make it run — per OS:**
   - **Windows (v1)** — the listener is a **resident** process, so register it to start at
     login: an `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` entry pointing at the
     Program Files exe. Per-user, and the registry write itself needs no admin (the Program
     Files copy in step 1 still does). Alternatives worth keeping in mind: `HKLM\…\Run` for
     all users, or a Task Scheduler logon trigger if it ever needs to start elevated.
   - **Linux (future)** — there is **nothing to autostart**. The listener is one-shot and udev
     starts it per event, so the installer's whole job is dropping
     `/etc/udev/rules.d/99-gacasy.rules` and running `udevadm control --reload`. Note the
     elevation shape inverts: this needs **root** for a system-wide rules directory, where
     Windows needed admin only for the binary copy and not for the per-user `Run` key.
4. **Repair / uninstall:** detect an existing install and offer to replace or remove it.
   Removing means deleting the folder *and* undoing step 3 — the `Run` entry on Windows, or
   the rule file plus a `udevadm control --reload` on Linux.

## Job 3 — Edit an existing cartridge

When the picked volume already carries a `.cartridge`, read its `config.toml` and
`catalog.json` and let the user:

- **Add games** — same flow as job 1, steps 3–5, appending to the existing catalog.
- **Remove games** — delete the game folder and its image, drop the catalog entry.
- **Change the key** — rewrite `.cartridge`. Nothing else on the cartridge holds the key.

> Changing the key un-pairs the cartridge: any PC whose listener config doesn't know the new
> key stops auto-launching it until the listener is updated too.

## Keys

The installer does **not** own the identification model — it is a cartridge ↔ PC contract,
defined in
[`../listener/structure.md`](../listener/structure.md#cartridge-identification-system). The
installer is just the tool that writes both halves into place:

- **Onto the cartridge** — the `.cartridge` marker at the volume root, carrying that
  cartridge's key.
- **Onto the PC** — the key, **appended** to the `keys` list in the listener's
  `config.toml`, so pairing a new cartridge never un-pairs an existing one.

The cartridge's own `config.toml` holds **no key** — it is look-and-feel only, and the
launcher has no identity role at all.

What the installer decides is only the user-facing part: the user **chooses the key before
installation starts**, either typing their own or accepting a generated random one (32 hex
chars). Reusing one key across several cartridges is fine and expected — that is what makes
them all work against a single listener install.

## Open questions

- What happens when the **same game folder is added twice** — reject, rename, or overwrite?
- **Free-space check** before the copy (fast, can be wrong about compression/sparse files) or
  handle the failure mid-copy (accurate, but leaves a half-written cartridge to roll back)?
- **Formatting or erasing media** is out of scope for v1 — the installer writes to a volume,
  it never repartitions one.

## Future

- **Linux target:** `/opt/gacasy` or `~/.local/share/gacasy` instead of Program Files, a
  **udev rule instead of a Run key**, and a Linux launcher binary instead of `launcher.exe`.
  The Linux listener is *not* a service and has nothing to autostart — it is started by udev
  per connection and exits, so there is no user unit to enable. See
  [`../listener/structure.md`](../listener/structure.md#linux--reactive-one-shot).
- **v2 trust:** once `launcher.exe` is officially code-signed, the listener verifies the
  exe's signature instead of a shared key. `.cartridge` is retired, so the installer stops
  writing markers and stops asking for a key at all — the key screen disappears from the
  wizard.

## Status / roadmap

- [ ] Root `Cargo.toml` workspace tying `launcher` + `listener` + `installer`.
- [ ] `installer` crate scaffold: `eframe`/`egui`, UAC manifest, `windows_subsystem`.
- [ ] Embedded payload + build-time check that every artifact is present.
- [ ] Volume enumeration and create-vs-edit routing on `.cartridge`.
- [ ] Key screen: generate or type, validated, required before install.
- [ ] Game folder picker, executable auto-detection, manual override.
- [ ] Per-game cover image picker with the 2:3 warning.
- [ ] Cartridge write: threaded `games/` copy with progress + cancel, images, catalog,
      `config.toml` (no key), `.cartridge` marker.
- [ ] Listener install: Program Files copy, key appended to the config's `keys` list, and
      per-OS activation — a `Run` entry on Windows, a udev rule plus reload on Linux.
- [ ] Edit mode: add games, remove games, change key.
- [ ] Free-space precheck and failure/rollback handling.
- [ ] Uninstall / repair path.
- [ ] Future: Linux target.
