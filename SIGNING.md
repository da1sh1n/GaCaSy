<!--
SPDX-License-Identifier: GPL-3.0-or-later
Copyright (C) 2026 da1sh1n
-->

# Signing and building GaCaSy

Everything about how a cartridge proves it is a cartridge, and how to build the
three programs without producing something that silently does nothing.

---

## TL;DR

```powershell
.\build.ps1          # Windows
./build.sh           # Linux / macOS / Git Bash
```

That generates a dev signing key if you have none, then builds and signs
`launcher.exe`, `listener.exe` and `installer.exe` into `target/release/`.

Plain `cargo build --release` **is not enough** and will fail. Why is the rest
of this document.

---

## 1. What the signature is for

The listener runs on the PC and watches for volumes being plugged in. When one
arrives it has to decide, with no human in the loop, whether to execute a
program off that disk. That is an auto-run mechanism, so the decision has to be
unforgeable.

The definition of a cartridge is exactly one sentence:

> A volume with a `launcher.exe` at its root carrying a valid signature from a
> key the listener was built to trust.

No marker file, no key stored on the cartridge, nothing on the PC to edit. An
earlier design had all three, and its own docs admitted it was a recognition
handshake rather than a security boundary — anyone who could read a cartridge
could clone its secret.

Two properties fall out of this and are worth stating because they constrain
everything else:

- **The listener reads the file; it never asks the program.** A binary that
  reports its own trustworthiness proves nothing, and to ask you would first
  have to run the thing you are deciding whether to run. See
  [listener/src/trust.rs](listener/src/trust.rs).
- **The trust anchor is compiled in, not configured.** If the list of accepted
  keys were a line in a `config.toml` next to the exe, anything that could write
  that file could append its own key and get arbitrary code auto-run. Changing
  what a listener trusts means replacing the listener. See
  [listener/build.rs](listener/build.rs).

---

## 2. The keys

