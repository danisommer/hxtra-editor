#!/usr/bin/env python3
"""hxs_catalog.py - the model catalog of an HX Stomp firmware file, for hxtra-editor.

Reads the user's own .hxf (stock or HXtra-built) or the L6IA archive inside it and writes
hxs-catalog.json (see docs/catalog-format.md). Nothing from the firmware is kept anywhere else:
the output is for the user's own installation, never for redistribution.

It runs on HXtra's own tools, as HXtra's setup page runs build.py in the browser (Pyodide):
  hxs_catalog.py HXTRA_TOOLS FIRMWARE OUT.json [--stock STOCK.hxf] [--features HX_BPM,HX_GR,...]

--stock     the stock .hxf the firmware was built from: models and parameters it lacks are marked
            as HXtra's, and its MD5 names the firmware version.
--features  the HXtra build flags the firmware was built with, recorded as they are.
"""
import argparse
import hashlib
import json
import os
import struct
import sys

FORMAT = "hxs-catalog"
FORMAT_VERSION = 1
GENERATOR_VERSION = "1"

# Stock files this tool knows, by MD5: product, device ID (the .hxf HEAD word at 8), firmware (as presets store it).
KNOWN_STOCK = {
    "a2cd8f4ae56f49415380270ebc484bf1": ("HX Stomp", 0x210006, 0x03800000),
}
# HX Stomp 3.80.0, Entry 0: the model browser's categories, 0x2c-byte records {name[0x11], ...}
# (docs/research/m4/model-browser.md, read through 0x28d9e048).
BROWSER_CATEGORIES = {"HX Stomp": (0x28e285d4, 19)}
CATEGORY_RECORD = 0x2C
NAME_LEN = 0x11
ITEM_LEN = 48
TABLE2_ROW = 44
TYPES = {1: "bool", 2: "int", 3: "float", 6: "data"}
# What a model really is when its firmware category says otherwise, and its parameters' roles.
# HXtra's NAM A2 Lite sits in the distortion category; its Model knob picks the User IR slot
# that holds the converted .nam capture (HXtra tools/effects.py), 1..128.
ROLES = {
    "HD2_DistNAMLite": ("capture", {"Model": "capture_slot"}),
}
# The .hxf HEAD chunk's data starts at byte 20; its third word is the device ID (0x210006, the HX Stomp).
HEAD_DEVICE_ID = 20 + 8


def bits_int(f):
    return struct.unpack("<i", struct.pack("<f", f))[0]


def cstr(b):
    return b.split(b"\0", 1)[0].decode("latin1")


def md5(path):
    return hashlib.md5(open(path, "rb").read()).hexdigest()


class Entry0:
    """Virtual-address reads in the M4 application, through patch_amp's segment map."""

    def __init__(self, pa, l6):
        self.pa = pa
        self.e0 = bytearray(pa.get_entry_swapped(l6, 0))
        self.segs = pa._parse_e0_segments(self.e0)
        self.dev = pa.e0_device(self.segs)

    def read(self, va, n):
        fo = self.pa._e0_vaddr_to_foff(self.segs, va, n)
        return bytes(self.e0[fo: fo + n])

    def u32(self, va):
        return struct.unpack("<I", self.read(va, 4))[0]

    def movw(self, va):
        return self.pa._decode_thumb_movw(self.read(va, 4))[1]

    def table1(self):
        """name index -> (preset_id, dsp_cost)."""
        base, count = self.u32(self.dev.TAB1_BASE_PTRS[0]), self.movw(self.dev.TAB1_CNT_INSN)
        raw = self.read(base, 12 * count)
        return {n: (pid, cost) for n, pid, _, cost in (struct.unpack_from("<IHHI", raw, 12 * i) for i in range(count))}

    def table2(self):
        """name index -> the 16-byte short title the Edit view shows."""
        base, count = self.u32(self.dev.TAB2_BASE_PTR), self.movw(self.dev.TAB2_CNT_INSN)
        raw = self.read(base, TABLE2_ROW * count)
        return {struct.unpack_from("<I", raw, TABLE2_ROW * i)[0]: cstr(raw[TABLE2_ROW * i + 4: TABLE2_ROW * i + 20]) for i in range(count)}

    def browser(self, name_to_pid):
        """The model browser: categories, their folders and items, in the unit's order."""
        cats = BROWSER_CATEGORIES.get(self.dev.NAME)
        cat_names = [cstr(self.read(cats[0] + CATEGORY_RECORD * c, NAME_LEN)) for c in range(cats[1])] if cats else []
        out = []
        for slot in range(self.pa.E0_BLIST_SLOTS):
            cat, sub = divmod(slot, 4)
            ptr, count = self.u32(self.dev.BLIST_PTR_TAB + 4 * slot), self.u32(self.dev.BLIST_CNT_TAB + 4 * slot)
            if not ptr or not count:
                continue
            raw = self.read(ptr, ITEM_LEN * count)
            items = []
            for k in range(count):
                item = raw[ITEM_LEN * k: ITEM_LEN * (k + 1)]
                kind, name_idx, cab_idx, _ = struct.unpack_from("<IIiH", item, 0)
                items.append({
                    "title": cstr(item[14:14 + NAME_LEN]),
                    "kind": kind,
                    "model": name_to_pid.get(name_idx),
                    "cab": name_to_pid.get(cab_idx) if cab_idx >= 0 else None,
                })
            out.append({
                "category": cat_names[cat] if cat < len(cat_names) else None,
                "category_index": cat,
                "folder": cstr(self.read(self.dev.SUBCAT_NAMES + 104 * cat + 26 * sub, 26)),
                "folder_index": sub,
                "items": items,
            })
        return out


