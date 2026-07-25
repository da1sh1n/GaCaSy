# GaCaSy Launcher — TODO

Nothing queued right now. The launcher carries no cartridge key and writes no `.cartridge`
marker — that contract lives between the [installer](../installer/structure.md) and the
[listener](../listener/structure.md).

Its one remaining item is in the [`structure.md`](structure.md#status) status list: **v2
code-signing of `launcher.exe`**, after which the exe's signature becomes the identity and
`.cartridge` is retired.
