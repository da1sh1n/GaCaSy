# GaCaSy Listener — TODO

Actionable list for the service specced in [`structure.md`](structure.md).
**The shared core and the Windows trigger are built and verified on real hardware; Linux is
not started.**

The two platforms differ in **process lifetime**, not just in which API detects a volume —
Windows stays resident from login to logout, Linux runs one-shot from udev and exits. So the
build splits into one shared core plus two unrelated triggers. See
[Execution models](structure.md#execution-models) before starting the Linux trigger.

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
