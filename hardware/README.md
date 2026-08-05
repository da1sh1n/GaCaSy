# Romzeta Hardware

The cartridge shell. A Romzeta cartridge is any storage volume with a signed `launcher.exe` at
its root — the enclosure here is one physical form of that, not a requirement of the format.

## Files

- `nvme_enclosure.FCStd` — the FreeCAD model. Three bodies plus the reference board.
- `PH80-583S.step` — the NVMe adapter board, imported as the fit reference. Not ours.
- `dimensions.avif` — the board's published dimensions, kept so the sketch constraints can be
  checked against something.
- `*.FCBak` — FreeCAD's own backup, written beside the document it belongs to.

## Status

First version only. **The front face and the chamfers are not modelled**, so nothing here has
been printed or test-fitted yet.
