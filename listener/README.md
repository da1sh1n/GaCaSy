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
  volume.rs    the shared core — verify, then launch (used by both OSes)
  trust.rs     which file to check, holding it still, and saying why not
  version.rs   x.y.z, its own and a launcher's
  settings.rs  the fixed tunables and where the log goes
  alert.rs     the one thing it ever says out loud
  log.rs       the activity log
  trigger/
    windows.rs  resident: hidden top-level window + GetMessage loop
    linux.rs    one-shot: udev handoff — NOT BUILT YET
output/        the deployed listener — this is what you ship
  listener.exe
  listener.log
```

`output/` is refreshed by `cargo run`, exactly like the launcher's.

**There is no config file.** There used to be one holding the cartridge keys this
PC trusted, plus a debounce window and a log path. The key list is gone because
trust is now cryptographic and compiled in — a list of trusted keys in a writable
file beside the exe would let anything that could edit it grant itself auto-run on
every insert, which is the exact capability the signature exists to deny. What
was left was two tunables nobody has ever needed to change, so the file went with
them. See [`src/settings.rs`](src/settings.rs).

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
| `<exe folder>\listener.log` | the activity log |
| `<volume>\launcher.exe` | the only file on a cartridge it reads, and the whole test |

Installed, `<exe folder>` is **`%LOCALAPPDATA%\GaCaSy\`** — the exe and its log,
one folder, no administrator needed to put them there or to read them back.

The log is worth knowing about: the listener has no console and no visible
window, so it is the only way to see why a cartridge did or didn't launch.
Every volume it looks at gets a line, including the ones it ignores and why.
A copy dropped by hand into a read-only folder falls back to
`%LOCALAPPDATA%\GaCaSy\listener.log` rather than going silent — the same folder
an install uses, so there is one place to look either way.

## Trust

A cartridge is **a volume with a `launcher.exe` at its root carrying a valid
signature, from a key this listener was built to trust, declaring itself a
launcher.** No marker file, no key stored on the cartridge, nothing on this PC to
edit.

Three things follow, and they are the point of the design:

- **The file is read, never asked.** A binary that reports its own
  trustworthiness proves nothing, and to ask you would first have to run the
  thing you are deciding whether to run. Nothing downstream asks either — a
  launcher's version comes out of its signature too.
- **The trust anchor is compiled in.** Changing what a listener accepts means
  replacing the listener. See [`build.rs`](build.rs), which bakes in `keys/*.pub`.
- **The signature declares a role.** All three GaCaSy programs are signed with the
  same key, so "signed by us" is not the same question as "is a launcher" — a
  genuine `installer.exe` renamed to `launcher.exe` is refused.

`listener.exe --signature` prints this build's own signature and the keys it
trusts, which is the way to check what a given copy will accept.

The signature covers `launcher.exe`'s own bytes and nothing else on the disk —
`catalog.json`, `config.toml`, `games/` and `assets/` are unsigned, and the
launcher treats them as untrusted input. See [`../SIGNING.md`](../SIGNING.md) §1
for what that does and does not buy, and [`structure.md`](structure.md) for the
full specification.
