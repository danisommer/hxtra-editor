"""Decode a USBPcap capture of HX Edit <-> HX Stomp traffic into service messages.

Layers (as documented by HXtra's hx_dfu_reboot.py / hx_ctl.py):
  USBPcap bulk transfer -> transport packet (8-byte header, padded to 4)
  -> stream packet (flags, seq, window) -> per-stream byte stream
  -> message {u16 dir, u16 service, u32 len} + MessagePack body.

Usage: decode_capture.py <capture.pcap> [--dump DIR] [--bin-min N]
Writes a text log to stdout; with --dump, every binary value >= --bin-min bytes
(default 512) is saved to DIR for inspection.
"""
import argparse
import os
import struct
import sys

import msgpack

LINKTYPE_USBPCAP = 249
BULK = 3
F_NAMES = [(0x01, 'SYN'), (0x02, 'FIN'), (0x04, 'DATA'), (0x08, 'WIN'), (0x10, 'HB'), (0x20, 'HBREQ')]
SERVICES = {6: 'editor', 2: 'librarian', 4: 'notify'}


def read_pcap(path):
    raw = open(path, 'rb').read()
    magic = struct.unpack_from('<I', raw, 0)[0]
    if magic != 0xA1B2C3D4:
        raise SystemExit(f'{path}: not a little-endian classic pcap (magic {magic:#x})')
    linktype = struct.unpack_from('<I', raw, 20)[0]
    if linktype != LINKTYPE_USBPCAP:
        raise SystemExit(f'{path}: link type {linktype}, expected USBPcap ({LINKTYPE_USBPCAP})')
    off, t0 = 24, None
    while off + 16 <= len(raw):
        sec, usec, incl, _ = struct.unpack_from('<IIII', raw, off)
        pkt = raw[off + 16: off + 16 + incl]
        off += 16 + incl
        t = sec + usec / 1e6
        t0 = t if t0 is None else t0
        yield t - t0, pkt


def usb_transfers(path):
    """Yield (time, 'OUT'|'IN', endpoint, data) for bulk transfers that carry data."""
    for t, pkt in read_pcap(path):
        hlen, _irp, _status, _func, info, _bus, _dev, ep, xfer, dlen = struct.unpack_from('<HQIHBHHBBI', pkt, 0)
        if xfer != BULK or dlen == 0:
            continue
        data = pkt[hlen: hlen + dlen]
        completion = info & 1
        if ep & 0x80 and completion:
            yield t, 'IN', ep, data
        elif not ep & 0x80 and not completion:
            yield t, 'OUT', ep, data


def transport_packets(buf):
    off = 0
    while off + 8 <= len(buf):
        w0, w1 = struct.unpack_from('<II', buf, off)
        ttype, ln = w0 >> 28, w0 & 0xFFFF
        body = buf[off + 8: off + 8 + ln]
        pkt = dict(type=ttype, src=w1 & 0xFFFF, dst=(w1 >> 16) & 0xFFFF, flags=None, data=b'')
        p = 4 if ttype == 2 else 0
        if len(body) - p >= 8:
            s0, s1 = struct.unpack_from('<II', body, p)
            pkt.update(flags=s0 >> 24, seq=(s0 >> 8) & 0xFF, window=s1, data=body[p + 8:])
        else:
            pkt['payload'] = body
        yield pkt
        off += (8 + ln + 3) & ~3


def flag_names(f):
    return '+'.join(n for b, n in F_NAMES if f & b) or '0'


def short(v, depth=0):
    if isinstance(v, (bytes, bytearray)):
        if len(v) <= 24:
            try:
                s = bytes(v).rstrip(b'\0').decode('ascii')
                if s.isprintable():
                    return repr(s)
            except UnicodeDecodeError:
                pass
            return '0x' + bytes(v).hex()
        return f'<bin {len(v)}>'
    if isinstance(v, dict):
        if depth > 3:
            return '{…}'
        items = [f'{short(k)}: {short(x, depth + 1)}' for k, x in list(v.items())[:12]]
        return '{' + ', '.join(items) + (', …' if len(v) > 12 else '') + '}'
    if isinstance(v, list):
        if depth > 3:
            return '[…]'
        items = [short(x, depth + 1) for x in v[:8]]
        return '[' + ', '.join(items) + (f', … ({len(v)})' if len(v) > 8 else '') + ']'
    if isinstance(v, int) and v > 9:
        return f'{v:#x}'
    return repr(v)


