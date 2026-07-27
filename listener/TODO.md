# GaCaSy Listener — TODO

Actionable list for the service specced in [`structure.md`](structure.md).
**The shared core and the Windows trigger are built; Linux is not started.**

The two platforms differ in **process lifetime**, not just in which API detects a volume —
Windows stays resident from login to logout, Linux runs one-shot from udev and exits. So the
build splits into one shared core plus two unrelated triggers. See
[Execution models](structure.md#execution-models) before starting the Linux trigger.

## Shared core — done

OS-agnostic, built once, called identically by both triggers —
`volume::handle_volume(root, config, log) -> Outcome`. Nothing here may be reimplemented per
platform.

- [x] Rust codebase scaffold, `cfg`-gated per OS — one crate, two trigger modules.
- [x] Parse the `.cartridge` marker (TOML: `version`, `key`, `launcher`) —
      [`src/marker.rs`](src/marker.rs).
- [x] Load this listener's own `config.toml` — the `keys` list, log location, debounce
      window — [`src/config.rs`](src/config.rs).
- [x] Trust check: the marker's key is present in `keys` (case- and whitespace-insensitive),
      with an **empty `keys` list trusting every cartridge** so a fresh install needs no
      pairing step.
- [x] `output/` deployment folder — exe, config and log together, refreshed by `cargo run`
      the way the launcher's is.
- [x] Auto-launch the binary named by the marker's `launcher`, resolved from the volume root
      (no hardcoded `launcher.exe`), refusing any path that escapes the volume.
- [x] Log ignored volumes with the reason (no marker / unknown key / bad TOML / bad version /
      launcher missing or outside the volume) — [`src/log.rs`](src/log.rs). The log lives
      beside the exe and its config — `%LOCALAPPDATA%\GaCaSy\` for an installed listener —
      falling back to that same folder if the exe was dropped somewhere read-only.
- [x] Behave correctly when started by hand, plus `--check <path>` to run the core against one
      volume and exit. Registering the login entry (Windows) and installing the udev rule
      (Linux) remain the **installer's** job.

## Windows trigger — done

[`src/trigger/windows.rs`](src/trigger/windows.rs).

- [x] Hidden **top-level** window. **Not** message-only (`HWND_MESSAGE`): broadcast
      `WM_DEVICECHANGE` reaches top-level windows only, so a message-only window silently
      receives nothing.
- [x] `WM_DEVICECHANGE` + `DBT_DEVICEARRIVAL` + `DEV_BROADCAST_VOLUME`; decode
      `dbcv_unitmask` as a **bitmask** — one event can carry several drive letters.
- [x] Message loop blocking in `GetMessage`. **No polling fallback** — the 0% idle CPU is the
      whole justification for staying resident.
- [x] Startup sweep: enumerate mounted volumes once at launch, so a cartridge plugged in
      before login is still picked up.
- [x] Named-mutex single-instance guard (same pattern as the launcher, its own name) — the
      `Run` entry can fire twice across a fast logoff/logon.
- [x] `#![windows_subsystem = "windows"]` — no console window.
- [x] Drive-type filter (`DRIVE_FIXED` / `DRIVE_REMOVABLE`) and `DBTF_NET` rejection, so
      network and virtual volumes are dropped before any file access.
- [x] `SetErrorMode(SEM_FAILCRITICALERRORS)`, so sweeping an empty card reader can't pop a
      modal "no disk in the drive" box from a process with no window.
- [x] Per-drive-letter arrival debounce (`debounce_seconds`, default 5).
- [x] Verify idle cost — measured on the release build: **1.2 MB private bytes, 7.7 MB
      working set, 0 ms CPU** after startup with nothing plugged in.

### Still worth doing on Windows

- [ ] End-to-end test against a **real** removable volume. Verified so far: the core (via
      `--check`), the startup sweep over real drives, and that the window receives and decodes
      genuine `DEV_BROADCAST_VOLUME` payloads. Not yet exercised on hardware: a
      non-network arrival running all the way through to a launch. (`subst` won't do it —
      those arrive flagged `DBTF_NET` and are filtered by design.)
- [ ] Decide whether `WM_QUERYENDSESSION` / `WM_ENDSESSION` need handling, or whether being
      killed at logout is fine (it currently is — nothing needs flushing).

## Linux trigger — not started

Placeholder at [`src/trigger/linux.rs`](src/trigger/linux.rs); the crate compiles on Linux and
`--check <mountpoint>` already exercises the shared core there.

- [ ] udev rule: `ACTION=="add"`, `SUBSYSTEM=="block"`, `ENV{ID_FS_USAGE}=="filesystem"`.
- [ ] `RUN+="… systemd-run --no-block …"` handoff — udev kills `RUN+=` children
      unconditionally when the event finishes, so it cannot run the work itself.
- [ ] Bounded wait for the mountpoint: udev fires on **device add**, before udisks2 mounts
      the filesystem. Give up cleanly on timeout.
- [ ] Resolve the active graphical session via logind (`loginctl`) and start the core with
      `systemd-run --uid=<user> --setenv=…` (`DISPLAY` / `WAYLAND_DISPLAY` /
      `DBUS_SESSION_BUS_ADDRESS`) — udev runs as root with no session.
- [ ] Decide and implement the headless case (no active session → log and exit).
- [ ] Verify nothing stays resident once the launcher is running.
- [ ] Decide the Linux equivalent of the drive-type filter — the Windows build drops network
      and virtual volumes before touching them, and `/media/...` vs an SMB mount needs the
      same call.

## v2 — signature-based trust

- [ ] Verify the code-signed `launcher.exe`'s signature instead of matching a key.
- [ ] Drop `.cartridge` handling entirely once that lands; the marker is retired, not kept
      as a fallback.
