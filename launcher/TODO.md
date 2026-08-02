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
- [x] A game whose exe isn't on the cartridge is marked unplayable at startup — veiled
      cover, sign over it, not clickable.
- [x] A game that was chosen and failed to start keeps its place in the row with a border
      and a short message, and stays clickable to retry.
- [x] Config knobs for all of it: `overlay_color`, `loading_ring_color`,
      `loading_text_color`, `error_border_color`, `error_border_width`, `error_text_color`,
      `missing_sign_color`, `missing_dim`. (`loading_ring_segments` and
      `loading_ring_speed` went with the spinning ring; `loading_ring_color` keeps its name
      and colours the progress line that replaced it, so a cartridge that set it doesn't
      silently revert.)
- [x] Console games open a console window when launched. Suppressed with `CREATE_NO_WINDOW`
      by default; `show_console_window` in `config.toml` brings it back if that's ever
      wanted.
- [x] `config.toml` is only ever written when missing, so a cartridge set up before a
      knob existed never saw that knob. `config::sync_defaults()` now appends a
      commented-out, already-in-effect `# key = default` line (with a short description)
      for any known setting missing from the file, every startup — nothing about current
      behavior changes, but the knob is there to find and uncomment.

## The gallery

The row used to give every cover `1/n` of the window width, so a cartridge with a lot of games
shrank all of them until none could be read. It no longer does. Details in
[`structure.md`](structure.md#the-gallery).

- [x] Covers are one size, set by the window height and never by the game count. A row too wide
      for the window scrolls sideways instead — wheel, arrow keys, or the bar drawn under it.
- [x] The window is never narrower than three covers, so 0/1/2 games all open the same window
      and the toolbar always has room (`MIN_VISIBLE_COVERS`).
- [x] A search box, at the right of the toolbar, shown only once the row actually overflows.
- [x] Four cover orders in a segmented control at the left of the toolbar — `usage` (last
      opened first), `alphabetic`, `catalog`, `user` — kept in `order_mode` in `config.toml`.
      It was a native `<select>`; the OS-drawn popup was the loudest "this is a web page"
      moment in an otherwise bespoke window.
- [x] `user` order can be arranged by hand: a toggle beside the order control puts a grip on
      each cover and lets them be dragged, with the row auto-scrolling at the edges.
- [x] The selected cover's name on one shared line under the row (`show_captions`), rather
      than a caption per card.
- [x] The launcher writes `config.toml` for the first time — `order_mode`, `usage_order` and
      `user_order` only, edited in place with `toml_edit` so every comment in the file survives.
      `usage_order` moves a game to the front when it is *confirmed up*, not when it is clicked.
- [x] `cargo run`'s config mirror carries those three keys across, so the seed still owns look
      and feel while the launcher owns the order in dev as well as on a cartridge.
- [x] The window has rounded corners (`window_corner_radius`), asked of Windows 11 and clipped
      by hand on Windows 10. Drawing them in CSS instead was tried and doesn't work — see
      [`structure.md`](structure.md#how-it-runs) for why, before anyone tries it again.

## The typeface

- [ ] **BackOut is the wrong face for this and needs replacing.** It came in with the
      redesign and it is a single-weight *display* face — the right instinct for the name
      plate under the row, the wrong one for everything else it is currently doing.
      `style.css` sets it on `body`, so it also renders the order segments, the search
      placeholder, the status line and the error text: sizes it was never drawn for, with
      no bold available to mark anything with (which is why the active order segment is
      marked with a pill fill rather than weight) and extra tracking bolted on to keep the
      small labels legible. Its Latin-only coverage means a non-Latin game name silently
      falls through to Segoe UI mid-row.
      What it should become: a proper text face for the chrome, and *at most* a display
      face kept for the name line alone. Whatever replaces it has to stay OFL or similarly
      free — the licence ships on the cartridge (`licenses/`) — and stay embedded rather
      than shipped in `output/`, for the reasons in
      [`structure.md`](structure.md#source-layout).

## v2

- [x] Code-signing of `launcher.exe`, after which the exe's signature becomes the identity
      and `.cartridge` is retired. See [`structure.md`](structure.md#status) — the
      launcher's only remaining identity work.
