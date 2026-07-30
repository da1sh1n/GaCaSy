# GaCaSy Installer — TODO

The installer specced in [`structure.md`](structure.md) is **built**. Everything under
"Foundation", "Cartridge creation", "Listener install" and "Cartridge editing" below is done;
what is left is verification on real hardware, and the polish items after it.

## Done

- [x] Root `Cargo.toml` workspace tying `launcher` + `listener` + `installer` together.
      `default-members` leaves the installer out so the payload build order happens by
      default: `cargo build --release`, then `cargo build --release -p installer`.
- [x] `installer` crate scaffold — `eframe`/`egui`, `#![windows_subsystem = "windows"]`.
      **No UAC manifest and no elevation path at all**: the listener lives in
      `%LOCALAPPDATA%\GaCaSy` with its config and log, and a cartridge goes on a drive the
      user can already write to. See structure.md, "Elevation".
- [x] Payload embedded via `include_bytes!` out of `OUT_DIR`, staged by `build.rs`:
      `launcher.exe`, `listener.exe`, and three seed files (the cartridge's `config.toml` and
      `catalog.json`, plus the listener's own `config.toml`).
- [x] Build fails with the exact command to run when a payload artifact is missing.
      `GACASY_PAYLOAD_OPTIONAL=1` gives a UI-only build that refuses to install and says so.
- [x] Wizard shell: step navigation, back, cancel, one error surface.
- [x] Volume enumeration; routing on the launcher's presence (create vs edit).
- [x] **External drives only.** The system drive is vetoed unconditionally (`%SystemRoot%` /
      `%windir%` / `%SystemDrive%`, any match wins) and everything else is judged on its disk's
      bus type via `IOCTL_STORAGE_QUERY_PROPERTY` — USB / FireWire / SD / MMC in, SATA / NVMe /
      RAID out. Refused drives stay listed with the reason. Re-checked in `choose_volume` and
      again in `plan()`, so a list that goes stale under a click can't be written to.
- [x] Key screen — 32-hex generated from OS entropy or a typed one, validated.
- [x] Game folder picker, recursive exe scan, reject-list, scoring, preselect only a clear
      winner, manual override always available.
- [x] Per-game cover picker; 2:3 warning from a header parser covering PNG / WebP (incl.
      animated `VP8X`) / JPEG / GIF, by content rather than by file extension.
- [x] Editable game name, defaulting to the folder name.
- [x] Free-space precheck (measured bytes + launcher + 256 MB headroom).
- [x] Threaded, chunked `games/` copy with a progress bar and a cancel that responds within
      one chunk — not `fs::copy`, which is uninterruptible on a 40 GB pak file.
- [x] Writes `images/`, `catalog.json`, `config.toml` (seeded only when absent),
      `launcher.exe`, `.cartridge`.
- [x] Failure / cancel rollback: only what this run created is removed, and the catalog is
      written last so a failure leaves a cartridge older than intended, never one listing
      games it doesn't have.
- [x] Listener install into `%LOCALAPPDATA%\GaCaSy`: copy, config `keys` **append** preserving
      every comment, `HKCU\…\Run` entry, starts it immediately, stops a running one first
      (matched on full image path).
- [x] Detect existing installs; repair/replace; uninstall that undoes the `Run` entry too.
- [x] Fold in an install left by an earlier build in `%LOCALAPPDATA%\Programs\GaCaSy` or
      `%ProgramFiles%\GaCaSy` — keys carried over first, then stopped, un-registered, deleted.
- [x] Edit mode: add games, remove games, change key with the un-pairing warning.
- [x] Duplicate game folders — settled as **reject**; see structure.md, "Settled questions".

## Next

- [x] **End-to-end run on real media.** Write a cartridge to an actual drive, install the
      listener, unplug and replug, confirm the launcher comes up.
- [x] Verify each screen visually at the user's real display scale. Only the home screen has
      been seen rendered; the rest use the same widgets but have not been looked at.
- [x] Decide whether a written cartridge should be verified after the copy (re-read the exe
      and cover of each new game) or whether the copy's own error handling is enough.

## Future

- [ ] Linux target: `/opt` or `~/.local/share` instead of Program Files, a **udev rule**
      instead of a Run key (not a systemd user service — nothing runs between connections),
      Linux launcher binary instead of `launcher.exe`. `volume.rs` and `listener.rs` are the
      two modules with `#[cfg(windows)]` platform halves; everything else is portable
      already.
