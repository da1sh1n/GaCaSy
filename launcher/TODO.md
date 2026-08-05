# GaCaSy Launcher — TODO

The launcher carries no secret and verifies nothing. Its signature *is* the cartridge's
identity, and it is read rather than asked — see
[`structure.md`](structure.md#role-in-cartridge-identification).

Launching, feedback, the gallery, ordering and the Departure Mono type grid are all built;
[`structure.md`](structure.md) describes what shipped.

## Type

The chrome is all one size. The next step up the 11px grid is 22px, which is name-plate
territory, so hierarchy in the toolbar has to come from the colour ramp and the pill fill
rather than from a size in between.

- [ ] **The name line lost its size at the default `border_gap`.** It was 16.2px; on the grid
      the gap at `border_gap = 36` only holds one 11px step, so it now matches the toolbar
      instead of reading as a plate. Two steps needs `border_gap >= 43`. Worth deciding
      whether to raise the seed default — it widens the window's margins, which is a look
      change beyond the type, so it is left alone rather than changed quietly.