def walk_bins(v, path=()):
    if isinstance(v, (bytes, bytearray)):
        yield path, bytes(v)
    elif isinstance(v, dict):
        for k, x in v.items():
            yield from walk_bins(x, path + (k,))
    elif isinstance(v, list):
        for i, x in enumerate(v):
            yield from walk_bins(x, path + (i,))


class StreamState:
    def __init__(self, name):
        self.name, self.buf, self.last_seq, self.service = name, b'', None, None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('pcap')
    ap.add_argument('--dump')
    ap.add_argument('--bin-min', type=int, default=512)
    ap.add_argument('--transport', action='store_true', help='also print every transport packet')
    a = ap.parse_args()
    if a.dump:
        os.makedirs(a.dump, exist_ok=True)
    streams, nbin = {}, 0
    for t, d, ep, data in usb_transfers(a.pcap):
        for pkt in transport_packets(data):
            host, dev = (pkt['src'], pkt['dst'] & 0x3FF) if d == 'OUT' else (pkt['dst'], pkt['src'] & 0x3FF)
            key = (d, host, dev)
            st = streams.setdefault(key, StreamState(f'{host:#06x}<->{dev:#05x}'))
            if pkt['flags'] is None:
                print(f'{t:9.4f} {d:3} {st.name} non-stream type {pkt["type"]} {pkt.get("payload", b"").hex()}')
                continue
            if a.transport or pkt['flags'] & 0x03:
                print(f'{t:9.4f} {d:3} {st.name} {flag_names(pkt["flags"])} seq {pkt["seq"]} win {pkt["window"]:#x} data {len(pkt["data"])}')
            if not pkt['flags'] & 0x04 or not pkt['data']:
                continue
            if pkt['seq'] == st.last_seq:
                continue
            st.last_seq = pkt['seq']
            st.buf += pkt['data']
            while len(st.buf) >= 8:
                w_dir, svc, ln = struct.unpack_from('<HHI', st.buf, 0)
                if len(st.buf) < 8 + ln:
                    break
                body, st.buf = st.buf[8: 8 + ln], st.buf[8 + ln:]
                if ln == 1:
                    st.service = svc or body[0]
                    print(f'{t:9.4f} {d:3} {st.name} select service {body[0]} ({SERVICES.get(body[0], "?")})')
                    continue
                svc_name = SERVICES.get(svc, str(svc))
                try:
                    msg = msgpack.unpackb(body, raw=True, strict_map_key=False)
                except Exception as e:
                    print(f'{t:9.4f} {d:3} {st.name} [{svc_name}] {ln} bytes, not msgpack ({e}): {body[:48].hex()}')
                    continue
                if isinstance(msg, dict) and 0x64 in msg:
                    head = f'REQ seq {msg.get(0x66)} cmd {msg[0x64]:#04x}'
                    rest = {k: v for k, v in msg.items() if k not in (0x64, 0x66)}
                elif isinstance(msg, dict) and 0x67 in msg:
                    head = f'REP seq {msg.get(0x66)} status {msg[0x67]}'
                    rest = msg.get(0x68)
                else:
                    head, rest = 'MSG', msg
                print(f'{t:9.4f} {d:3} {st.name} [{svc_name}] {head} ({ln} B) {short(rest)}')
                for path, b in walk_bins(msg):
                    if len(b) >= a.bin_min:
                        tag = ' HX6B@%d' % b.find(b'HX6B') if b'HX6B' in b else ''
                        magic = b[:12]
                        print(f'{"":9} {"":3} {"":17}   bin at {list(path)}: {len(b)} B, starts {magic!r}{tag}')
                        if a.dump:
                            nbin += 1
                            fn = os.path.join(a.dump, f'{nbin:04d}_{t:09.4f}_{d}_{svc_name}.bin')
                            open(fn, 'wb').write(b)


if __name__ == '__main__':
    sys.exit(main())
