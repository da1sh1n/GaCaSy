# GaCaSy Listener — PC Side (spec to build)

Part of the three-app **GaCaSy** game-cartridge system. This document covers the
**listener**: the PC-side component that detects a connected cartridge, verifies it, and
auto-starts that cartridge's launcher. The cartridge-side
companion is documented in [`../launcher/structure.md`](../launcher/structure.md) and the
setup tool that installs this service in
[`../installer/structure.md`](../installer/structure.md); the user-facing overview is in
[`../README.md`](../README.md).

> **Build status.** The shared core and the Windows trigger exist —
> [`src/`](src/), with [`TODO.md`](TODO.md) tracking what is ticked off. The Linux trigger
> does not; for that half this doc is still a *spec to build by*.

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
- `#![windows_subsystem = "windows"]`, one dependency (`toml`) plus `windows-sys` on Windows
  only. Nothing here needs a UI framework.

### Deployed layout

The listener keeps its files together in one folder — the same shape as the launcher's
`output/`, minus the content it has no use for:

```text
output/
  listener.exe   <- the program
  config.toml    <- the keys this PC trusts; seeded from src/config.toml if missing
  listener.log   <- what it did, and why it ignored what it ignored
```

That folder is simply **wherever the exe is**. Installed, that is
**`%LOCALAPPDATA%\GaCaSy\`** and nowhere else — the installer has one location and no
elevated path to any other, chosen precisely so these three files are always together and
always writable ([`../installer/structure.md`](../installer/structure.md#elevation)). It can
also be `output/`, or anywhere the exe was dropped by hand. The single exception is a
`cargo run` build, whose exe lives under `target/`: that resolves to the repo's `output/`
instead, and refreshes `output/listener.exe` so the shippable copy tracks the source.
`config.toml` is never overwritten once present, so an edited key list survives every build.

The refresh is done by the exe that is *running*, so `cargo build --release` deploys
nothing — it never runs anything. `cargo run --release -- --check .` deploys the release
build and exits.

The "am I a dev build?" test is **"is the exe inside this crate's `target/`?"**, deliberately
not the launcher's "is my parent folder named `output`?". The latter misreads an installed
`…\AppData\Local\GaCaSy\listener.exe` as a dev build, because that parent isn't named `output`
either — the bug noted against `running_deployed()` in
[`../launcher/src/content.rs`](../launcher/src/content.rs).

### Source layout

The folder split *is* the shape above: `trigger/` is the only `cfg`-gated part, and nothing
about markers, keys or launching lives inside it.

```text
listener/
  Cargo.toml
  src/
    main.rs        <- entry point, config load, `--check` handling
    volume.rs      <- THE SHARED CORE: handle_volume(root, config, log)
    marker.rs      <- reading the cartridge's .cartridge file
    config.rs      <- reading this listener's own config.toml
    log.rs         <- the activity log
    config.toml    <- seed config, embedded via include_str!
    trigger/
      mod.rs       <- cfg selects one of the two below
      windows.rs   <- resident: hidden top-level window + GetMessage loop
      linux.rs     <- one-shot: udev handoff — placeholder, not built
  output/          <- the deployed listener (see above)
```

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
| **Idle cost** | nothing is running | 1.2 MB private / 7.7 MB working set, 0% CPU¹ |
| **Started by** | udev, once per event | the installer's `Run` entry, at login |
| **What stays resident** | `systemd-udevd`, already there | the listener itself |

```text
Linux    (nothing running) ──udev add──▶ listener ──▶ verify ──▶ launch ──▶ exit

Windows  login ──▶ listener ─────────────────────────────────────────────▶ logout
                      └──WM_DEVICECHANGE──▶ verify ──▶ launch ──┘  (stays alive)
