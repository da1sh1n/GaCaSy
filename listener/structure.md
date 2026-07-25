# GaCaSy Listener — PC Side (spec to build)

Part of the three-app **GaCaSy** game-cartridge system. This document covers the
**listener**: the PC-side component that detects a connected cartridge, verifies it, and
auto-starts that cartridge's launcher. The cartridge-side
companion is documented in [`../launcher/structure.md`](../launcher/structure.md) and the
setup tool that installs this service in
[`../installer/structure.md`](../installer/structure.md); the user-facing overview is in
[`../README.md`](../README.md).

> **This side does not exist yet.** Treat this doc as a *spec to build by*.

## Purpose

Something on the user's PC notices a cartridge being connected and, once it trusts it,
launches that cartridge's launcher automatically — so plugging in a cartridge "just works"
like slotting one into a console.

"Something" is deliberately vague here: on Windows it is a resident background process, on
Linux it is a program that does not exist until udev starts it. Both satisfy this purpose;
see [Execution models](#execution-models).

## Shape

- **One Rust codebase**, `cfg`-gated per OS, producing a **Windows build** and a
  **Linux build**. (Matches the launcher's language; single repo, two targets.)
- What `cfg` gates is **the trigger and the process lifetime**, not just which detection API
  gets called. The two platforms are genuinely different programs at that level — Windows
  stays resident from login to logout, Linux runs only when something is plugged in and then
  exits. See [Execution models](#execution-models); it is the most important thing in this
  document.
- Everything downstream of "a volume showed up" is **shared, OS-agnostic code** — one
  implementation of the trust check and the launch, called by both triggers.

## What counts as a "cartridge"

**Any mounted storage volume.** The reference cartridge is an NVMe behind a USB-C
adapter, but the listener must treat every storage device the same — HDD, SSD, NVMe,
USB stick. So detection is **"a new volume mounted"**, never a specific USB VID/PID.

## Responsibilities / flow

Steps 2–5 are the **shared core** — identical on both platforms. Only step 1 differs, and it
differs a lot; see [Execution models](#execution-models).

1. A **volume becomes available** — however this platform learns about that.
2. Look for a **`.cartridge`** file at the volume root.
3. If present, read its **key** and check it against **this listener's own trusted key
   list** in its `config.toml` (see
   [Cartridge identification system](#cartridge-identification-system)).
4. If the key is trusted, **launch that volume's launcher** — the binary the marker's
   `launcher` field names, resolved relative to the volume root.
5. Otherwise, ignore the volume (optionally log why).

## Execution models

The two builds are **not** the same program with a different detection call. They have
different process lifetimes, and that difference is deliberate rather than incidental:

| | Linux | Windows |
| --- | --- | --- |
| **Trigger** | udev device-add event | `WM_DEVICECHANGE` / `DBT_DEVICEARRIVAL` |
| **Process lifetime** | one-shot — runs, acts, exits | resident, login → logout |
| **Idle cost** | nothing is running | ~1–3 MB working set, 0% CPU |
| **Started by** | udev, once per event | the installer's `Run` entry, at login |
| **What stays resident** | `systemd-udevd`, already there | the listener itself |

```text
Linux    (nothing running) ──udev add──▶ listener ──▶ verify ──▶ launch ──▶ exit

Windows  login ──▶ listener ─────────────────────────────────────────────▶ logout
                      └──WM_DEVICECHANGE──▶ verify ──▶ launch ──┘  (stays alive)
```

The honest framing is that **neither platform is free** — the question is only *which
already-resident host does the waiting*. Linux delegates it to `systemd-udevd`, which is
running regardless. Windows has no equivalent host worth delegating to (see
[Why not one-shot on Windows too](#why-not-one-shot-on-windows-too)), so the listener does its
own waiting — cheaply.

### Shared core

The asymmetry stops at the trigger. Both platforms call one OS-agnostic entry point, so the
trust logic exists exactly once:

```text
handle_volume(root: &Path) -> Outcome
  read <root>/.cartridge  →  parse TOML  →  key in config keys?  →  spawn <root>/<launcher>
```

That is steps 2–5 of [Responsibilities / flow](#responsibilities--flow) in full. **Do not
reimplement any of it per platform** — a Windows-only bug in the key check is exactly the
failure this split is meant to prevent.

### Windows — resident, event-driven

A hidden window and a message loop. It costs a few megabytes and nothing else.

- **A hidden *top-level* window — not a message-only (`HWND_MESSAGE`) window.** This is the
  trap worth writing down: broadcast `WM_DEVICECHANGE` volume notifications are delivered to
  top-level windows only, so a message-only window compiles, runs, and silently receives
  nothing forever.
- On `WM_DEVICECHANGE` with `DBT_DEVICEARRIVAL` and a `DEV_BROADCAST_VOLUME` payload, decode
  `dbcv_unitmask` — a **bitmask of drive letters**, not a single one, since one event can carry
  several — and call the shared core once per resulting root.
- **No polling loop, ever.** The process blocks in `GetMessage`, which is a true kernel wait.
  That is what makes the idle cost 0% CPU, and it is the entire justification for the resident
  model. A drive-letter polling fallback would forfeit it and must not be added quietly.
- **Startup sweep.** A cartridge plugged in *before* login never produces an arrival event, so
  enumerate mounted volumes once at startup and run each through the same core. Without this,
  "it only works if you plug it in after logging in" — a bug that looks like flakiness.
- **Single-instance guard** — reuse the named-mutex pattern already proven in
  [`../launcher/src/main.rs`](../launcher/src/main.rs) (`Local\GaCaSy.CartridgeLauncher`, via
  `windows-sys`) under its own name. The `Run` entry can fire twice across a fast
  logoff/logon, and two listeners racing to launch the same cartridge means two launchers.
- `#![windows_subsystem = "windows"]` — no console window, same as the launcher.

### Linux — reactive, one-shot

Nothing runs between connections. udev fires the listener, it verifies, launches, and exits.

- **Rule shape** — `ACTION=="add"`, `SUBSYSTEM=="block"`, `ENV{ID_FS_USAGE}=="filesystem"`,
  with `RUN+="… systemd-run --no-block …"` handing off to a transient unit.
- **Why udev doesn't run the listener directly.** udev spawns `RUN+=` as a short-lived
  foreground task and **unconditionally kills the process once event handling finishes** —
  detached or not. So it can neither wait for the mount nor be the parent of a long-lived GUI
  launcher. `--no-block` also keeps udev's event queue from being held open while we work.
- **Mount timing.** udev fires on **device add**, not on mount — the filesystem is mounted
  moments later by udisks2 or the desktop's automounter, if at all. At `RUN+=` time there is
  usually **no mountpoint yet**, so the transient unit waits (bounded, then gives up) for one
  to appear before calling the core. Skipping this is the single most likely way to build a
  Linux listener that "works when I test it by hand" and fails on a real plug-in.
- **Session handoff.** udev runs as **root with no session** — no `DISPLAY`,
  `WAYLAND_DISPLAY`, no `DBUS_SESSION_BUS_ADDRESS`. The launcher is a GUI app, so it has to
  land in the logged-in user's graphical session: resolve the active seat's session and user
  via logind (`loginctl`), then `systemd-run --uid=<user> --setenv=…`. Launching as root would
  either fail to reach a display or put a root-owned window on the user's desktop.
- **Nothing to autostart.** There is no login entry, no daemon, and no systemd *user service*
  on this side. The installer's entire Linux job is dropping the rule file and reloading udev.

### Why not one-shot on Windows too

Windows **can** be made event-triggered — the resident model is a choice, not a limitation, so
here is the reasoning rather than a re-argument later:

- **Task Scheduler "On an event" trigger** — the closest true analogue to udev, genuinely
  resident-free. Rejected because it keys off event-log channels
  (`Microsoft-Windows-Partition/Diagnostic`, Kernel-PnP) that are **disabled by default on some
  systems**, fire on *detach* as well as attach, and carry no drive letter — so the volume set
  has to be re-scanned anyway. Plus seconds of latency, which reads as "it didn't work".
- **WMI permanent event consumer** (`__EventFilter` + `CommandLineEventConsumer`) —
  technically ideal and hosted by a service that is already running. Rejected on
  reputation: it is MITRE **T1546.003**, a textbook malware persistence pattern, and would get
  the installer flagged by antivirus and EDR. Not a fight worth having for a games tool.
- **AutoPlay handler registration** — the blessed shell mechanism and properly one-shot, but it
  needs a one-time "always do this" from the user before it is automatic, and group policy can
  disable AutoPlay outright.

**Conclusion.** The resident pump costs ~1–3 MB and 0% CPU, needs no admin at runtime, has no
event-channel dependencies, no AV false positives and no latency. Every one-shot alternative
trades that for fragility, so Windows stays resident and the asymmetry is accepted.

## Cartridge identification system

The **cartridge ↔ PC** contract, defined here and referenced by the other two docs. It has
exactly two halves:

- **On the cartridge** — a `.cartridge` marker at the volume root, holding that cartridge's
  key.
- **On the PC** — this listener's `config.toml`, holding the list of keys it trusts.

A cartridge is trusted when its key appears in that list. The **launcher plays no part**: it
carries no key and never writes the marker (the installer writes it at setup time), so it is
purely the thing that gets started.

```text
volume shows up  →  has .cartridge?  →  key in listener's keys?  →  run <volume>/<launcher>
                          │ no                │ no
                          └── ignore ─────────┘
```

### The `.cartridge` marker

TOML, at the volume root, written by the installer:

```toml
# GaCaSy cartridge marker
version = 1
key = "3f9a1c…"
launcher = "launcher.exe"
```

- `version` — format version, so a later change has something to migrate from.
- `key` — this cartridge's identity.
- `launcher` — the binary to start, relative to the volume root. The listener reads it
  rather than hardcoding a name, which is what lets a Linux cartridge name a different
  binary without any listener change.

### Listener configuration

The listener's own `config.toml`, written beside its exe by the installer:

```toml
keys = ["3f9a1c…", "b72e04…"]
```

- `keys` — every cartridge key this PC trusts. A **list**, so pairing a second cartridge
  appends rather than un-pairing the first. The installer appends; the user can hand-edit.
- Also worth carrying here: a log file location, and any behaviour switches (e.g. whether to
  launch silently or notify).

**Version note.** This shared-key scheme is **v1** — it proves "this looks like a GaCaSy
cartridge I know about", but it is not a strong security boundary: anyone who can read a
cartridge can copy its key. It is a recognition handshake, not tamper protection.

## Open questions

- **Headless / no active session on Linux.** If logind reports no graphical session, there is
  nowhere to put a launcher window. Log and do nothing, presumably — but confirm that rather
  than letting `systemd-run` fail obscurely.
- **Several volumes at once.** A multi-partition device produces several arrivals (and one
  Windows event can carry several drive letters in its bitmask). Serialise them, or handle
  each independently? Two trusted cartridges plugged in together would launch two launchers.
- **Re-arrival debounce.** A flaky USB link can produce repeated add events for one device.
  How long is the window in which a second arrival for the same volume is ignored?
- **Network and virtual volumes.** "Any mounted storage volume" taken literally includes SMB
  shares, loopback mounts, and VM shared folders. Filter them out, or let the missing
  `.cartridge` do that job implicitly?

## Future

**v2 — signature-based trust.** Once `launcher.exe` is officially **code-signed**, the
listener verifies the exe's signature instead of matching a shared secret. That **replaces**
the key check rather than augmenting it, and `.cartridge` becomes unnecessary — the listener
looks for the launcher binary on the volume and checks its signature. Trust becomes
cryptographic, and there is no secret on the cartridge to copy.

## Status / roadmap

Grouped the way the code should be: one shared core, two triggers.

**Shared core** — OS-agnostic, built once:

- [ ] Rust codebase scaffold, `cfg`-gated per OS.
- [ ] Parse the `.cartridge` marker and this listener's `config.toml` key list.
- [ ] Trust check: marker key present in `keys`.
- [ ] Auto-launch the marker's `launcher` binary from the volume root.
- [ ] Log ignored volumes with the reason.

**Windows trigger** — resident:

- [ ] Hidden **top-level** window (not message-only) and a `GetMessage` loop, no polling.
- [ ] `WM_DEVICECHANGE` / `DBT_DEVICEARRIVAL`, decoding the `dbcv_unitmask` bitmask.
- [ ] Startup sweep for volumes already connected before login.
- [ ] Named-mutex single-instance guard.

**Linux trigger** — one-shot:

- [ ] udev rule and the `systemd-run --no-block` handoff.
- [ ] Bounded wait for the mountpoint to appear before calling the core.
- [ ] logind session lookup and environment handoff into the user's graphical session.
- [ ] Confirm nothing stays resident once the launcher is running.

**Both:**

- [ ] Behave correctly when started by hand. Registering the Windows login entry and
      installing the Linux udev rule are the **installer's** job — see
      [`../installer/structure.md`](../installer/structure.md).
- [ ] v2: verify the code-signed launcher's signature and drop `.cartridge` handling.
