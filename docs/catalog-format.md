# hxs-catalog.json (format 1)

The model catalog hxtra-editor reads: the models and parameters of the firmware running on the
unit, extracted from the user's own `.hxf` by `tools/catalog/hxs_catalog.py`. It is meant to be made
where the user builds HXtra (its setup page runs the same Python tools in the browser) and to ship
next to the editor's executable. **It contains data from Line 6's firmware: it is never committed
to this repository, attached to a release or shared.**

## Where the editor looks for it

1. `--catalog FILE`
2. the `HXS_CATALOG` environment variable
3. `hxs-catalog.json` next to the executable (the bundle the setup page hands out)
4. `hxs-catalog.json` in the user's data folder (`%APPDATA%\hxtra-editor\` on Windows)

Without a catalog the editor still works; it shows model and parameter numbers instead of names.

## Top level

| key | meaning |
|---|---|
| `format` | `"hxs-catalog"` |
| `version` | `1`; a reader refuses other versions |
| `generator` | `{tool, version}` of the extractor |
| `source` | where it came from (below) |
| `models` | every model a preset can name (below), by `preset_id` |
| `browser` | the unit's model browser: folders in its order (below) |

### `source`

| key | meaning |
|---|---|
| `md5` | MD5 of the `.hxf` (or L6IA) read |
| `product` | `"HX Stomp"` |
| `device_id` | the `.hxf` HEAD's device word (`0x210006` for the HX Stomp; HX Edit's backups call it `device`) |
| `firmware` | the version as presets store it (`0x03800000` = 3.80.0), when the stock file is known; else `null` |
| `stock_md5` | MD5 of the stock file it was built from, when known |
| `hxtra` | `null` for a stock file; else `{features}`: the build flags (e.g. `["HX_BPM", "HX_GR"]`) or `null` when not given |

The editor compares `firmware` with the version in the unit's preset files.

### `models[]`

| key | meaning |
|---|---|
| `preset_id` | the number a preset file stores for the block (block body key 25) |
| `model_id` | its index in the firmware's model table |
| `name` | the internal name (`HD2_AmpEssexA30`) |
| `title` | the title **the unit shows** (model browser / Edit view). With HXtra's "real names" option this is the real-world name |
| `stock_title` | Line 6's title (`Essex A30`) |
| `real_name` | HXtra's real-world name for it (`Vox AC-30 TB`), whether or not the build uses them; `null` for Line 6 originals |
| `category`, `subcategory` | e.g. `amp`/`guitar`, `delay`/`stereo` |
| `legacy` | `true` for Legacy models, else `null` |
| `dsp_cost` | the firmware's DSP cost figure |
| `hxtra_added` | `true` for a model HXtra adds (NAM A2 Lite, Metronome); `false` for stock models; absent when the stock file was not given |
| `params` | the parameters, in the order a preset stores their values (below) |

### `models[].params[]`

| key | meaning |
|---|---|
| `index` | position in the block's value list (block body key 11, its key 4) |
| `slot` | the firmware's parameter slot |
| `name`, `title`, `short_title` | internal name, display name, short display name |
| `type` | `float`, `int`, `list` (an int with `choices`), `bool` or `data` (internal, not a knob) |
| `min`, `max`, `default` | in the parameter's own units; ints and bools as integers |
| `unit` | e.g. `sec`, `db`, `hz`, or `null` |
| `display` | the firmware's display type (`knob`, `percent`, `db`, `time`, `list`, ...) |
| `choices` | `[{value, label}]` for lists |
| `visible` | `false` for hidden helpers (tempo-sync companions, internal data) |
| `sync` | `true` for a time that can follow the tempo |
| `hxtra_added` | `true` for a parameter HXtra adds to a stock model (the FX Loop gate, the GR meter's knobs, the Noise Gate's sidechain) |

### `browser[]`

`{category, category_index, folder, folder_index, items: [{title, kind, model, cab}]}` where `model`
and `cab` are `preset_id`s (`cab` for Amp+Cab items).

## Making one

```
python tools/catalog/hxs_catalog.py HXTRA/tools HXtra.hxf hxs-catalog.json --stock HX_Stomp_v3.80.0_0x103966115.hxf [--features HX_BPM,...]
```

`HXTRA/tools` is the `tools` folder of an HXtra checkout (the extractor uses its firmware readers).
