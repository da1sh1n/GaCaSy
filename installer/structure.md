# Romzeta Installer — Setup Side (spec to build)

Part of the **Romzeta** game-cartridge system. This document covers the **installer**: the
single file an end user downloads, which turns blank media into a cartridge, installs the
PC-side listener, and edits cartridges that already exist. The cartridge-side app is
documented in [`../launcher/structure.md`](../launcher/structure.md), the PC-side service in
[`../listener/structure.md`](../listener/structure.md); the user-facing overview is in
[`../README.md`](../README.md).

> **Built.** This was a spec; it is now a description. Where the built thing differs
> from the original spec — the elevation model, chiefly — the section says so and why.

## Purpose

The installer is the only thing a user has to obtain. Everything else — `launcher.exe` on
the cartridge, `listener.exe` on the PC, the config and catalog files — is *placed by it*.
It never downloads anything: **one self-contained exe, no internet, no prerequisites, no
side-by-side files.**

## Shape

- **Its own crate**, `installer/`, sibling to `launcher/` and `listener/`, tied together by
  the root `Cargo.toml` workspace.
- **`eframe` / `egui`** for the UI: pure Rust, statically linked, no runtime dependency.
  This is the reason it is not a webview like the launcher — a WebView2-based installer that
  finds the runtime missing has no way to bootstrap itself with no internet.