def parameter(p, rank, visible_count):
    kind = TYPES.get(p["type"], "unknown")
    meta = p["meta"]
    as_number = (lambda v: bits_int(v)) if kind in ("int", "bool") else (lambda v: v)
    choices = sorted(
        ({"value": int(k), "label": v} for k, v in meta.items() if k.isdigit()),
        key=lambda c: c["value"],
    )
    if kind == "bool" and choices == [] and ("true" in meta or "false" in meta):
        choices = [{"value": 0, "label": meta.get("false", "Off")}, {"value": 1, "label": meta.get("true", "On")}]
    display = meta.get("displayType")
    lo, hi = (0, 1) if kind == "bool" else (as_number(p["min"]), as_number(p["max"]))
    return {
        "index": rank,
        "slot": p["slot"],
        "name": p["name"],
        "title": meta.get("displayName") or p["name"],
        "short_title": meta.get("displayNameShort") or None,
        "type": "list" if kind == "int" and display in ("list", "List", "dynamicList") else kind,
        "min": lo,
        "max": hi,
        "default": as_number(p["default"]),
        "unit": meta.get("units"),
        "display": display,
        "choices": choices or None,
        "visible": kind != "data" and (visible_count is None or rank < visible_count),
        "sync": meta.get("sync") == "1",
    }


def catalog(pa, path, real_names):
    l6, _ = pa.load_l6ia_from_any(path)
    e3 = pa.parse_entry3_full(pa.get_entry_swapped(l6, 3))
    e0 = Entry0(pa, l6)
    t1, t2 = e0.table1(), e0.table2()
    name_to_pid = {n: pid for n, (pid, _) in t1.items()}
    models = []
    for m in e3["models"]:
        name_idx = m["words"][2]
        if name_idx not in t1:
            continue
        pid, cost = t1[name_idx]
        params = sorted(m["params"], key=lambda p: p["slot"])
        visible = m["meta"].get("displayParamCount")
        visible = int(visible) if visible and visible.isdigit() else None
        models.append({
            "preset_id": pid,
            "model_id": m["model_id"],
            "name": m["name"],
            "title": t2.get(name_idx) or m["displayName"],
            "stock_title": m["displayName"] or None,
            "real_name": real_names.get(m["displayName"]),
            "category": m["meta"].get("category"),
            "subcategory": m["meta"].get("subcategory"),
            "legacy": m["meta"].get("legacy") == "1" or None,
            "dsp_cost": cost,
            "params": [parameter(p, rank, visible) for rank, p in enumerate(params)],
        })
    models.sort(key=lambda m: m["preset_id"])
    return models, e0.browser(name_to_pid), e0.dev.NAME


def mark_hxtra(models, stock_models):
    by_name = {m["name"]: m for m in stock_models}
    for m in models:
        stock = by_name.get(m["name"])
        m["hxtra_added"] = stock is None
        stock_params = {p["name"] for p in stock["params"]} if stock else set()
        for p in m["params"]:
            p["hxtra_added"] = stock is not None and p["name"] not in stock_params


def mark_roles(models):
    for m in models:
        role, param_roles = ROLES.get(m["name"], (None, {}))
        m["role"] = role
        for p in m["params"]:
            p["role"] = param_roles.get(p["name"]) or param_roles.get(p["title"])


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("hxtra_tools")
    ap.add_argument("firmware")
    ap.add_argument("out")
    ap.add_argument("--stock")
    ap.add_argument("--features", default=None)
    a = ap.parse_args(argv)
    sys.path.insert(0, a.hxtra_tools)
    import patch_amp as pa
    try:
        from features import real_names
        real = dict(real_names.BY_TITLE)
    except ImportError:
        real = {}

    models, browser, product = catalog(pa, a.firmware, real)
    file_md5 = md5(a.firmware)
    stock_md5 = md5(a.stock) if a.stock else (file_md5 if file_md5 in KNOWN_STOCK else None)
    known = KNOWN_STOCK.get(stock_md5)
    if a.stock:
        stock_models, _, _ = catalog(pa, a.stock, real)
        mark_hxtra(models, stock_models)
    mark_roles(models)
    head = open(a.firmware, "rb").read(64)
    is_hxf = head[:4] == b"FORM" and head[12:16] == b"HEAD"
    device_id = struct.unpack_from(">I", head, HEAD_DEVICE_ID)[0] if is_hxf else (known[1] if known else None)
    out = {
        "format": FORMAT,
        "version": FORMAT_VERSION,
        "generator": {"tool": "hxs_catalog.py", "version": GENERATOR_VERSION},
        "source": {
            "md5": file_md5,
            "product": product,
            "device_id": device_id,
            "firmware": known[2] if known else None,
            "stock_md5": stock_md5,
            "hxtra": {"features": a.features.split(",") if a.features else None} if a.stock or a.features else None,
        },
        "models": models,
        "browser": browser,
    }
    with open(a.out, "w", encoding="utf-8") as f:
        json.dump(out, f, ensure_ascii=False, indent=1)
    added = sum(1 for m in models if m.get("hxtra_added"))
    added_params = sum(1 for m in models for p in m["params"] if p.get("hxtra_added"))
    print(f"{os.path.basename(a.firmware)}: {len(models)} models ({added} added by HXtra, {added_params} parameters added), "
          f"{len(browser)} browser folders, {sum(1 for m in models if m['real_name'])} real names -> {a.out}")


if __name__ == "__main__":
    main()
