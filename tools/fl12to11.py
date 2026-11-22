"""fl12to11.py in.flp out.flp: FL 12.3 -> FL 11.1 rewrite (input: an FL 12.3 save or `flpkit downgrade --to 12.3`
output). Rules from newstuff / SolidInc saved by FL 11.1 and FL 12.3 (samples/*FL11Export / *FL12Export) and
FL 11's demos. FL 11 keeps FL 12's 24-byte notes, 32-byte playlist items and 105 mixer tracks; like FL 8 it has
8-bit text, 8 effect slots, steps as 91 events and FL 8's mixer track layout."""
import sys, os, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fl8to6 import read, write
import fl12to8 as e

DROP = {32, 97, 98}  # FL 12 only
TEXT = e.TEXT | {202}
MISSING = {'Transient Processor', 'Wave Candy'}  # no FL 11 counterpart (Wave Candy v9: no rule)
SLOT = {}  # (track, FL 12 slot) -> FL 11 slot
D3 = set()  # (track, slot) of Fruity Delay 3s, which become Fruity Delay 2s

def tmap(t): return t if 0 <= t <= 104 else None

def addr(p, tgt):
    a = _addr(p, tgt)
    if a and 0x2000 <= a[1] < 0x4000:
        t, s = (a[1] >> 6) - 128, a[1] & 63
        if (t, s) not in SLOT and s >= 8: return None
        if (t, s) in D3 and a[0] >> 8 != 31:  # a Delay 3 knob: only wet (23) has a Delay 2 counterpart (input, 1)
            if a[0] != 23: return None
            a = (1, a[1])
        a =(a[0], 0x2000 + (t << 6) + SLOT.get((t, s), s))
    return a

e.tmap, _addr, e.addr = tmap, e.addr, addr

def slots(ev):
    """FL 12 slots 8/9 -> the track's free slots below 8 (none free: the effect is dropped)"""
    used, name = {}, None
    for i, d in ev:
        if i == 201: name = bytes(d).decode('utf-16le', 'replace').rstrip('\0')
        if i == 212 and len(d) >= 8:
            used.setdefault(struct.unpack_from('<I', d)[0], []).append(struct.unpack_from('<I', d, 4)[0])
            if name == 'Fruity Delay 3': D3.add(struct.unpack_from('<II', d))
    for t, ss in used.items():
        free = iter(s for s in range(8) if s not in ss)
        for s in ss:
            if s >= 8:
                n = next(free, None)
                if n is not None: SLOT[(t, s)] = n

def text(d):
    return e.text(d)

def channels(ev):
    """FL 12: 64 21 201 212 203 155 128 [213] ... 143 20 [196]; FL 11: 64 21 [201 212 213] ... 143 20 [196] 192
    155 128 (samplers have no empty 201/212)"""
    out, k = [], 0
    while k < len(ev):
        if ev[k][0] != 64: out.append(ev[k]); k += 1; continue
        j = k + 1
        while j < len(ev) and ev[j][0] not in (64, 65, 233, 149, 236, 226, 227): j += 1
        blk, tail = [], {}
        for i, d in ev[k:j]:
            if i in (203, 155) or (i == 128 and blk and blk[-1][0] == 155): tail[192 if i == 203 else i] = d
            if i == 155: blk.append([155, d]); continue
            if i == 128 and blk and blk[-1][0] == 155: blk.pop(); continue
            if i == 203: continue
            if i == 201 and not bytes(d).strip(b'\0'): continue
            if i == 212 and blk and blk[-1][0] == 21: continue  # the empty 201's window
            blk.append([i, d])
        out += blk + [[i, tail[i]] for i in (192, 155, 128) if i in tail]; k = j
    return out

