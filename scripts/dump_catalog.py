"""Dump the model catalog of a .hxf to JSON (local analysis only; output stays in work/).

Entry 3 gives each model's parameters; Entry 0's Static Model Table (Table 1) maps the
model's name string to the preset_id that preset files store (block key 25).
"""
import json
import os
import struct
import sys

sys.path.insert(0, sys.argv[1])
import patch_amp  # noqa: E402


def table1(e0):
    segs = patch_amp._parse_e0_segments(e0)
    dev = patch_amp.E0_DEVICES[segs[6][3]]

    def vread(va, ln):
        fo = patch_amp._e0_vaddr_to_foff(segs, va, ln)
        return bytes(e0[fo: fo + ln])

    base = struct.unpack('<I', vread(dev.TAB1_BASE_PTRS[0], 4))[0]
    _, count = patch_amp._decode_thumb_movw(vread(dev.TAB1_CNT_INSN, 4))
    raw = vread(base, 12 * count)
    rows = [struct.unpack_from('<IHHI', raw, 12 * i) for i in range(count)]
    return {name_idx: dict(preset_id=pid, dsp_cost=cost) for name_idx, pid, _, cost in rows}


def main(hxf_path, out_path):
    l6, _ = patch_amp.load_l6ia_from_any(hxf_path)
    info = patch_amp.parse_entry3_full(patch_amp.get_entry_swapped(l6, 3))
    t1 = table1(bytearray(patch_amp.get_entry_swapped(l6, 0)))
    models = []
    for m in info['models']:
        row = t1.get(m['words'][2], {})
        models.append(dict(
            idx=m['idx'],
            model_id=m['model_id'],
            preset_id=row.get('preset_id'),
            dsp_cost=row.get('dsp_cost'),
            name=m['name'],
            displayName=m['displayName'],
            blob_index=m['blob_index'],
            meta=dict(m['meta']),
            params=[{k: p[k] for k in ('name', 'type', 'slot', 'min', 'max', 'default', 'displayName', 'displayNameShort', 'meta')}
                    for p in m['params']],
        ))
    with open(out_path, 'w', encoding='utf-8') as f:
        json.dump(dict(header=list(info['header']), n_strings=len(info['strings']), table1_rows=len(t1), models=models),
                  f, indent=1, ensure_ascii=False)
    mapped = sum(1 for m in models if m['preset_id'] is not None)
    print(f'{os.path.basename(hxf_path)}: {len(models)} models ({mapped} with a preset_id), '
          f'{len(t1)} Table 1 rows -> {out_path}')


if __name__ == '__main__':
    main(*sys.argv[2:4])
