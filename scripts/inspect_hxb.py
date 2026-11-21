"""Inspect an HX Edit .hxb backup: header, every zlib stream, and a summary of its JSON."""
import json
import sys
import zlib


def streams(raw):
    pos = 0
    while pos < len(raw):
        i = raw.find(b'\x78', pos)
        if i < 0:
            return
        d = zlib.decompressobj()
        try:
            out = d.decompress(raw[i:])
        except zlib.error:
            pos = i + 1
            continue
        if not d.eof:
            pos = i + 1
            continue
        used = len(raw) - i - len(d.unused_data)
        yield i, used, out
        pos = i + used


def main(path, dump_dir=None):
    raw = open(path, 'rb').read()
    print(f'{path}: {len(raw)} bytes, head {raw[:32].hex(" ")}')
    for n, (off, used, out) in enumerate(streams(raw)):
        try:
            j = json.loads(out)
            keys = list(j.keys())[:8] if isinstance(j, dict) else type(j).__name__
            name = j.get('data', {}).get('meta', {}).get('name') if isinstance(j, dict) else None
            print(f'  #{n:3d} @{off:#07x} z={used:6d} raw={len(out):6d} json keys={keys} name={name!r}')
        except ValueError:
            print(f'  #{n:3d} @{off:#07x} z={used:6d} raw={len(out):6d} non-json {out[:40]!r}')
        if dump_dir:
            open(f'{dump_dir}/{n:03d}.json', 'wb').write(out)


if __name__ == '__main__':
    main(*sys.argv[1:3])
