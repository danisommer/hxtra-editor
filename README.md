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
| `hxs inspect <file.l6p> [--hidden]` | sections of a preset file, its blocks (named with a catalog) and its six snapshots | no (offline) |
| `hxs show SLOT [--hidden]` | a stored preset's blocks with model names and each parameter's value as the unit shows it, then its snapshots | no |
| `hxs catalog [SEARCH]` | the model catalog in use, or the models whose names contain SEARCH | no (offline) |
| `hxs restore-preset <file.l6p> --slot N --name NAME` | dry run by default; with `--yes` writes the file into the slot and reads it back to compare byte for byte | yes, one slot |
| `hxs restore-globals <backup>` | dry run by default; with `--yes` writes the global settings with their original bytes (HX Edit's restore turns some ints into floats and rounds floats) and reads them back | yes |
| `hxs monitor [--seconds N] [--all] [--midi-channel N]` | keeps the connection open and prints the unit's events live (settings, parameters, snapshots with their bank when known, preset loads, unknown notifications); accepts `select SLOT`, `set ID VALUE`, `param BLOCK PARAM VALUE` (with a catalog PARAM can be a name and VALUE is typed and range-checked), `block BLOCK on\|off`, `snapshot 1-6` (over MIDI CC 69, which reaches HXtra's snapshots 4–6) and `quit` | only what you type; nothing is saved to a preset |
| `hxs restore <backup>` | checks every backup file's SHA-256, lists only what differs from the unit (IRs, presets, favorites, global settings); with `--yes` writes that list and compares again | yes, what differs |

Close HX Edit first: it holds the unit's USB interface while it runs.

`hxtra-editor` (crate `hxs-gui`) is a first, plain window over the same functions: the preset
list, the current preset's blocks with every parameter (NAM captures pick their IR slot by
name), snapshots 1–6, save, backup, restore with a comparison first, writing one preset file,
IRs, the catalog and the global settings. Every write to the unit asks for a confirmation box
or is an explicit edit.

### Model catalog

Model and parameter names come from `hxs-catalog.json`, extracted from your own
firmware file by `tools/catalog/hxs_catalog.py` (format in `docs/catalog-format.md`).
One build of `hxs` reads catalogs of stock and HXtra firmware alike: HXtra's real-world
names, added models and added parameters are marked from the catalog's data
(`--names unit|line6|real` picks which names to print). `hxs` looks for it in this
order: `--catalog FILE`, `$HXS_CATALOG`, next to the executable, then
`%APPDATA%\hxtra-editor\`. Without one everything works with numbers for names; a
catalog made for another firmware version than the unit's is not used.

No catalog is ever shipped: it holds Line 6 data. The page in `web/` (published to GitHub Pages
by `.github/workflows/pages.yml`) extracts it from your `.hxf` in the browser with Pyodide,
running this repository's extractor on HXtra's firmware tools at a pinned commit; nothing is
uploaded. To try it locally:

```
python web/build_site.py PATH_TO_HXTRA_CHECKOUT work/site
python -m http.server -d work/site 8000
```

then open http://localhost:8000.

## Build

Rust stable (MSVC toolchain on Windows), then:

```
cargo build --release
cargo test
```

The device is reached through its WinUSB interface with [nusb](https://crates.io/crates/nusb); no driver change is needed on Windows.

## Layout

- `crates/hxs-core`: USB link, transport, streams, sessions, MessagePack messages, the editor and librarian commands, preset files and blocks, snapshots, backups, the model catalog.
- `crates/hxs-cli`: the `hxs` command.
- `tools/catalog/`: the catalog extractor (runs HXtra's firmware tools on your `.hxf`).
- `scripts/`: USB capture (USBPcap) and decoding helpers used to study the protocol.
- `docs/findings.md`: research notes (Portuguese), each claim tagged as verified or hypothesis.

## Credits and license

The protocol notes build on the research published by the HXtra project and are cross-checked against the protocol notes of [tonepush](https://github.com/crmne/tonepush) (MIT). Licensed under GPL-3.0-or-later (see `LICENSE`).
