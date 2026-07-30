# GaCaSy Launcher — TODO

The launcher carries no cartridge key and writes no `.cartridge` marker — that contract
lives between the [installer](../installer/structure.md) and the
[listener](../listener/structure.md).

## Launching and feedback

Choosing a cover now starts the game, shows that it's starting, and reports it when it
doesn't. Details in [`structure.md`](structure.md#launching-a-game).

- [x] Start the game with its cwd set to the exe's own folder, not the cartridge root.
- [x] Wait for the game's window (`WaitForInputIdle`) on a worker thread, then close the
      launcher — rather than closing the instant `spawn` returns.
- [x] Launch transition: the other covers fade out, the chosen one animates to the centre
      at full size, the screen dims, and a segmented ring spins in the middle with a
      faint status line along the bottom until the game is up.
- [x] `logs/`: every attempt and its full OS error in `launcher.log`, each game's own
      stdout/stderr in `logs/<game>/`.
- [x] A game whose exe isn't on the cartridge is marked unplayable at startup — dimmed
      cover, sign over it, not clickable.
- [x] A game that was chosen and failed to start keeps its place in the row with a border
      and a short message, and stays clickable to retry.
- [x] Config knobs for all of it: `overlay_color`, `loading_ring_color`,
      `loading_text_color`, `loading_ring_segments`, `error_border_color`,
      `error_border_width`, `error_text_color`, `missing_sign_color`, `missing_dim`.
- [x] Console games open a console window when launched. Suppressed with `CREATE_NO_WINDOW`
      by default; `show_console_window` in `config.toml` brings it back if that's ever
      wanted.
- [x] `config.toml` is only ever written when missing, so a cartridge set up before a
      knob existed never saw that knob. `config::sync_defaults()` now appends a
      commented-out, already-in-effect `# key = default` line (with a short description)
      for any known setting missing from the file, every startup — nothing about current
      behavior changes, but the knob is there to find and uncomment.

## v2

- [x] Code-signing of `launcher.exe`, after which the exe's signature becomes the identity
      and `.cartridge` is retired. See [`structure.md`](structure.md#status) — the
      launcher's only remaining identity work.