def mixer(ev):
    """FL 12 track: [149] [95] [204] 236(8) [98]* [201 212 [203] 155 128 213]* 235 154 147; FL 11: [149] [95] [204]
    27 236(5) [201 212 [203] 213]* 235 154 147, 149 only on colored tracks (FL 12 color - 0x24 per channel,
    FL 12's default 0x868178 = none)"""
    first = next(n for n, x in enumerate(ev) if x[0] in (149, 236))
    out, k, t = list(ev[:first]), first, -1
    while k < len(ev) and ev[k][0] in (149, 95, 204, 236):
        j = k
        while ev[j][0] != 147: j += 1
        t += 1; blk = ev[k:j + 1]; k = j + 1
        if t > 104: continue
        get = lambda n: next((d for i, d in blk if i == n), None)
        c, flags = get(149), (get(236) or b'\0\0\0\0\x2c')[4]
        head = [[149, bytearray([max(0, x - 0x24) for x in c[:3]]) + c[3:]]] if c and bytes(c[:3]) != b'\x78\x81\x86' else []
        head += [x for x in blk if x[0] in (95, 204)] + [[27, bytearray([flags & 0x1f])], [236, bytearray(b'\0\0\0\0\x01')]]
        body, drop, prev = [], False, None
        for i, d in blk:
            p, prev = prev, i
            if i in (149, 95, 204, 236, 98, 155) or (i == 128 and p == 155): continue
            if i == 201 and bytes(d).rstrip(b'\0') == b'Fruity Delay 3': d = bytearray(b'Fruity Delay 2\0')
            if i == 201:
                drop =bytes(d).rstrip(b'\0').decode('latin1') in MISSING
                if drop: print('warning: FL 11 has no', bytes(d).rstrip(b'\0').decode('latin1'), '(track', t, ') dropped', file=sys.stderr)
            if i == 212:
                s = struct.unpack_from('<I', d, 4)[0]
                if s >= 8 and (t, s) not in SLOT:
                    print('warning: FL 11 track', t, 'has no free slot; effect dropped', file=sys.stderr); drop = True
                if not drop: d = bytearray(d); struct.pack_into('<II', d, 0, t, SLOT.get((t, s), s))
            if drop and i in (201, 212, 203, 213):
                if i == 213: drop = False
                continue
            if i == 235 and 1 <= t <= 99: d = bytearray(d); d[100:104] = b'\x01' * 4  # inserts route to the sends
            if i == 213 and body[-2][0] == 201 and bytes(body[-2][1]).startswith(b'Fruity Wrapper'):
                r = e._records(bytes(d)).get(54)  # FL 11 names wrapped effects (203 = the VST's name)
                if r: body.append([203, bytearray(r.rstrip(b'\0') + b'\0')])
            body.append([i, d])
        out += head + body
    return out + list(ev[k:])

def m225(d):
    """225 records [u32 0][u8 param][u8 31][u16 0x2000 + track << 6 + slot][i32]: slot records (params 0/1) for
    slots 8/9 remapped or dropped"""
    out = bytearray()
    for k in range(0, len(d) - 11, 12):
        r = bytearray(d[k:k + 12])
        if r[5] == 31 and 164 <= r[4] <= 168 and r[8:12] == bytes(4): continue  # FL 11 omits unused send levels
        if r[5] == 31 and r[4] in (0, 1):
            a = struct.unpack_from('<H', r, 6)[0]; t, s = (a >> 6) - 128, a & 63
            if s >= 8:
                if (t, s) not in SLOT: continue
                struct.pack_into('<H', r, 6, 0x2000 + (t << 6) + SLOT[(t, s)])
        out += r
    return out

def state(name, d):
    """FL 12.3 native states -> FL 11.1 (versions from FL 11's demos and the SolidInc pair)"""
    d = bytes(d)
    if name in ('Maximus', 'Gross Beat', 'Slicex') and d[:2] == b'\x03\0':  # [u16 3][u16 v] -> [u32 v]
        return bytearray(struct.pack('<I', struct.unpack_from('<H', d, 2)[0]) + d[4:])
    if name == 'Fruity Parametric EQ 2' and d[0] == 3: return bytearray(b'\x02' + d[1:-4])  # v3 appends a u32
    if name == 'Harmless' and d[0] == 12: return bytearray(b'\x0b' + d[1:-1])  # v12 appends a byte (demo pairs)
    if name == 'Fruity Delay 2' and len(d) == 108 and d[:4] == b'\x01\0\0\0': return delay3(d)
    if name == 'Sytrus' and d[:4] == b'\x03\0\x17\0':
        print('warning: Sytrus v23 state kept (FL 11 has v22, no rule yet)', file=sys.stderr)
    return bytearray(d)

