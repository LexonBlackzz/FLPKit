"""fl6to5.py in.flp out.flp: FL 6.0 -> FL 5.0 rewrite (rules from FL 5's own 5.0.0 demos and the 5 demos FL 5
ships as 4.x saves and FL 6 as 6.0 saves). fl8to5.py chains fl8to6 + this."""
import sys, os, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fl8to6 import read, write
import zlib, sy15to7

DROP = {26, 95, 27, 235, 236}  # FL 6 only: never in FL 5.0 saves (236 insert start, 235 routing)
VST5 = {b'Fruity Compressor', b'Fruity Reeverb', b'Fruity Phaser', b'Fruity Free Filter'}  # 212 byte 8 = 0 (VST-hosted in FL 5)
SUBST = {}  # plugin -> (FL 5 name, state fn(d, ev)) or None to drop the slot; filled by fl8to5 for song specifics

def mixer_param(p, t):
    """FL 6 mixer param p of track t -> FL 5 param, or None (FL 5 has no per-track routing)"""
    if p in (0, 1): return p
    if 129 <= p <= 132 and 1 <= t <= 64: return p - 65
    if p >= 192: return p - 96
    return None

def links227(d):
    """automation/controller links: 20-byte records, a mixer target is [u8 param][u8 31][u16 128 + track << 6]
    at 8 (same coding as 225), renumbered like 225"""
    for k in range(0, len(d) - 19, 20):
        d[k + 12:k + 16] = bytes(4)  # FL 6 sets 4 / 8 here, FL 5 always 0
        if d[k + 9] == 31:
            q = mixer_param(d[k + 8], (struct.unpack_from('<H', d, k + 10)[0] >> 6) - 128)
            if q is not None: d[k + 8] = q
    return d

def mixer225(d):
    """FL 6 params per track t: 0/1 slot on/mix, 64+j route level to track j (0..68), 192.. track params.
    FL 5: 0/1 the same, 64..67 send level to the 4 send tracks (65..68; inserts 1..64 only), track params at
    -96. Mix, send, volume (96) and pan (97) are 0..128 / ±64 in FL 5, x100 in FL 6."""
    out = []
    for k in range(0, len(d), 12):
        r = bytearray(d[k:k + 12])
        if r[5] != 31: continue
        q = mixer_param(r[4], (struct.unpack_from('<H', r, 6)[0] >> 6) - 128)
        if q is None: continue
        r[4] = q
        if r[4] in (1, 64, 65, 66, 67, 96, 97):
            r[8:12] = struct.pack('<i', round(struct.unpack_from('<i', r, 8)[0] / 100))
        out.append(bytes(r))
    # FL 5 order: per track, its slots' 0/1 params, then track params by number
    out.sort(key=lambda r: (struct.unpack_from('<H', r, 6)[0] >> 6, r[4] > 1, struct.unpack_from('<H', r, 6)[0], r[4]))
    return bytearray(b''.join(out))

def values216(d):
    """216: current values of automated params, 12-byte records [u32 0][u16 param][u16 target][i32 value] with
    227's addresses. Mixer params (byte 1 of param = 31) renumbered like 225 and, for mix/send/volume/pan, /100;
    channel volume (0) and pan (1) /100 too (FL 5 0..128). Plugin params etc. as is."""
    d = bytearray(d)
    for k in range(0, len(d) - 11, 12):
        p, tgt = struct.unpack_from('<HH', d, k + 4)
        scale = False
        if p >> 8 == 31:
            q = mixer_param(p & 255, (tgt >> 6) - 128)
            if q is not None: d[k + 4] = q; scale = q in (1, 64, 65, 66, 67, 96, 97)
        elif (tgt < 0x2000 and p in (0, 1)) or (tgt == 0x4000 and p == 0): scale = True  # channel vol/pan, main volume
        if scale: struct.pack_into('<i', d, k + 8, round(struct.unpack_from('<i', d, k + 8)[0] / 100))
    return d

def levels(d, fields=(0, 1)):
    """209 / 219 / 229: fields 0 and 1 (pan, volume; for automation channels the range) are 0..128 in FL 5, x100 in
    FL 6 (all 81 channels of the 5 pairs)"""
    d = bytearray(d)
    for k in fields:
        struct.pack_into('<i', d, 4 * k, round(struct.unpack_from('<i', d, 4 * k)[0] / 100))
    return d

def convert(ev):
    first = next(k for k, e in enumerate(ev) if e[0] == 149)
    # 230 right after a link = its mapping formula ("1-Input" = inverted): FL 5 writes it the same way (user's save)
    out = [[i, bytearray(b'5.0.0\0') if i == 199 else links227(d) if i in (226, 227) else levels(d) if i in (209, 219, 229) else values216(d) if i == 216 else d]
           for i, d in ev[:first] if i not in DROP]
    if not any(i == 12 for i, _ in out):  # main volume: FL 5 always writes it (128 in its demos), FL 6 never
        k = next((k for k, e in enumerate(out) if e[0] == 11), next(k for k, e in enumerate(out) if e[0] == 199))
        out.insert(k + 1, [12, bytearray([128])])
    # FL 6 track: 149 204 27 236 [201 212 213]* 235 154 147; FL 5: [201 212 213]* 149 204 154 147
    head, slots, plugin, t = [], [], None, 0
    route = None
    for i, d in ev[first:]:
        if i == 235: route = d  # FL 6 routing: byte j = 1 if the track feeds track j
        if i in DROP: continue
        if i == 201:
            plugin = bytes(d).rstrip(b'\0')
            if plugin in SUBST:
                if SUBST[plugin] is None: plugin = 'drop'; continue
                d = bytearray(SUBST[plugin][0] + b'\0')
        if i in (201, 212, 203, 213):  # 203 = slot custom name, inside the slot
            if plugin == 'drop': continue
            if i == 213 and SUBST.get(plugin): d = SUBST[plugin][1](d, ev)
            if i == 212 and SUBST.get(plugin) and SUBST[plugin][0] in VST5: d = bytearray(d); d[8] = 0
            if i == 201 and slots and slots[-1][0] == 212: slots.append([213, bytearray()])  # FL 5 writes empty states
            slots.append([i, d]); continue
        if i in (149, 204, 154, 147):
            if i == 147:  # FL 5 output target: 0 master, j insert j (one only); the master has 65
                j = next((j for j in range(1, 65) if route and route[j]), 0)
                d = bytearray(struct.pack('<i', 65 if t == 0 else j)); route = None
            head.append([i, d])
            if i == 147:
                if slots and slots[-1][0] == 212: slots.append([213, bytearray()])
                out += slots + head; slots, head, t = [], [], t + 1
            continue
        if i == 225: d = mixer225(d)
        if i == 227: d = links227(d)
        out.append([i, d])
    return [[i, sytrus(d) if i == 213 and n == b'Sytrus' else d] for (i, d), n in zip(out, _names(out))]

def _names(ev):
    n = None
    for i, d in ev:
        if i == 201: n = bytes(d).rstrip(b'\0')
        elif i == 64: n = None
        yield n

def sytrus(d):
    """state [u32 version][zlib]: FL 6 v15 -> FL 5 v7 (sy15to7)"""
    if d[:4] != b'\x0f\0\0\0': return d
    return bytearray(b'\x07\0\0\0' + zlib.compress(sy15to7.convert(zlib.decompress(bytes(d[4:])))))

if __name__ == '__main__':
    h, ev = read(sys.argv[1])
    write(sys.argv[2], h, convert(ev))
