# Romzeta Installer — TODO

The installer specced in [`structure.md`](structure.md) is **built** and has been exercised end
to end on real media. What is left is the Linux target.

## Future

- [ ] Linux target: `/opt` or `~/.local/share` instead of Program Files, a **udev rule**
      instead of a Run key (not a systemd user service — nothing runs between connections),
      Linux launcher binary instead of `launcher.exe`. `volume.rs` and `listener.rs` are the
      two modules with `#[cfg(windows)]` platform halves; everything else is portable
      already.
