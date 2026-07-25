# GaCaSy Listener — PC Side (spec to build)

Part of the two-app **GaCaSy** game-cartridge system. This document covers the
**listener**: the background service on the user's PC that detects a connected
cartridge, verifies it, and auto-starts that cartridge's launcher. The cartridge-side
companion is documented in [`../launcher/structure.md`](../launcher/structure.md); the
user-facing overview is in [`../README.md`](../README.md).

> **This side does not exist yet.** Treat this doc as a *spec to build by*.

## Purpose

A background service on the user's PC that watches for a cartridge being connected and,
once it trusts it, launches that cartridge's launcher automatically — so plugging in a
cartridge "just works" like inserting a console cartridge.

## Shape

- **One Rust codebase**, `cfg`-gated per OS, producing a **Windows build** and a
  **Linux build**. (Matches the launcher's language; single repo, two targets.)
- Runs **in the background at login** (Windows: a startup entry / service; Linux: a
  systemd user service or autostart).

## What counts as a "cartridge"

**Any mounted storage volume.** The reference cartridge is an NVMe behind a USB-C
adapter, but the listener must treat every storage device the same — HDD, SSD, NVMe,
USB stick. So detection is **"a new volume mounted"**, never a specific USB VID/PID.

## Responsibilities / flow

1. Watch for a **new volume mount**.
2. On mount, look for a **`.cartridge`** file at the volume root.
3. If present, read its **key** and compare it to the launcher `config.ini`'s key on the
   same volume (see [Cartridge identification system](#cartridge-identification-system)).
4. If the key is valid, **launch that volume's launcher** (`<volume>/launcher.exe`, or the
   platform's launcher binary).
5. Otherwise, ignore the volume (optionally log why).

## Per-OS detection

- **Windows:** device/volume-arrival events (`WM_DEVICECHANGE` / `DBT_DEVICEARRIVAL`) or
  poll drive letters; resolve the new volume's root path.
- **Linux:** watch mounts (udev, `/proc/mounts`, or a mount-watch crate); resolve the
  mountpoint.

## Cartridge identification system

The contract that links the two apps. The launcher exposes the identity (see
[`../launcher/structure.md`](../launcher/structure.md#role-in-cartridge-identification));
the listener verifies it here.

- The cartridge carries a **`.cartridge`** marker file at the volume root containing a
  **key**.
- The launcher's **`config.ini`** carries the **same key**.
- The listener's decision:

  ```text
  volume mounts  →  has .cartridge?  →  key matches config.ini?  →  launch launcher
                          │ no                 │ no
                          └── ignore ──────────┘
  ```

**Version note.** This shared-key scheme is **v1** — it proves "this looks like a GaCaSy
cartridge" but is not a strong security boundary. **v2** is signature-based trust:
verifying the launcher exe's official code signature (see [Future](#future)). Until then,
the key is a lightweight recognition handshake, not tamper protection.

> Neither `config.ini`'s `key` nor the `.cartridge` file exists in the code yet — both
> are part of this design and are added when the identification system is implemented.

## Future

Once the launcher is officially **code-signed**, augment or replace the key check with
**signature verification** of the launcher exe, so trust is cryptographic rather than a
shared secret.

## Status / roadmap

- [ ] Rust codebase scaffold, `cfg`-gated per OS.
- [ ] Volume-mount detection (Windows + Linux).
- [ ] `.cartridge` + `config.ini` key check.
- [ ] Auto-launch the verified cartridge's launcher.
- [ ] Background-at-login install (startup entry / systemd user service).
- [ ] v2: signature verification of the code-signed launcher.