```

¹ Measured on the release build, idle with nothing plugged in. The two memory figures are
both worth quoting: **private bytes** (1.2 MB) is what this process actually costs the
machine, while the larger **working set** is mostly shared system DLLs that are resident
anyway. CPU time after startup is zero — not "low", zero, because `GetMessage` is a kernel
wait rather than a loop.

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
  [`../launcher/src/instance.rs`](../launcher/src/instance.rs) (`Local\GaCaSy.CartridgeLauncher`, via
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

- `version` — format version, so a later change has something to migrate from. A marker
  declaring a version this build doesn't know is **refused**, not guessed at: every other
  field could mean something else in a later format.
- `key` — this cartridge's identity.
- `launcher` — the binary to start, relative to the volume root. The listener reads it
  rather than hardcoding a name, which is what lets a Linux cartridge name a different
  binary without any listener change.

Unlike the listener's own config.toml, the marker is **not** parsed forgivingly — a missing
key or launcher describes no cartridge worth starting, so it fails as a whole and the volume
is ignored with a logged reason.

The `launcher` path must stay **inside the volume**: no absolute paths, no drive prefix, no
`..`. A marker is trusted to name a binary *on its own cartridge* and no further, otherwise
"plug in a disk" quietly becomes "run an arbitrary program already on this PC".

### Listener configuration

The listener's own `config.toml`, written beside its exe by the installer:

```toml
keys = ["3f9a1c…", "b72e04…"]
# log_file = "listener.log"
debounce_seconds = 5
```

- `keys` — every cartridge key this PC trusts. A **list**, so pairing a second cartridge
  appends rather than un-pairing the first. The installer appends; the user can hand-edit.
  Compared case- and whitespace-insensitively, because this key gets copied between two files
  by hand often enough that `3F9A` failing to match `3f9a` would only ever be a support
  burden — it is a recognition handshake, not a secret (see the version note below).
- `log_file` — where to append activity. Defaults to `listener.log` **beside the exe and the
  config**, so all three of the listener's files sit in one folder you can open — and for an
  installed listener that folder is `%LOCALAPPDATA%\GaCaSy\`, which it can always write. An
  empty string disables logging. A copy dropped by hand somewhere read-only falls back to
  `%LOCALAPPDATA%\GaCaSy\listener.log` rather than going silent; that is the same folder an
  install uses, so there is only ever one place to look for this file.
- `debounce_seconds` — how long to ignore repeat arrivals for a drive letter just handled.

### An empty `keys` list trusts everything

**`keys = []` accepts any cartridge**, rather than none. The unpaired default is *open*, not
*locked*: a fresh install works the moment a volume carries a valid `.cartridge`, with no
pairing step. Listing even one key switches the listener to matching that list and nothing
else.

Worth being clear about what open means: any volume with a well-formed marker will have the
binary that marker names started. The marker is then the only thing between plugging a disk
in and running something off it — which is a reasonable v1 posture for a tool whose trust
model is already "a shared secret anyone who can read the cartridge can copy", but it is a
real choice, not an oversight.

One consequence shows up in the parsing: blank entries in `keys` are **kept**, not filtered
out. They can never match anything, but dropping them would turn `keys = [""]` into an empty
list — silently promoting a locked-down config to an open one over a stray blank string.

The file is parsed **forgivingly, key by key**, the same way the launcher reads its own config:
one wrong-typed value costs that setting only. Rejecting the whole file over a typo would
silently un-pair every cartridge on the PC, which is a much worse failure than one setting
falling back to its default.

Reading the log is the only way to see what the listener did — it has no console and no
visible window. Every volume it looks at produces a line, including the ignored ones and why.

**Version note.** This shared-key scheme is **v1** — it proves "this looks like a GaCaSy
cartridge I know about", but it is not a strong security boundary: anyone who can read a
cartridge can copy its key. It is a recognition handshake, not tamper protection.

## Open questions

Three of these are now settled by the Windows build. The answers are recorded here rather
than deleted, because the Linux trigger has to make the same calls and should make them the
same way.

- **Several volumes at once.** *Settled: handled independently, in bitmask order.* One event
  carrying several letters runs the core once per letter, on the message thread, one after
  another — so they are serialised in practice, but nothing coordinates them. Two trusted
  cartridges plugged in together therefore do launch two launchers, which is the honest
  reading of what the user asked for. Deduplicating that is the launcher's business, not
  this component's — but note that its mutex does **not** currently cover the cartridge
  case: `running_deployed()` in
  [`../launcher/src/content.rs`](../launcher/src/content.rs) is what `main.rs` arms the
  guard on, and it is true only when the exe's parent folder is named `output` — on a real
  cartridge the exe sits at the volume root.
  So nothing dedupes today. Left as a launcher-side issue rather than worked around here.
- **Re-arrival debounce.** *Settled: `debounce_seconds`, default 5, keyed on drive letter.*
  Long enough to swallow the repeat events a flaky USB link produces, short enough that
  deliberately re-plugging a cartridge still works. Keyed on the letter rather than the
  marker, since keying on the marker means reading the volume before deciding to skip it —
  most of the work the debounce exists to avoid. The consequence is that swapping a
  *different* cartridge into the same letter within the window is also skipped; at five
  seconds that is not a real sequence.
- **Network and virtual volumes.** *Settled: filtered out explicitly, not left to the missing
  `.cartridge`.* On Windows that means `DRIVE_FIXED` / `DRIVE_REMOVABLE` only, plus dropping
  any arrival flagged `DBTF_NET`. The reason to be explicit is timing, not tidiness: reaching
  for a file on a stale network mount can block for a long time, and this runs on the message
  thread. **Linux needs the equivalent** — an SMB or VM-shared mount under `/media` must be
  rejected before it is touched.
- **Headless / no active session on Linux.** *Still open.* If logind reports no graphical
  session, there is nowhere to put a launcher window. Log and do nothing, presumably — but
  confirm that rather than letting `systemd-run` fail obscurely.

## Future

**v2 — signature-based trust.** Once `launcher.exe` is officially **code-signed**, the
listener verifies the exe's signature instead of matching a shared secret. That **replaces**
the key check rather than augmenting it, and `.cartridge` becomes unnecessary — the listener
looks for the launcher binary on the volume and checks its signature. Trust becomes
cryptographic, and there is no secret on the cartridge to copy.

## Status / roadmap

Grouped the way the code is: one shared core, two triggers. [`TODO.md`](TODO.md) carries the
same list in more detail.

**Shared core** — OS-agnostic, built once, in [`src/volume.rs`](src/volume.rs) and friends:

- [x] Rust codebase scaffold, `cfg`-gated per OS.
- [x] Parse the `.cartridge` marker and this listener's `config.toml` key list.
- [x] Trust check: marker key present in `keys`.
- [x] Auto-launch the marker's `launcher` binary from the volume root, refusing paths that
      escape it.
- [x] Log ignored volumes with the reason.

**Windows trigger** — resident, in [`src/trigger/windows.rs`](src/trigger/windows.rs):

- [x] Hidden **top-level** window (not message-only) and a `GetMessage` loop, no polling.
- [x] `WM_DEVICECHANGE` / `DBT_DEVICEARRIVAL`, decoding the `dbcv_unitmask` bitmask.
- [x] Startup sweep for volumes already connected before login.
- [x] Named-mutex single-instance guard.
- [x] Drive-type and `DBTF_NET` filtering, arrival debounce, and `SEM_FAILCRITICALERRORS`
      so an empty card reader can't pop a modal error box.
- [ ] End-to-end run against real removable hardware — see [`TODO.md`](TODO.md) for exactly
      which seam that leaves untested.

**Linux trigger** — one-shot, not started
([`src/trigger/linux.rs`](src/trigger/linux.rs) is a placeholder):

- [ ] udev rule and the `systemd-run --no-block` handoff.
- [ ] Bounded wait for the mountpoint to appear before calling the core.
- [ ] logind session lookup and environment handoff into the user's graphical session.
- [ ] Confirm nothing stays resident once the launcher is running.

**Both:**

- [x] Behave correctly when started by hand, plus `--check <path>` to run the core against a
      single volume and exit. Registering the Windows login entry and
      installing the Linux udev rule are the **installer's** job — see
      [`../installer/structure.md`](../installer/structure.md).
- [ ] v2: verify the code-signed launcher's signature and drop `.cartridge` handling.