GaCaSy uses [minisign](https://jedisct1.github.io/minisign/) (Ed25519). There is
one secret key and two public key *slots*.

| File | What it is | Committed? |
|---|---|---|
| `keys/gacasy.pub` | The release key. Every published listener trusts it. | **yes** — it is public |
| `keys/dev.pub` | Whatever `xtask keygen` made on your machine. | no — gitignored |
| `~/.gacasy/gacasy.key` | **The secret key.** | **never** |

On Windows that secret path is `%USERPROFILE%\.gacasy\gacasy.key`.

A listener trusts **both** public keys, and that is the whole point of the
two-slot design: your build accepts your cartridges *and* official ones, while
an official listener still refuses yours. Cloning the repo and building gives
you a working system without anyone handing you a secret.

At least one of the two must exist or `listener/build.rs` fails the build — a
listener with no anchor would refuse every cartridge on earth and log its way
through the confusion one volume at a time.

### The secret key never enters the repository

This is the one rule everything else serves. The repo is public; the only thing
stopping a stranger producing a launcher that published listeners will execute is
that they do not have this file. So:

- the default location is outside the working tree entirely,
- `xtask keygen` **refuses** to write it anywhere inside the repo
  (`refuse_inside_repo` in [xtask/src/keys.rs](xtask/src/keys.rs)),
- `.gitignore` carries `*.key` as a second lock on an already-shut door.

Back it up somewhere safe. Losing it means no future release can be signed with
the same identity, and every listener already built against it would have to be
replaced.

### Generating one

```
cargo run -p xtask -- keygen
```

Writes the secret to `~/.gacasy/gacasy.key` and the public half to
`keys/dev.pub`. Once per machine. It refuses to overwrite an existing key,
because doing so would orphan every cartridge already signed with the old one.

By default the key is **unencrypted**, and that is a deliberate and reasonable
choice for a key whose only job is signing your own local builds. Nothing needs
to be configured; there is no password, no `.env`, no prompt.

If you want a key you have to unlock, set `GACASY_SIGNING_PASSWORD` *before*
running `keygen`. From then on that same variable (environment, or a `.env` at
the repo root) unlocks it; without it you get an interactive prompt.

`--release` writes `keys/gacasy.pub` instead. There is only ever one of those,
ever, so it refuses to overwrite it.

---

## 3. How the signature is carried

A cartridge is one file. No `.minisig` sidecar to lose, nothing to keep in step
with the exe. So the signature lives *inside* the exe, appended past the end:

```text
[ the exe exactly as the linker produced it        ]  <- the signed bytes
[ minisig text, N bytes UTF-8 (the 2-line format)  ]
[ N            u32 little-endian                   ]  }
[ format       u16 little-endian, = 1              ]  } 16-byte footer
[ magic        b"GACASYSIG\0" (10 bytes)           ]  }
```

You cannot sign bytes that already contain their own signature, so the choice is
between reserving a blank region (sign it blank, fill it in, blank it again to
verify) or appending past the end. GaCaSy appends. The signed bytes are then
byte-for-byte the linker's output, and verifying is just "chop off the footer and
check what is left".

PE and ELF both ignore trailing data — it sits outside every section header and
is never mapped — so a signed exe runs exactly as the unsigned one did.
Authenticode does structurally the same thing.

Implementation: [sigblock/src/lib.rs](sigblock/src/lib.rs). Re-signing
*replaces* the block rather than nesting it, so signing twice is safe.

Note the asymmetry: xtask signs with `minisign` (the reference implementation)
and verifies with `minisign-verify` (what the listener actually links).
Verifying with the same library that just signed would only prove it agrees with
itself.

---

## 4. The build order, and why it is not negotiable

Four stages, and the dependencies between them are **not the kind cargo can
see**:

1. Build `launcher` and `listener`.
2. **Sign them.** This rewrites the files cargo just produced.
3. Build `installer`, whose `build.rs` embeds those *now-signed* bytes.
4. Sign the installer.

Stage 3 must come after stage 2. If it does not, the installer ships an unsigned
launcher — and then the installer builds fine, runs fine, writes a
perfect-looking cartridge, and every listener on earth silently ignores it. The
user's only symptom is *nothing happening* when they plug it in.

That failure is expensive enough, and quiet enough, that the sequence lives in
code rather than in a README: [xtask/src/release.rs](xtask/src/release.rs).

Two structural decisions back this up:

- **`installer` is not a default workspace member.** In a single
  `cargo build --workspace` invocation, cargo is free to run the installer's
  build script while `launcher.exe` is still linking, because there is no
  dependency edge to order them (binary-artifact dependencies are still
  unstable). Leaving it out makes the correct order the natural one instead of a
  race that usually goes your way.
- **`installer/build.rs` verifies the payload it is about to embed** against
  `keys/*.pub`. This is the check that turns a silent failure into a loud one,
  at the one moment both halves are in the same room.

---

## 5. Commands

```
cargo run -p xtask -- release          build and sign all three, in order
cargo run -p xtask -- keygen           make a dev signing key (once per machine)
cargo run -p xtask -- keygen --release make the one release key -> keys/gacasy.pub
cargo run -p xtask -- sign <exe>...    sign in place
cargo run -p xtask -- verify <exe>...  check against keys/gacasy.pub and keys/dev.pub
cargo run -p xtask -- version          show the project version and every crate's
```

`build.ps1` / `build.sh` wrap `release` with the "do you have a key yet" check.

Checking what you just built:

```
cargo run -p xtask -- verify target/release/launcher.exe target/release/listener.exe
```

Or ask a binary what it carries — for a human reading output, never as a trust
decision:

```
target/release/launcher.exe --signature
target/release/launcher.exe --version
```

### Environment variables

| Variable | Effect |
|---|---|
| `GACASY_SIGNING_KEY` | Path to the secret key. Overrides `~/.gacasy/gacasy.key`. |
| `GACASY_SIGNING_PASSWORD` | Unlocks an encrypted key. **Not needed for the default unencrypted key.** |
| `GACASY_LAUNCHER_EXE` / `GACASY_LISTENER_EXE` | Where the installer finds its payload. |
| `GACASY_PAYLOAD_OPTIONAL=1` | Build the installer with an empty payload, for UI work. The result refuses to install anything. |

All of these can also live in a `.env` at the repo root (gitignored). Real
environment variables win over `.env`. The `.env` file is entirely optional —
a default setup needs none of this.

---

## 6. Using the same identity on two machines

Copying `keys/dev.pub` is **not** enough. That is only the public half: it tells
a listener what to trust, but gives the machine nothing to sign *with*.

What you copy is the secret key:

1. On machine A, take `~/.gacasy/gacasy.key`
   (`%USERPROFILE%\.gacasy\gacasy.key`).
2. Move it to machine B over a channel you trust — not the git repo, not a
   pastebin, not chat. A password manager's secure-file field or an encrypted
   USB stick is the right shape of thing.
3. Put it at the same path on machine B, or anywhere you like with
   `GACASY_SIGNING_KEY` pointing at it.
4. Copy `keys/dev.pub` across too, so machine B's listener trusts the key and
   `installer/build.rs` can verify the payload.
5. Do **not** run `keygen` on machine B — that mints a *different* identity. It
   will refuse to overwrite an existing key anyway.

If the key was created with `GACASY_SIGNING_PASSWORD`, machine B needs the same
password.

> There is currently no `xtask` command to re-derive `dev.pub` from an existing
> secret key, so step 4 means copying the file rather than regenerating it.

---

## 7. Troubleshooting

**`no trusted public key: neither keys/gacasy.pub nor keys/dev.pub exists`**
Fresh clone, no key yet. Run `cargo run -p xtask -- keygen`, or just use
`build.ps1` / `build.sh`, which does it for you.

**`payload: target/release/launcher.exe is missing`**
You ran `cargo build --release -p installer` before building launcher and
listener. Use `xtask release`.

**`launcher.exe is not signed — build with cargo run -p xtask -- release`**
The binaries exist but stage 2 never happened, usually from a plain
`cargo build --release`. A bare cargo build *always* leaves them unsigned; the
signing step is xtask's. Re-run `xtask release`.

**`is signed by a key none of keys/*.pub names`**
The binaries were signed with a secret key whose public half is not in `keys/`.
Usually means `dev.pub` was deleted or you swapped in a different secret key.
Restore the matching `.pub`.

**A cartridge does nothing when plugged in.**
Verify the launcher on the cartridge itself:
`cargo run -p xtask -- verify E:\launcher.exe`. Also check the launcher was not
rebuilt *after* being signed — a plain `cargo build` overwrites
`target/release/launcher.exe` and strips the block.

**A folder opens instead of (or on top of) the launcher.**
Not a signing problem — that is Windows AutoPlay, and it fires off the same
device event the listener hears. Tick *"Stop Windows opening a folder when a
cartridge is plugged in"* on the installer's listener screen, or set it by hand
in Settings → Bluetooth & devices → AutoPlay → **Removable drive → Take no
action**. The value behind both is the `(Default)` of
`HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers\UserChosenExecuteHandlers\StorageOnArrival`,
which the installer sets to `MSTakeNoAction` and puts back on uninstall. It
applies to every removable drive on that account, not only cartridges, which is
why it is asked for rather than assumed — see `installer/src/autoplay.rs`.

**The listener refuses a launcher whose signature is fine.**
Check the project version. `[workspace.metadata.gacasy] project_version` is the
compatibility contract: the listener asks a *verified* launcher for its version
and refuses one whose major differs from its own. `xtask version` shows the lot;
`xtask release` refuses to ship a set whose majors have drifted apart.

---

## 8. Before publishing anything

- `keys/dev.pub` must **not** be in a published build. A listener built with it
  present trusts your local key, and shipping that asks every user's machine to
  trust it too. `xtask release` prints a warning whenever `dev.pub` exists.
- The release build should be signed with the release key and `keys/gacasy.pub`
  should be the only anchor compiled in.
- `xtask release` verifies everything it just signed before declaring success.
  That is not ceremony: it is the only thing that proves the secret key in use
  actually corresponds to a public key baked into the listener you just built.
  If it does not, every cartridge from that release would be refused, and this is
  where you find out.
