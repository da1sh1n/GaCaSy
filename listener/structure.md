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
- `#![windows_subsystem = "windows"]`, two workspace dependencies (`sigblock`, `trust`) plus
  `windows-sys` on Windows only. Nothing here needs a UI framework, and on a non-Windows
  build those two crates are the entire dependency tree — `toml` left with the config file.

### Deployed layout

The listener keeps its files together in one folder — the same shape as the launcher's
`output/`, minus the content it has no use for:

```text
output/
  listener.exe   <- the program
  listener.log   <- what it did, and why it ignored what it ignored
```

Two files, and it used to be three: a `config.toml` held the cartridge keys this PC trusted.
Trust is compiled in now, so there is nothing left for a config file to hold — see
[Trust](#trust) and [`src/settings.rs`](src/settings.rs).

That folder is simply **wherever the exe is**. Installed, that is
**`%LOCALAPPDATA%\GaCaSy\`** and nowhere else — the installer has one location and no
elevated path to any other, chosen precisely so these files are always together and
always writable ([`../installer/structure.md`](../installer/structure.md#elevation)). It can
also be `output/`, or anywhere the exe was dropped by hand. The single exception is a
`cargo run` build, whose exe lives under `target/`: that resolves to the repo's `output/`
instead, and refreshes `output/listener.exe` so the shippable copy tracks the source.

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
about verifying or launching lives inside it.

```text
listener/
  Cargo.toml
  build.rs         <- bakes keys/*.pub in as ANCHORS; no keys, no compile
  src/
    main.rs        <- entry point, folder resolution, argument handling
    volume.rs      <- THE SHARED CORE: handle_volume(root, log)
    trust.rs       <- which file to check, holding it still, and saying why not
    version.rs     <- x.y.z: its own, and a launcher's as its signature states it
    settings.rs    <- the fixed tunables and where the log goes
    alert.rs       <- the one thing it ever says out loud
    log.rs         <- the activity log
    trigger/
      mod.rs       <- cfg selects one of the two below
      windows.rs   <- resident: hidden top-level window + GetMessage loop
      linux.rs     <- one-shot: udev handoff — placeholder, not built
  output/          <- the deployed listener (see above)
```

The decision itself is not in this crate at all: [`../trust/`](../trust/) is a workspace crate
shared with the installer, which needs the same answer and used to reach a different one.
What `trust.rs` holds is the part that is about a *volume* — which file to look at, holding it
still, and saying why not in a way the log can be read back from.

## What counts as a "cartridge"

**Any mounted storage volume.** The reference cartridge is an NVMe behind a USB-C
adapter, but the listener must treat every storage device the same — HDD, SSD, NVMe,
USB stick. So detection is **"a new volume mounted"**, never a specific USB VID/PID.

## Responsibilities / flow

Steps 2–5 are the **shared core** — identical on both platforms. Only step 1 differs, and it
differs a lot; see [Execution models](#execution-models).

1. A **volume becomes available** — however this platform learns about that.
2. Look for a **`launcher.exe`** at the volume root, and open it so nothing else can write
   to or delete it while we decide.
3. **Verify its signature** against the keys compiled into this build, and require the
   signature to declare itself a *launcher* (see [Trust](#trust)).
4. **Read its version out of that same signature** — authenticated, so this costs nothing
   and asks the binary nothing.
5. If it verified, **launch it**, still holding the handle from step 2.
6. Otherwise, ignore the volume and log why.

The order is the security property, not a style choice: nothing is executed at any point
before the last step.

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
  read <root>/launcher.exe (locked)  →  verify signature + role  →  version from the
  signed comment  →  spawn it
```

That is steps 2–6 of [Responsibilities / flow](#responsibilities--flow) in full. **Do not
reimplement any of it per platform** — a Windows-only bug in the signature check is exactly
the failure this split is meant to prevent.

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

## Trust

The **cartridge ↔ PC** contract, in exactly one sentence:

> A cartridge is a volume with a `launcher.exe` at its root carrying a valid signature, from a
> key this listener was built to trust, declaring itself a launcher.

No marker file, no key stored on the cartridge, nothing on this PC to edit. There is nothing
to pair and no state to get into.

```text
volume shows up  →  launcher.exe at the root?  →  signature verifies, as a launcher?  →  run it
                              │ no                            │ no
                              └────── ignore, and log why ────┘
```

An earlier design had a `.cartridge` marker holding a shared key, matched against a key list
in a `config.toml` beside the listener. It is gone, and its own documentation admitted why:
anyone who could read a cartridge could copy its secret, so it proved recognition rather than
authenticity. The full reasoning and the migration are in [`../SIGNING.md`](../SIGNING.md).

### Four properties, and the reason each one is load-bearing

- **The file is read; the program is never asked.** A binary that reports its own
  trustworthiness proves nothing — a hostile one prints whatever makes it look legitimate —
  and to ask, you would first have to execute the very thing you are deciding whether to
  permit. Nothing here spawns anything, and nothing downstream does either: the launcher's
  version comes out of the signature too. By the time this listener runs a launcher, it has
  already asked every question it has.
- **The trust anchor is compiled in, not configured.** [`build.rs`](build.rs) reads
  `keys/*.pub` at build time and bakes them in as `ANCHORS`; a fresh clone with no `keys/`
  does not compile at all. Were the accepted keys a line in a file beside the exe, anything
  that could write that file could append its own key and get arbitrary code auto-run on
  every insert. Changing what a listener trusts means replacing the listener.
- **The signature declares a role.** All three GaCaSy programs are signed with the same key,
  so "signed by us" is not the same question as "is a launcher" — a genuine `installer.exe`
  renamed to `launcher.exe` is still genuinely signed. minisign authenticates a *trusted
  comment* alongside the payload, so the role written into it (`gacasy-launcher`) is as
  unforgeable as the file itself. [`../trust/`](../trust/) checks both, and is the one place
  that does for every program that needs to.
- **The file is held still.** Verifying bytes and then executing a *path* is two different
  files if anything can change the disk in between — and the disk was plugged in by a
  stranger. The file is opened once denying writes and deletes to everyone else, and that
  handle is held until the launcher has been started. It does not stop hostile USB
  *firmware*, which lies below the filesystem; it stops anything going through Windows.

### What this does not cover

The signature is over `launcher.exe`'s own bytes and only those. `catalog.json`,
`config.toml`, `assets/` and `games/` sit beside it on the same disk, written by whoever made
the cartridge — typically the installer, on a machine holding no signing key — and carry no
signature of their own. Nothing could sign them, so nothing does. The launcher therefore
treats that content as untrusted input regardless of how it was started.

The residual case this leaves open, stated plainly: someone who copies a genuine, publicly
available signed launcher onto a drive alongside their own catalog and games gets it
auto-started. The signature proves the launcher is genuine, not that the drive is. See
[`../SIGNING.md`](../SIGNING.md) §1.

### Why a volume is refused

Every refusal is logged with its reason, and they are deliberately different from each other:

| | |
| --- | --- |
| no `launcher.exe` at the root | the ordinary case — every USB stick anyone ever plugs in. Not a problem, and deliberately not alarming. |
| unreadable | there and could not be opened |
| unsigned | a self-built or stripped binary |
| malformed | a signature block that is not a minisign signature |
| untrusted | correctly signed, by a key this build does not accept. The interesting one, and the only refusal whose log line names the anchors. |
| wrong role | genuinely signed, and not as a launcher |

`listener.exe --signature` prints this build's own signature and the keys it trusts, which is
how to find out what a given copy will accept without plugging anything in.

### Settings

There are none to edit. The debounce window (5s, how long repeat arrivals for a drive letter
already handled are ignored) and the log path are compiled in — see
[`src/settings.rs`](src/settings.rs), whose module doc explains why a file that exists to be
found, read and left alone is a file worth deleting.

The log defaults to `listener.log` **beside the exe**, so the listener's two files sit in one
folder — for an installed listener that is `%LOCALAPPDATA%\GaCaSy\`, which it can always
write. A copy dropped by hand somewhere read-only falls back to
`%LOCALAPPDATA%\GaCaSy\listener.log` rather than going silent, so there is only ever one
place to look.

Reading the log is the only way to see what the listener did — it has no console and no
visible window. Every volume it looks at produces a line, including the ignored ones and why.

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
- **Re-arrival debounce.** *Settled: `settings::DEBOUNCE_SECONDS`, 5, keyed on drive letter.*
  Long enough to swallow the repeat events a flaky USB link produces, short enough that
  deliberately re-plugging a cartridge still works. Keyed on the letter rather than on the
  volume's contents, since deciding by contents means reading the volume before deciding to
  skip it — most of the work the debounce exists to avoid. The consequence is that swapping a
  *different* cartridge into the same letter within the window is also skipped; at five
  seconds that is not a real sequence.
- **Network and virtual volumes.** *Settled: filtered out explicitly, not left to the absent
  launcher.* On Windows that means `DRIVE_FIXED` / `DRIVE_REMOVABLE` only, plus dropping
  any arrival flagged `DBTF_NET`. The reason to be explicit is timing, not tidiness: reaching
  for a file on a stale network mount can block for a long time, and this runs on the message
  thread. **Linux needs the equivalent** — an SMB or VM-shared mount under `/media` must be
  rejected before it is touched.
- **Headless / no active session on Linux.** *Still open.* If logind reports no graphical
  session, there is nowhere to put a launcher window. Log and do nothing, presumably — but
  confirm that rather than letting `systemd-run` fail obscurely.

## Future

**Linux.** The one substantial thing left in this crate — see
[Linux — reactive, one-shot](#linux--reactive-one-shot) for the design and
[`TODO.md`](TODO.md) for the list.

Signature-based trust used to be the headline here. It shipped; see [Trust](#trust).

## Status / roadmap

Grouped the way the code is: one shared core, two triggers. [`TODO.md`](TODO.md) carries the
same list in more detail.

**Shared core** — OS-agnostic, built once, in [`src/volume.rs`](src/volume.rs) and friends:

- [x] Rust codebase scaffold, `cfg`-gated per OS.
- [x] Verify `launcher.exe`'s minisign signature against the anchors `build.rs` baked in, and
      require it to declare the launcher role.
- [x] Hold the verified file open against writers and deleters until it has been started.
- [x] Take the launcher's version from the signed comment rather than running it.
- [x] Auto-launch it from the volume root.
- [x] Log ignored volumes with the reason.

**Windows trigger** — resident, in [`src/trigger/windows.rs`](src/trigger/windows.rs):

- [x] Hidden **top-level** window (not message-only) and a `GetMessage` loop, no polling.
- [x] `WM_DEVICECHANGE` / `DBT_DEVICEARRIVAL`, decoding the `dbcv_unitmask` bitmask.
- [x] Startup sweep for volumes already connected before login.
- [x] Named-mutex single-instance guard.
- [x] Drive-type and `DBTF_NET` filtering, arrival debounce, and `SEM_FAILCRITICALERRORS`
      so an empty card reader can't pop a modal error box.
- [x] End-to-end run against real removable hardware: a genuine arrival carried all the way
      through to a launch.

**Linux trigger** — one-shot, not started
([`src/trigger/linux.rs`](src/trigger/linux.rs) is a placeholder):

- [ ] udev rule and the `systemd-run --no-block` handoff.
- [ ] Bounded wait for the mountpoint to appear before calling the core.
- [ ] logind session lookup and environment handoff into the user's graphical session.
- [ ] Confirm nothing stays resident once the launcher is running.

**Both:**

- [x] Behave correctly when started by hand, plus `--check <path>` to run the core against a
      single volume and exit, and `--signature` to print what this build trusts. Registering
      the Windows login entry and installing the Linux udev rule are the **installer's**
      job — see [`../installer/structure.md`](../installer/structure.md).
