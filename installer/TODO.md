# GaCaSy Installer — TODO

Build order for the installer specced in [`structure.md`](structure.md). Nothing here is
started; the folder currently holds documentation only.

## Foundation

- [ ] Root `Cargo.toml` workspace tying `launcher` + `listener` + `installer` together
      (today `launcher/Cargo.toml` is a standalone crate and there is no root manifest).
- [ ] `installer` crate scaffold — `eframe`/`egui`, `#![windows_subsystem = "windows"]`,
      UAC manifest for the Program Files write.
- [ ] Embed the payload (`launcher.exe`, `listener.exe`, seed `config.toml` +
      `catalog.json`) via `include_bytes!` / `rust-embed`.
- [ ] Fail the build with a clear message when a payload artifact is missing — launcher and
      listener must be built `--release` first.
- [ ] Wizard shell: step navigation, back/next, cancel, error surface.

## Cartridge creation (job 1)

- [ ] Enumerate mounted volumes; route on presence of `.cartridge` (create vs edit).
- [ ] Key screen — generate a random 32-hex key or accept a typed one; validate; block
      installation until chosen.
- [ ] Game folder picker (one or many).
- [ ] Executable auto-detection: recursive `*.exe` scan, reject-list, scoring, preselect the
      winner.
- [ ] Manual executable override — always available, required when detection is ambiguous
      or finds nothing.
- [ ] Per-game cover image picker; warn on a non-2:3 ratio (no resizing in v1).
- [ ] Editable game name, defaulting to the folder name.
- [ ] Free-space precheck before the copy starts.
- [ ] Threaded `games/` copy with a progress bar and working cancel.
- [ ] Write `images/`, `catalog.json` (paths relative to the cartridge root), `config.toml`
      (look-and-feel only — **no key**), `launcher.exe`, and the `.cartridge` marker at the
      volume root.
- [ ] Failure / cancel handling — don't leave a half-written cartridge behind.

## Listener install (job 2)

- [ ] Copy `listener.exe` to `C:\Program Files\GaCaSy\`.
- [ ] Write the listener's `config.toml` beside it — **appending** to its `keys` list when
      one already exists, so an earlier cartridge stays paired.
- [ ] **Windows:** register autostart at login (`HKCU\…\Run` entry pointing at the installed
      exe) — the Windows listener is a resident process.
- [ ] **Linux (future):** no autostart at all — install
      `/etc/udev/rules.d/99-gacasy.rules` and run `udevadm control --reload`. The Linux
      listener is one-shot and udev starts it per connection; there is no service to enable.
- [ ] Detect an existing install; offer repair/replace.
- [ ] Uninstall — remove the folder *and* undo the activation: the `Run` entry on Windows,
      the rule file plus a udev reload on Linux.

## Cartridge editing (job 3)

- [ ] Load an existing cartridge's `config.toml` + `catalog.json`.
- [ ] Add games (reuses the job 1 flow, appending to the catalog).
- [ ] Remove games — delete the folder and image, drop the catalog entry.
- [ ] Change the key — rewrite `.cartridge` (nothing else on the cartridge holds it), and
      warn that this un-pairs the cartridge from any listener that doesn't know the new key.

## Design work to settle

- [ ] Decide what a duplicate game folder does — reject, rename, or overwrite.

## Future

- [ ] Linux target: `/opt` or `~/.local/share` instead of Program Files, a **udev rule**
      instead of a Run key (not a systemd user service — nothing runs between connections),
      Linux launcher binary instead of `launcher.exe`.