def wrapper(d):
    """Fruity Wrapper kind 8 = [u32 8][records u32 id, u64 len, data]. As FL 11 writes them (user's FL 11 save with
    Nexus / Sylenth1 VST3, GClip / GMax / TapeStop VST2): no 56 (vendor) / 57; record 2's u32 at 9 without FL 2026's
    top byte (0x80 there in Nexus / TapeStop, which FL 11 reported missing); VST2 name 54 = the DLL's file name"""
    d = bytes(d)
    if d[:4] != b'\x08\0\0\0': return bytearray(d)
    r = e._records(d)
    for k in (56, 57): r.pop(k, None)
    if len(r.get(2, b'')) >= 13: r[2] = r[2][:12] + b'\0' + r[2][13:]
    c = r.get(53, b'')
    if c[:4] == b'\xf7\xff\xff\xff' and c[5:9] == b'\xfe\xff\xff\xff' and len(c) >= 21:
        # VST2 plugin data. FL 12+: [f7ffffff][u8 flags: 4 params, 0x0c chunk][i32 -2][u32 chunk size][8 bytes]
        # [data]; FL 11 (TapeStop in the user's save, BassMidi chunk in bbp.flp): [f7ffffff][u8 flags | 1]
        # [u32 chunk size, 0 for params][u32 program][data]. FL 12's u32 at 17 = current program (Sylenth1 26,
        # 35, 83, 168, 325 in Avicii; those lost their preset when it was dropped). ponytail: program slot guessed
        size = struct.unpack_from('<I', c, 9)[0] if c[4] & 8 else 0
        r[53] = c[:4] + bytes([c[4] | 1]) + struct.pack('<II', size, struct.unpack_from('<I', c, 17)[0]) + c[21:]
    p = r.get(55, b'').rstrip(b'\0').decode('latin1')
    if p.lower().endswith('.dll'): r[54] = p.replace('/', chr(92)).rsplit(chr(92), 1)[-1][:-4].encode('latin1')
    return bytearray(d[:4] + b''.join(struct.pack('<IQ', t, len(v)) + v for t, v in r.items()))

def delay3(d):
    """Fruity Delay 3 state [u32 1][26 i32, knobs 0..6000] -> Fruity Delay 2 [8 i32: input pan, input vol, dry,
    feedback, time, stereo offset, feedback mode (0 normal, 2 ping pong), cutoff], knobs 0..128. Delay 3 fields
    read off FL 2026's 10 presets (index incl. the version): 2 stereo mode (0 mono, 1 stereo, 2 ping pong), 3 tempo
    sync, 5 time (1/192 beat = Delay 2's unit, its default 144 = 3 steps), 15 feedback (3000 = Delay 2's 64),
    17 cutoff, 23 dry, 24 wet. ponytail: linear knob scaling; filter type, modulation, diffusion, saturation dropped"""
    v = struct.unpack_from('<27i', d)
    k = lambda x: max(0, min(128, round(128 * x / 6000)))
    if not v[3]: print('warning: Delay 3 time not tempo-synced, kept as Delay 2 ticks', v[5], file=sys.stderr)
    return bytearray(struct.pack('<8i', 0, k(v[24]), k(v[23]), k(v[15]), v[5], 0, 2 if v[2] == 2 else 0, k(v[17])))

def plugins(ev):
    out, name = [], None
    for i, d in ev:
        if i == 201: name = bytes(d).rstrip(b'\0').decode('latin1')
        if i == 213 and name: d = wrapper(d) if name == 'Fruity Wrapper' else state(name, d); name = None
        out.append([i, d])
    return out

def convert(ev, ppq=96):
    SLOT.clear(); D3.clear(); slots(ev)
    out = []
    for i, d in ev:
        d = bytearray(d)
        if i in DROP: continue
        if i == 199: d = bytearray(b'11.1.1\0')
        if i in TEXT: d = text(d)
        if i == 195: i = 198
        if i == 215:  # FL 12 appends 13 bytes; i32 at 108 = stretch mode: FL 11 has 0..9 (demos), FL 12's
            d = d[:112]  # Elastique Pro modes (< 0) fall back to resample there -> E3 generic (1)
            if struct.unpack_from('<i', d, 108)[0] < 0: struct.pack_into('<i', d, 108, 1)
        if i == 238:  # FL 12 appends 2 bytes; byte 17 f0 in every FL 11 save. ponytail: its other changed fields kept
            d = d[:47]
            if d[17] == 0xff: d[17] = 0xf0
        if i == 225: d = m225(d)
        if i == 216: d = e.links(d, 12, 4)
        if i in (226, 227): d = e.links(d, 20, 8)
        out.append([i, d])
    return pitch(e.steps(plugins(mixer(channels(out))), ppq, 24, 21))

def pitch(ev):
    """FL 11 ignores the channel pitch knob (219 i32 at 8, cents) on a stretched sample (215 stretch mode at 108
    set) and only applies the stretch pitch (215 i32 at 100, cents; user's FL 11 test): fold the knob into it"""
    lv = None
    for i, d in ev:
        if i == 64: lv = None
        if i == 219: lv = d
        if i == 215 and lv is not None and len(d) >= 112 and struct.unpack_from('<i', d, 108)[0] != 0:
            k = struct.unpack_from('<i', lv, 8)[0]
            if k:
                struct.pack_into('<i', d, 100, struct.unpack_from('<i', d, 100)[0] + k); struct.pack_into('<i', lv, 8, 0)
    return ev

if __name__ == '__main__':
    h, ev = read(sys.argv[1])
    write(sys.argv[2], h, convert(ev, int.from_bytes(h[12:14], 'little')))
