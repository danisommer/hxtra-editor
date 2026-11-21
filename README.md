# hxtra-editor

An independent, open-source editor toolkit for the Line 6 HX Stomp running the
[HXtra](https://github.com/wisemandenny/hxtra) modified firmware. It talks to the
unit over USB the way HX Edit does, and it keeps what HX Edit drops: HXtra's
snapshots 4–6 live in each preset file, and HX Edit's exports and backups leave
them out.

Not affiliated with or endorsed by Line 6 or Yamaha. This repository holds no
Line 6 code, firmware or data. Model catalogs are meant to be generated locally
from your own firmware file and never committed.

## Status

Early, command line only, Windows tested (HX Stomp, HXtra v1.2.1 on stock 3.80.0).

| Command | What it does | Writes to the unit |
|---|---|---|
| `hxs info` | device, firmware, setlists, current preset | no |
| `hxs presets` | presets in slot order | no |
| `hxs backup <dir>` | every preset file as stored (with snapshots 4–6), IRs, favorites, globals, plus a manifest with SHA-256 hashes | no |
| `hxs inspect <file.l6p>` | sections of a preset file and its six snapshots | no (offline) |
| `hxs restore-preset <file.l6p> --slot N --name NAME` | dry run by default; with `--yes` writes the file into the slot and reads it back to compare byte for byte | yes, one slot |
| `hxs restore-globals <backup>` | dry run by default; with `--yes` writes the global settings with their original bytes (HX Edit's restore turns some ints into floats and rounds floats) and reads them back | yes |
| `hxs monitor [--seconds N] [--all]` | keeps the connection open and prints the unit's events live (settings, preset loads, unknown notifications); accepts `select SLOT`, `set ID VALUE` (typed as the setting's current value) and `quit` | only when you type `select` or `set` |
| `hxs restore <backup>` | checks every backup file's SHA-256, lists only what differs from the unit (IRs, presets, favorites, global settings); with `--yes` writes that list and compares again | yes, what differs |

Close HX Edit first: it holds the unit's USB interface while it runs.

## Build

Rust stable (MSVC toolchain on Windows), then:

```
cargo build --release
cargo test
```

The device is reached through its WinUSB interface with [nusb](https://crates.io/crates/nusb); no driver change is needed on Windows.

## Layout

- `crates/hxs-core`: USB link, transport, streams, sessions, MessagePack messages, the editor and librarian commands, preset files, snapshots, backups.
- `crates/hxs-cli`: the `hxs` command.
- `scripts/`: USB capture (USBPcap) and decoding helpers used to study the protocol.
- `docs/findings.md`: research notes (Portuguese), each claim tagged as verified or hypothesis.

## Credits and license

The protocol notes build on the research published by the HXtra project and are cross-checked against the protocol notes of [tonepush](https://github.com/crmne/tonepush) (MIT). Licensed under GPL-3.0-or-later (see `LICENSE`).
