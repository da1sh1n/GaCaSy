# GaCaSy Listener — TODO

Actionable list for the service specced in [`structure.md`](structure.md). Nothing is built
yet — this folder holds documentation only.

The two platforms differ in **process lifetime**, not just in which API detects a volume —
Windows stays resident from login to logout, Linux runs one-shot from udev and exits. So the
build splits into one shared core plus two unrelated triggers. See
[Execution models](structure.md#execution-models) before starting either trigger.

## Shared core

OS-agnostic, built once, called identically by both triggers —
`handle_volume(root) -> Outcome`. Nothing here may be reimplemented per platform.

- [ ] Rust codebase scaffold, `cfg`-gated per OS — one crate, two trigger modules.
- [ ] Parse the `.cartridge` marker (TOML: `version`, `key`, `launcher`).
- [ ] Load this listener's own `config.toml` — the `keys` list, log location, behaviour
      switches.
- [ ] Trust check: the marker's key is present in `keys`.
- [ ] Auto-launch the binary named by the marker's `launcher`, resolved from the volume root
      (no hardcoded `launcher.exe`).
- [ ] Log ignored volumes with the reason (no marker / unknown key / bad TOML).
- [ ] Behave correctly when started by hand — registering the login entry (Windows) and
      installing the udev rule (Linux) are the **installer's** job, not this program's.

## Windows trigger — resident

- [ ] Hidden **top-level** window. **Not** message-only (`HWND_MESSAGE`): broadcast
      `WM_DEVICECHANGE` reaches top-level windows only, so a message-only window silently
      receives nothing.
- [ ] `WM_DEVICECHANGE` + `DBT_DEVICEARRIVAL` + `DEV_BROADCAST_VOLUME`; decode
      `dbcv_unitmask` as a **bitmask** — one event can carry several drive letters.
- [ ] Message loop blocking in `GetMessage`. **No polling fallback** — the 0% idle CPU is the
      whole justification for staying resident.
- [ ] Startup sweep: enumerate mounted volumes once at launch, so a cartridge plugged in
      before login is still picked up.
- [ ] Named-mutex single-instance guard (same pattern as the launcher, its own name) — the
      `Run` entry can fire twice across a fast logoff/logon.
- [ ] `#![windows_subsystem = "windows"]` — no console window.
- [ ] Verify idle cost: 0% CPU and a few MB working set with nothing plugged in.

## Linux trigger — one-shot

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

## v2 — signature-based trust

- [ ] Verify the code-signed `launcher.exe`'s signature instead of matching a key.
- [ ] Drop `.cartridge` handling entirely once that lands; the marker is retired, not kept
      as a fallback.