- `#![windows_subsystem = "windows"]`, no console window (same as the launcher).
- **Windows for v1.** The Linux equivalents of every Windows-shaped step (`Program Files`,
  registry autostart, `launcher.exe`) are noted under [Future](#future) so the design
  doesn't paint itself into a corner.
- **Asks for no elevation** — see [Elevation](#elevation), which is where this build
  departs from the original spec.

### Source layout

```text
installer/
  build.rs        finds the payload, stages it in OUT_DIR, fails loudly if it's missing
  src/
    main.rs       entry point and the module map
    app.rs        wizard state, and the create-vs-edit routing rule
    ui/           the screens; ui/mod.rs holds the shell and the only footer
    work.rs       the worker thread, and how the UI hears from it
    payload.rs    the embedded launcher, listener and seed files
    volume.rs     which drives can be cartridges, and which already are
    detect.rs     finding the game's exe inside a folder, and measuring it
    image.rs      cover dimensions, for the 2:3 warning
    catalog.rs    catalog.json — the file the launcher reads
    cartridge.rs  the write itself: copy, catalog, config, launcher
    listener.rs   job 2 — install folder, Run entry, uninstall
    copy.rs       the cancellable, measured file copy underneath it all
    version.rs    x.y.z — its own, and the one a cartridge's signature states
    autoplay.rs   the Windows AutoRun nudge
    font.rs, image.rs, clipboard.rs, reg.rs, shell.rs   the small OS errands
```

### Embedded payload

The installer carries its outputs inside itself, via `include_bytes!` / `rust-embed`:

```text
installer.exe
  ├─ launcher.exe      ← written onto the cartridge
  ├─ listener.exe      ← written into the listener's install folder
  ├─ config.toml       ← cartridge seed, from launcher/src/config.toml
  └─ catalog.json      ← cartridge seed, from launcher/src/catalog.json
```

The listener used to have a seed config carried alongside these, because pairing a PC edited
its `keys` list. The listener has no config file at all now — trust is compiled into it — so
there is nothing to seed and nothing to merge.

**Both embedded binaries are checked at build time**: `build.rs` verifies each one's signature
against `keys/*.pub` *and* that its signature declares the role its slot is for, through the
same [`../trust/`](../trust/) crate the listener uses at runtime. Getting the sign-then-build
order wrong would otherwise produce an installer that works, writes a cartridge that looks
right, and is silently ignored by every listener on earth.

Build-ordering consequence: **launcher and listener must be built `--release` before the
installer**, which `build.rs` enforces — a missing artifact fails the build with the command
to run, rather than producing an installer that ships nothing.

The workspace's `default-members` leaves the installer out for the same reason, so the two
steps happen in the right order by default:

```sh
cargo build --release               # launcher + listener
cargo build --release -p installer  # embeds what that produced
```

In a single all-crates invocation nothing orders the installer's build script after the
launcher's link step — there is no dependency edge between them, and binary-artifact
dependencies are still unstable — so it is a race that usually goes your way. Two commands
is cheaper than a flaky one.

`ROMZETA_PAYLOAD_OPTIONAL=1` builds an installer with empty binary slots, for working on the
UI without a release build in front of every iteration. That build is not shippable and says
so: `payload::defect()` puts a red line across the top of every screen and every write
refuses before it starts.

## Why not part of the listener

The installer is deliberately *not* a second mode of `listener.exe`. It has to embed
`launcher.exe` regardless, so embedding the listener alongside it costs no extra machinery —
whereas bundling would push the whole wizard UI into a process that has to stay small and
cross-platform: one that runs at every login on Windows, and that udev fires and kills per
connection on Linux, where `Program Files` means nothing and a wizard has nobody to show
itself to.

## Elevation

**The spec said "requires elevation — a UAC manifest, for the `Program Files` write in job
2". The built installer asks for none, and has no code that could.** The reasoning is in the
spec's own blockquote under [job 2](#job-2--install-the-listener): the listener keeps its exe,
config and log in one folder, `Program Files` is the one place that can't hold, and a folder
under `%LOCALAPPDATA%` is writable and needs no elevation — "which would also remove the only
reason job 2 requires admin".

That is now the *only* location. See [Where the listener lives](#where-the-listener-lives).
What follows from having no elevated path at all:

- The listener's three files stay together, always writable, always in the folder its own
  documentation points at.
- **Job 1 stops demanding admin.** Writing a cartridge to a drive the user can already write
  to never needed it, and a blanket `requireAdministrator` manifest would have put a UAC
  prompt in front of the common case to serve the rare one.
- No process-token check, no `ShellExecuteW "runas"`, no elevated relaunch, no second copy of
  the installer racing the first over one registry value. The
  `Win32_Security` / `Win32_UI_Shell` windows-sys features are gone with them.

### Where the listener lives

`%LOCALAPPDATA%\Romzeta\` — `listener.exe` and `listener.log`, together.

It is the same path `settings::fallback_log_path` in
[`../listener/src/settings.rs`](../listener/src/settings.rs) already names as the log's home,
so the install folder and the log folder are one folder rather than two that a paragraph had
to reconcile. An installed listener never reaches that fallback at all: the primary path and
the fallback resolve to the same file.

Program Files bought a shared *binary* and nothing else — autostart is `HKCU\…\Run`, per user,
wherever the exe sits, so every account that wants the listener registers its own regardless.
It is not offered.

**Folders earlier builds used** — `%LOCALAPPDATA%\Programs\Romzeta\` and
`%ProgramFiles%\Romzeta\` — are still *looked at*, never written. An install found in one is
listed on the listener screen and folded in when you install: it is stopped, un-registered
and deleted. This used to carry that install's trusted keys over first, and that step was the
delicate part of the whole operation — removing the folder without them would silently
un-pair every cartridge the PC knew. There is nothing to carry now: what a listener trusts is
compiled into it, so a replacement trusts exactly what the new binary was built to trust. A
leftover that refuses to delete (a Program Files copy, with no elevation to remove it) is
reported and stepped over — the new listener works either way, and the stale copy no longer
holds the login entry.

## Flow

Which job runs is decided by the target volume:

```text
volume picked  →  launcher.exe we signed?  →  yes → edit mode  (add / remove games, rename)
                        │ no
                        └─────────────────────→ create mode (name → games → exes → images → copy)
```

"We signed" is the whole test, and it is made by verifying the file rather than by finding a
name — see [Is this already a cartridge?](#is-this-already-a-cartridge).

Installing the listener (job 2) is independent of both and can be run on its own.

## Job 1 — Create a cartridge

1. **Pick the target volume — external drives only.** See
   [Which drives are offered](#which-drives-are-offered). A volume that already carries a
   launcher routes to [job 3](#job-3--edit-an-existing-cartridge); one without lands here.
2. **Name the cartridge.** See [Naming a cartridge](#naming-a-cartridge). Seeded with the name
   the drive already has, so this is a step you can walk straight past.
3. **Add games.** The user picks one or more game folders.
4. **Per game:** auto-find the executable (below), then pick a cover image.
5. **Review**, check free space, then copy — with a progress bar on a worker thread. Game
   folders run to many GB; the UI must stay responsive and the copy must be cancellable.
6. **Write the cartridge layout**, then set the drive's name.

There is no key step. A cartridge's identity is the signature inside the `launcher.exe` the
installer carries, so there is nothing for the user to choose or keep in step.

### Naming a cartridge

A cartridge's name is the drive's **volume label**, and it is the only part of a cartridge that
is not a file on it. There is nowhere else for it to live: `config.toml` is look and feel,
`catalog.json` is the game list, and identity is a signature inside `launcher.exe`. So the
label is the whole of it — read for the picker's summary line, written by `volume::set_label`.

Two things follow from putting it in the plan rather than applying it on the keystroke:

- **A rename alone is a plan worth running.** `Plan::is_empty` counts the label, so a cartridge
  can be renamed without also adding or removing a game.
- **It is written last — after `launcher.exe`.** A cancelled or failed run never reaches it,
  which is what keeps "the cartridge is as it was" literally true of a cancel, and it means the
  rename needs no rollback entry. A rename that fails on its own comes back as a *warning* on
  the Done screen, not an error: the cartridge was written correctly, and reporting otherwise
  would say a working cartridge doesn't work.

What a label may contain is a property of the filesystem and nothing else — 32 characters on
NTFS, 11 on the FAT family, which also refuses `* ? / \ | . , ; : + = [ ] < > "`. That is
checked when the plan is built rather than at the write, so a name that was never going to land
stops the Review button instead of the last step of a copy that has already run for minutes. An
unrecognised filesystem is given the larger limit: Windows is the authority at the moment of
writing, and guessing low would refuse names that would have worked.

### Which drives are offered

**External storage only.** The drive Windows is installed on is refused outright, and so is
every internal disk. A cartridge is a thing you unplug and carry to another PC, and making one
means copying many gigabytes onto a volume and — in edit mode — deleting folders from it.
Pointing that at `C:\` is not a supported choice that happens to be unwise; it is not offered.

This is deliberately **stricter than the listener**, which starts a correctly signed launcher
from any volume it sees. The asymmetry is the right way round: the listener decides whether to
*run* something already on a disk, this decides where to *write* gigabytes. A cartridge
someone hand-assembles on an internal disk still works — the installer just won't make one
there.

#### What counts as external

Not `DRIVE_REMOVABLE`, which is the tempting answer and the wrong one. A USB SSD in an
enclosure — precisely the drive a game cartridge wants to be — reports as `DRIVE_FIXED`,
exactly like the disk Windows is installed on. Filtering on it would reject the best
candidates and admit none of the ones it was meant to catch.

So the question is asked of the hardware: `IOCTL_STORAGE_QUERY_PROPERTY` for the underlying
disk's **bus type**. USB, FireWire, SD and MMC are external; SATA, NVMe, SAS, RAID and the
rest are not. The handle is opened with **zero** access rights, which is why none of this
needs administrator — asking for `GENERIC_READ` would put a UAC wall in front of the drive
picker.

Two consequences worth knowing:

- A **Thunderbolt/PCIe enclosure** presents its disk as `BusTypeNvme`, indistinguishable from
  an internal one, so it is treated as internal and refused. That is the conservative
  direction to be wrong in: the cost is a drive you can't pick, not a system disk you can.
  USB-attached NVMe enclosures — the common kind — report `BusTypeUsb` and are fine.
- **Windows-To-Go** on a USB stick passes the bus test, so the system-drive check runs first,
  separately, and wins.

The system-drive check compares drive letters against `%SystemRoot%`, `%windir%` and
`%SystemDrive%`, and **any** match vetoes. Not hardcoded to `C:` — Windows on another letter
is rare but real, and a hardcoded `C:` would be wrong in the dangerous direction there. Three
variables rather than one because each can be missing or tampered with, and requiring only one
to match means no single absent variable can quietly switch the veto off.

If the bus query fails outright, the fallback is Windows' own coarse answer taken
conservatively: only a drive Windows itself calls *removable* gets through, so an
unidentifiable fixed disk is refused rather than guessed at.

Refused drives are **not listed at all**. `volume::list()` returns only the ones that can be
picked; `volume::all()` is the unfiltered view, kept for the tests that check a drive is
refused for the right *reason* rather than merely absent.

They used to be listed, greyed out, under a "Not usable" heading with the reason beside them —
on the grounds that a filter which silently shortens a list is indistinguishable from a bug to
the person looking at it. That reasoning was right about the risk and wrong about the remedy:
on a normal PC it filled the picker with `C:` and every internal disk, rows that existed only
to say no. The screen answers "why isn't my D: drive here?" in one line of prose above the
list instead, once rather than once per drive. Network shares, optical drives and RAM disks
never reach either function — probing a stale network mount can block for a long time.

The check runs at **three** points, because the list can go stale under a click — an external
drive unplugged, its letter picked up by something internal: the picker won't offer it,
`choose_volume` refuses it, and `plan()` refuses it again, which is what the Review screen
shows and what the Write button consumes.

### Resulting layout

Matches the launcher's deployed layout
([`../launcher/structure.md`](../launcher/structure.md#deployed-layout)) exactly:

```text
<volume>/
  launcher.exe     <- the app, from the embedded payload; the signature rides inside it
  config.toml      <- look and feel only
  catalog.json     <- the game list the installer just built
  assets/images/   <- one cover per game
  games/           <- the copied game installs
```

There is **no identity file**. What makes this a cartridge is the minisign signature carried
inside `launcher.exe`, which the listener checks against a key compiled into itself — the
`.cartridge` marker that used to sit at the volume root is gone.

One part of a cartridge is not in this layout at all: its **name**, which is the drive's volume
label. See [Naming a cartridge](#naming-a-cartridge).

`assets/EBWebView/` is **not** created by the installer — the launcher makes it on first run.

### Executable auto-detection

The fiddliest part of job 1. For each chosen game folder:

- Recursively collect every `*.exe`.
- **Reject** known non-game names: `unins*`, `*setup*`, `vcredist*`, `dxsetup`, `directx*`,
  `*crashhandler*` (e.g. `UnityCrashHandler64.exe`), and anything under `redist/`,
  `_CommonRedist/` or `Engine/Binaries/ThirdParty/`.
- **Score** the survivors: shallower path wins, a name matching the folder name wins, larger
  file wins — in that order of weight. A name match is worth several folder levels; size is
  capped so it can only ever break a tie.
- One clear winner → preselect it. Ambiguous, or nothing left → the user **must** pick
  manually. "Clear" means the top score beats the runner-up by at least one depth level;
  a coin flip is never presented as a decision.
- The user can **always** override the pick, including when detection succeeded. A
  hand-picked exe must be *inside* the game folder, since that folder is all the copy moves.

Two bounds keep a pathological tree from hanging the scan: nothing deeper than eight levels
is looked at, and files under 16 KB are treated as stubs rather than game binaries. Symlinks
are neither followed nor counted, which also keeps the byte total honest — the copy doesn't
follow them either.

The same walk **measures the folder**. A game install is walked once, not twice: the byte
total it produces is what the free-space check and the progress bar both use.

### Cover images

One image per game, chosen by the user, copied to `assets/images/<slug>.<ext>`. The extension is
kept rather than forced to `.png`: the launcher hands the path to the webview, which goes by
content and not by name, and renaming a `.webp` to `.png` only makes the cartridge harder to
read later.

The launcher's native cover size is **600×900 (2:3)** — `COVER_NATIVE_WIDTH` /
`COVER_NATIVE_HEIGHT` in [`../launcher/src/constants.rs`](../launcher/src/constants.rs).
v1 copies the file as-is and **warns** on a non-2:3 ratio rather than resizing it, which
keeps the exe small and dependency-free.

Dimensions come from a header parser, not an image library — PNG, WebP (including the `VP8X`
form an *animated* WebP uses), JPEG and GIF. The format is decided by the bytes and the
extension is never consulted, because cover art is routinely an animated WebP saved as
`.png`. Anything unrecognised produces no warning at all, rather than a rule that would
reject formats the webview renders perfectly well.

### Catalog writing

`catalog.json` is the array of `{ name, exe, image }` the launcher deserializes into its
`Game` struct. Paths are **relative to the cartridge root** (`games/bg3/bg3.exe`,
`assets/images/bg3.png`), and `name` defaults to the game folder's name, editable by the user.

## Job 2 — Install the listener

Steps 1 and 2 are the same everywhere. **Step 3 is not** — the listener's two builds have
different process lifetimes (see
[Execution models](../listener/structure.md#execution-models)), so "make it run" means two
unrelated things:

1. Copy the embedded listener binary into place: **`%LOCALAPPDATA%\Romzeta\listener.exe`**,
   the only location, alongside the config and log it writes. There is no choice to make and
   no elevation — see [Elevation](#elevation).
   > The original spec said `C:\Program Files\Romzeta\` and named the problem with it in the
   > same breath: the listener keeps its exe, config and log in one folder, and Program Files
   > is the one location where that can't hold — the user it runs as can't write there, so
   > its log falls back to `%LOCALAPPDATA%\Romzeta\listener.log`. Rather than keep a location
   > that splits the three files up, the install *is* that folder.

   A listener that is already running holds its own exe open, so an update or repair stops
   it first — matched on the **full image path**, never on the name `listener.exe`. It has
   no window to close and no IPC to ask through, and nothing to lose by being stopped: its
   only state is its log.
2. There is **no step 2 any more.** This used to write a `config.toml` beside the exe and
   append the new cartridge's key to its `keys` list — carefully, as a textual edit rather
   than a TOML round-trip, so the comments explaining the key model survived. The listener has
   no config file now: what it trusts is compiled in, and a trusted-key list in a writable
   file beside the exe was precisely the capability the signature exists to deny. Writing the
   binary *is* installing the trust.
3. **Make it run — per OS:**
   - **Windows (v1)** — the listener is a **resident** process, so register it to start at
     login: an `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` entry named
     `Romzeta Listener`, holding the **quoted** exe path — `C:\Users\First Last\AppData\…`
     has a space in it whenever the account name does. Per-user, like the install folder it
     points into. Alternatives worth keeping in mind: `HKLM\…\Run` for all users, or a Task
     Scheduler logon trigger if it ever needs to start elevated.
     > It is also started **immediately**, not just registered. Without that, plugging a
     > cartridge in does nothing until the next login, which reads as the install having
     > failed.
   - **Linux (future)** — there is **nothing to autostart**. The listener is one-shot and udev
     starts it per event, so the installer's whole job is dropping
     `/etc/udev/rules.d/99-romzeta.rules` and running `udevadm control --reload`. Note the
     elevation shape inverts: this needs **root** for a system-wide rules directory, where
     Windows needs no elevation at all.
4. **Repair / uninstall:** detect an existing install and offer to replace or remove it,
   including one left in a folder an earlier build used. Removing means deleting the folder
   *and* undoing step 3 — the `Run` entry on Windows, or the rule file plus a
   `udevadm control --reload` on Linux. The `Run` entry is only cleared when it points at
   *that* folder's exe, so cleaning up a stray copy can't retire the working install's login
   entry as a side effect.

## Job 3 — Edit an existing cartridge

When the picked volume already carries a launcher this installer can verify, read its
`catalog.json` and let the user:

- **Add games** — same flow as job 1, steps 3–5, appending to the existing catalog.
- **Remove games** — delete the game folder and its image, drop the catalog entry.
- **Rename it** — the volume label, see [Naming a cartridge](#naming-a-cartridge).
- **Refresh its launcher** — offered only when the cartridge's launcher is a different version
  from the one this installer carries. That version comes out of the file's *signature*, so
  noticing it costs nothing and asks the binary nothing.

There is no "change the key" step, because there is no key. A cartridge's identity is the
signature inside its `launcher.exe`, and the only way to change it is to write a different
launcher.

### Is this already a cartridge?

The question that decides create-vs-edit, and the one place the installer makes a trust
decision of its own: `volume::attested_launcher` reads `<root>/launcher.exe`, verifies it
against the anchors `build.rs` baked in, and requires the launcher role — the same call the
listener makes, through the same [`../trust/`](../trust/) crate.

**Nothing is executed.** The installer used to answer this by looking for a *file name* and
then running the file with `--version` to see what it was: running an arbitrary binary off a
stranger's USB stick in order to decide what it was, which is the exact thing
[`../listener/src/trust.rs`](../listener/src/trust.rs) documents at length that it must never
do, done by the program next door. The version was inside the signature the whole time.

A drive with no launcher, an unsigned one, one signed by a key this build does not carry, and
a genuine Romzeta binary that is not a launcher all mean the same thing here — *not a cartridge
this installer made* — and none of them are worth telling apart on a screen whose next
question is "create or edit?".

## Settled questions

### Adding the same game twice — **rejected**, with the existing game named

The two alternatives are both worse. *Overwrite* destroys an install that may be many
gigabytes and may be the user's only copy, over an ambiguous click. *Rename* produces
`games/foo` and `games/foo_2` — two entries the user cannot tell apart on the cartridge later,
and no way back to which was which. Refusing costs one click: remove the existing one, or
rename the folder being added.

The check runs twice over, because there are two ways to collide: the same source folder
added twice in one session, and a folder whose slug matches a game already on the cartridge.
A *third* collision — two differently-named games that squash to the same slug (`Game: II`
and `Game II`) — is not a user mistake and is silently suffixed instead.

### Free space — **precheck, and handle the failure anyway**

The precheck is cheap and catches the honest case before a twenty-minute copy starts. It
demands the measured bytes plus the launcher plus **256 MB of headroom**, because the
measurement is a sum of file sizes and what a filesystem consumes is that plus per-file slack
and cluster rounding — and because filling a cartridge to the last byte leaves the launcher no
room for its log and WebView2 cache.

It is a precheck, not a guarantee, so the copy still handles running out mid-way. That path
is the same one cancel takes: everything **this run** created is removed, content that was
already on the cartridge is untouched, and `catalog.json` — written last — still describes
the cartridge as it was. A failure leaves a cartridge that is *older* than intended, never
one listing games it doesn't have.

Space that removals will release is not subtracted from the estimate. Removals happen first,
so a tight plan simply proceeds with the room they freed, and one that passes without them
was never in doubt.

### Out of scope for v1

**Formatting or erasing media.** The installer writes files to a volume; it never
repartitions one.

## Future

- **Linux target:** `/opt/romzeta` or `~/.local/share/romzeta` instead of Program Files, a
  **udev rule instead of a Run key**, and a Linux launcher binary instead of `launcher.exe`.
  The Linux listener is *not* a service and has nothing to autostart — it is started by udev
  per connection and exits, so there is no user unit to enable. See
  [`../listener/structure.md`](../listener/structure.md#linux--reactive-one-shot).
Signature-based trust used to be the second item here. It shipped — see
[Is this already a cartridge?](#is-this-already-a-cartridge) and
[`../SIGNING.md`](../SIGNING.md).

## Status / roadmap

- [x] Root `Cargo.toml` workspace tying `launcher` + `listener` + `installer`.
- [x] `installer` crate scaffold: `eframe`/`egui`, `windows_subsystem`. **No UAC manifest** —
      see [Elevation](#elevation).
- [x] Embedded payload + build-time check that every artifact is present, signed by a key the
      embedded listener accepts, and signed *as the program its slot is for*.
- [x] Volume enumeration, **external drives only** (bus type, plus an unconditional
      system-drive veto), and create-vs-edit routing decided by **verifying** the launcher
      already on the drive rather than by running it.
- [x] Game folder picker, executable auto-detection, manual override.
- [x] Per-game cover image picker with the 2:3 warning.
- [x] Cartridge write: threaded `games/` copy with progress + cancel, covers under
      `assets/images/`, catalog, `config.toml`, and the signed `launcher.exe` that is the
      cartridge's whole identity.
- [x] Listener install into `%LOCALAPPDATA%\Romzeta` — the only location — and a `Run` entry on
      Windows. Installs left by an earlier build elsewhere are cleared out.
- [x] Edit mode: add games, remove games, rename, refresh a stale launcher.
- [x] Free-space precheck and failure/rollback handling.
- [x] Uninstall / repair path.
- [x] **Exercised end to end on real media**: a cartridge written to a physical drive, then
      unplugged, replugged and booted by an installed listener.
- [x] Every screen checked visually at the user's real display scale.
- [ ] Future: Linux target.
