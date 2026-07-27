<!--
  SPDX-License-Identifier: GPL-3.0-or-later
  Copyright (C) 2026 da1sh1n
  This file is part of GaCaSy, licensed under the GNU General Public License
  v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
  or <https://www.gnu.org/licenses/> for details.
-->

# GaCaSy Listener (PC side)

The PC-side component that auto-detects a connected cartridge, verifies it, and
launches the cartridge's launcher.

**Windows is built. Linux is specced but not implemented** — see
[`src/trigger/linux.rs`](src/trigger/linux.rs) for exactly what is missing.

One Rust crate, `cfg`-gated per OS. A cartridge is **any mounted storage
volume** (NVMe / SSD / HDD / USB) — detection is "a new volume showed up", not a
specific USB device id.

The two builds share their logic but **not their shape**: on Windows the
listener is a resident background process waiting on `WM_DEVICECHANGE` from
login to logout, while on Linux nothing runs at all until udev starts it on
connect — it verifies, launches, and exits. See
[Execution models](structure.md#execution-models) for why, and what each one
costs.

## Layout

```text
src/
  main.rs      entry point, folder resolution, argument handling
  volume.rs    the shared core — marker, trust check, launch (used by both OSes)
  marker.rs    parsing the cartridge's .cartridge file
  config.rs    parsing this listener's own config.toml
  log.rs       the activity log
  config.toml  the seed written when no config.toml exists yet
  trigger/
    windows.rs  resident: hidden top-level window + GetMessage loop
    linux.rs    one-shot: udev handoff — NOT BUILT YET
output/        the deployed listener — this is what you ship
  listener.exe
  config.toml
  listener.log
```

`output/` is refreshed by `cargo run`, exactly like the launcher's. `config.toml`
is never overwritten once present, so an edited key list survives every build.

## Running it

```sh
cargo run                              # start the trigger for this platform
cargo run -- --check E:\               # run the core once against a volume, then exit
cargo run --release -- --check E:\     # same, and deploys the release exe to output/
```

`--check` is the way to answer "would this cartridge launch on this PC?"
without plugging anything in.

`output/listener.exe` is refreshed by the exe that is *running*, so `cargo run`
deploys a debug build and `cargo build --release` deploys nothing at all — it
never runs anything. To put the shippable release build in `output/`, run it
once: `cargo run --release -- --check .` deploys and exits immediately.

Registering the Windows login entry and installing the Linux udev rule are the
**installer's** job ([`../installer/structure.md`](../installer/structure.md)),
not this program's — running it by hand does nothing special.

## Where it looks

| | |
| --- | --- |
| `<exe folder>\config.toml` | the keys this PC trusts, written by the installer |
| `<exe folder>\listener.log` | the activity log (override with `log_file`) |
| `<volume>\.cartridge` | the cartridge's marker: `version`, `key`, `launcher` |

Installed, `<exe folder>` is **`%LOCALAPPDATA%\GaCaSy\`** — the exe, its config
and its log, one folder, no administrator needed to put them there or to read
them back.

The log is worth knowing about: the listener has no console and no visible
window, so it is the only way to see why a cartridge did or didn't launch.
Every volume it looks at gets a line, including the ones it ignores and why.
A copy dropped by hand into a read-only folder falls back to
`%LOCALAPPDATA%\GaCaSy\listener.log` rather than going silent — the same folder
an install uses, so there is one place to look either way.

## Trust

`keys = []` in config.toml — the shipped default — **trusts every cartridge**.
A fresh install works as soon as a volume carries a valid `.cartridge` marker,
with no pairing step. Listing even one key switches to matching that list and
nothing else.

So in the default state the marker is the only thing between plugging a disk in
and running the binary it names. That is a deliberate v1 posture (the key is a
shared secret anyone who can read a cartridge can copy — a recognition
handshake, not tamper protection), but it is worth knowing before you leave it
open. See [Trust](structure.md#an-empty-keys-list-trusts-everything).

See [`structure.md`](structure.md) for the full specification, including the
**Cartridge identification system** — the `.cartridge` key handshake this
component performs before launching anything.
