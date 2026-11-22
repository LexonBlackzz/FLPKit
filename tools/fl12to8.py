"""fl12to8.py in.flp out.flp: FL 12.3 -> FL 8.0 rewrite (input: an FL 12.3 save or `flpkit downgrade --to 12.3`
output). Rules from newstuff / SolidInc saved by FL 8.0 and FL 12.3 (samples/*FL8Export / *FL12Export) and the
Scott Fisher - Moholithic Filter demo (FL 8.0 save in FL 8's Data, FL 9.1 save in FL 12's)."""
import sys, os, struct, zlib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fl8to6 import read, write, pcm_wav
import sy23to18

TEXT = {193, 194, 195, 196, 197, 200, 201, 203, 204, 206, 207, 230, 231, 239, 241}  # UTF-16 in FL 12, 8-bit in FL 8
DROP = {28, 29, 31, 32, 97, 98, 202, 237, 238, 239}  # FL 11/12 only

def text(d):
    return bytearray(bytes(d).decode('utf-16le', 'replace').rstrip('\0').encode('latin1', 'replace') + b'\0')

def notes(d):
    """224: 24-byte notes -> 20: key u16 -> u8, the u16 group and the u8 after fine go"""
    out = bytearray()
    for k in range(0, len(d) - 23, 24):
        r = d[k:k + 24]
        out += r[:12] + r[12:13] + r[16:17] + r[18:24]
    return out

def playlist(d):
    """233: 32-byte items -> 28: track u16 at 12 (199 - track) -> u8 (99 - track), the u8 at 16 moves to 13;
    untrimmed start/end i32 -1 -> f32 -1. Clips on tracks past 99 dropped."""
    out = bytearray()
    for k in range(0, len(d) - 31, 32):
        r = d[k:k + 32]
        t = struct.unpack_from('<H', r, 12)[0] - 100
        if t < 0: continue
        se = b''.join(b'\0\0\x80\xbf' if r[o:o + 4] == b'\xff' * 4 else r[o:o + 4] for o in (24, 28))
        out += r[:12] + bytes([t, r[16], 0, 0]) + r[20:24] + se
    return out

def autoclip(d):
    """234 automation clip: [21-byte head, version at 13][n x 24-byte points (f64 x, f64 y, f32 tension, u32
    mode)][tail, version at 40]: versions 3 -> 2, the mode's top byte (0..2) cleared, FL 12 appends 4 bytes
    (SolidInc's 31 clips equal after this)"""
    d = bytearray(d)
    if len(d) < 21 or d[13] != 3: return d
    d[13] = 2
    n = struct.unpack_from('<I', d, 17)[0]
    for k in range(n): d[21 + 24 * k + 23] = 0
    e = 21 + 24 * n
    if len(d) > e + 40 and d[e + 40] == 3: d[e + 40] = 2
    return d[:-4]

def channels(ev):
    """FL 12 channel: 64 21 201 212 203 155 128 [213] ... 143 20 [196]; FL 8: 64 21 [201 212 213] ... 143 20
    [196] 192 128 (name and color at the end; samplers have no empty 201/212)"""
    out, k = [], 0
    while k < len(ev):
        if ev[k][0] != 64: out.append(ev[k]); k += 1; continue
        j = k + 1
        while j < len(ev) and ev[j][0] not in (64, 65, 233, 149, 226, 227): j += 1
        blk, name, color = [], None, None
        for i, d in ev[k:j]:
            if i == 203: name = d; continue
            if i == 128 and blk and blk[-1][0] == 155: color = d; blk.pop(); continue
            if i == 201 and not bytes(d).strip(b'\0'): continue
            if i == 212 and blk and blk[-1][0] == 21: continue  # the empty 201's window
            blk.append([i, d])
        if name is not None: blk.append([192, name])
        if color is not None: blk.append([128, color])
        out += blk; k = j
    return out

def tmap(t):
    """FL 12 mixer track -> FL 8 (None: no FL 8 track). FL 9+ keeps FL 8's inserts 1..64, moves its 4 send
    tracks to 100..103 and the current track to 104; 65..99 are new."""
    if 0 <= t <= 64: return t
    if 100 <= t <= 104: return t - 35
    return None

def addr(p, tgt):
    """automation / link address (param u16, target u16) FL 12 -> FL 8, or None. Mixer targets 0x2000 + track
    << 6 + slot: track remapped, mixer params 64+j (route level to track j) remapped; plugin params flagged 0x8000
    in FL 12: FL 8 writes them bare on mixer effects, 0x2000 + param on channel generators (Sytrus in SolidInc).
    ponytail: the generator rule is from 2 Sytrus links only"""
    if 0x2000 <= tgt < 0x4000:
        t = tmap((tgt >> 6) - 128)
        if t is None: return None
        tgt = 0x2000 + (t << 6) + (tgt & 63)
        if p >> 8 == 31 and 64 <= p & 255 <= 168:
            j = tmap((p & 255) - 64)
            if j is None: return None
            p = 0x1f00 + 64 + j
        elif p & 0x8000: p &= 0x7fff
    elif tgt < 0x2000 and p & 0x8000: p = 0x2000 | (p & 0x7fff)
    return p, tgt

def links(d, size, at):
    """216 (12-byte records, address at 4) / 226, 227 (20-byte, address at 8): addresses via addr()"""
    out = bytearray()
    for k in range(0, len(d) - size + 1, size):
        r = bytearray(d[k:k + size])
        a = addr(*struct.unpack_from('<HH', r, at))
        if a is None: continue
        struct.pack_into('<HH', r, at, *a); out += r
    return out

def mixer225(d):
    """225 [u32 0][u8 param][u8 31][u16 0x2000 + track << 6 + slot][i32]: per FL 8 track: slot params 0/1
    (slots 0..7; FL 12 has 10), track params 192.., then route levels 64..133 (to tracks 0..69: 12800 to
    tracks, 0 to the sends unless FL 12 has a level; FL 12 writes only the levels it uses)"""
    head, per = bytearray(), {}
    for k in range(0, len(d) - 11, 12):
        r = bytearray(d[k:k + 12])
        if r[5] != 31: head += r; continue
        a = struct.unpack_from('<H', r, 6)[0]
        t, slot = tmap((a >> 6) - 128), a & 63
        if t is None or slot >= 8: continue
        struct.pack_into('<H', r, 6, 0x2000 + (t << 6) + slot)
        per.setdefault(t, []).append(r)
    out = head
    for t in sorted(per):
        rs, routes = per[t], {}
        for r in rs:
            if 64 <= r[4] <= 168 and tmap(r[4] - 64) is not None: routes[64 + tmap(r[4] - 64)] = r[8:12]
        out += b''.join(r for r in rs if r[4] in (0, 1))
        out += b''.join(r for r in rs if r[4] > 1 and not 64 <= r[4] <= 168)
        a = struct.pack('<H', 0x2000 + (t << 6))
        for q in range(64, 134):
            out += struct.pack('<IBB', 0, q, 31) + a + routes.get(q, struct.pack('<i', 12800 if q < 129 else 0))
    return out

def mixer(ev):
    """FL 12 track (105): [149] [95] [204] 236(8) [98 slot]* [201 212 155 128 213]* 235(105) 154 147 (149 only on
    tracks it has data for); FL 8 (70): 149 [95] 204 27 236(5) [201 212 213]* 235(70) 154 147"""
    first = next(n for n, e in enumerate(ev) if e[0] in (149, 236))
    k, t, blocks = first, -1, {}
    while ev[k][0] in (149, 95, 204, 236):
        j = k
        while ev[j][0] != 147: j += 1
        t += 1
        blocks[t] = ev[k:j + 1]; k = j + 1
    out = list(ev[:first])
    for t12, blk in sorted(blocks.items(), key=lambda b: tmap(b[0]) if tmap(b[0]) is not None else 999):
        t = tmap(t12)
        if t is None: continue
        name = next((d for i, d in blk if i == 204), bytearray(b'\0'))
        flags = next((d[4] for i, d in blk if i == 236), 0x2c)
        color = next((d for i, d in blk if i == 149), None)
        # FL 12 colors = FL 8's + 0x24 per channel (SolidInc's 25 colored tracks; default 0x868178 -> 0x625d54)
        color = bytearray([max(0, x - 0x24) for x in color[:3]]) + color[3:] if color else bytearray.fromhex('545d6200')
        head = [[149, color]] + [e for e in blk[:3] if e[0] == 95]
        body = []
        # FL 12 has 10 slots, FL 8 8: slots 8/9 move to the track's free slots below 8 (dropped if none).
        # ponytail: automation of a moved slot keeps its old address
        used = [struct.unpack_from('<I', d, 4)[0] for i, d in blk if i == 212]
        free = iter(s for s in range(8) if s not in used)
        for i, d in blk:
            if i in (149, 95, 204, 236, 98, 155) or (i == 128 and body and body[-1][0] == 212): continue
            if i == 212:
                s = struct.unpack_from('<I', d, 4)[0]
                if s >= 8: s = next(free, None)
                if s is None:
                    print('warning: FL 8 track', t, 'has no free slot; effect dropped', file=sys.stderr)
                    while body and body[-1][0] == 201: body.pop()
                    skip = True; continue
                d = bytearray(d); struct.pack_into('<II', d, 0, t, s); skip = False
            if i == 213 and locals().get('skip'): skip = False; continue
            if i == 235:  # FL 8: inserts 1..64 always route to the 4 send tracks; the current track (69) nowhere
                r = bytearray(70)
                for j, v in enumerate(d):
                    if v and tmap(j) is not None: r[tmap(j)] = v
                if 1 <= t <= 64: r[65:69] = b'\x01' * 4
                if t == 69: r[0] = 0
                d = r
            if i == 147:
                o = struct.unpack('<i', bytes(d))[0]
                d = bytearray(struct.pack('<i', -1 if o < 0 or tmap(o) is None else tmap(o)))
            body.append([i, d])
        out += head + [[204, name], [27, bytearray([flags & 0x1f])], [236, bytearray(b'\0\0\0\0\x01')]] + body
    return out + list(ev[k:])

LEGACY = {'Fruity Balance', 'Fruity Blood Overdrive', 'Fruity Chorus', 'Fruity Compressor', 'Fruity Delay',
          'Fruity Fast LP', 'Fruity Filter', 'Fruity Flanger', 'Fruity Free Filter', 'Fruity Mute 2', 'Fruity Phaser',
          'Fruity Reeverb', 'Fruity Bass Boost', 'Fruity 7 Band EQ', 'Fruity Center', 'Fruity Phase Inverter'}

def _records(b):
    o, r = 4, {}
    while o + 12 <= len(b):
        t, n = struct.unpack_from('<IQ', b, o); r[t] = b[o + 12:o + 12 + n]; o += 12 + n
    return r

def unwrap(d):
    """FL 12 Fruity Wrapper (kind 8) hosting one of FL 8's legacy effect VSTs -> (name, FL 8 state): FL 8 saves
    those as the plugin name with the VST's f32 parameters (SolidInc: equal to FL 12's chunk floats), or None"""
    if d[:4] != b'\x08\0\0\0': return None
    r = _records(bytes(d))
    name, c = r.get(54, b'').decode('latin1', 'replace').rstrip('\0'), r.get(53, b'')
    if name not in LEGACY or c[:1] != b'\xf7' or len(c) < 25: return None
    n = struct.unpack_from('<I', c, 21)[0]
    return name, bytearray(c[25:25 + 4 * n])

def state(name, d):
    """FL 12 / 2026 native states FL 8 reads differently; others as is (same versions: 3x Osc, Parametric EQ,
    Stereo Enhancer, Delay 2, Flangus, ...)"""
    d = bytes(d)
    if name == 'Sytrus' and d[:4] == b'\x03\0\x17\0':
        return bytearray(struct.pack('<I', 18) + zlib.compress(sy23to18.convert(zlib.decompress(d[4:]))))
    if name == 'Fruity slicer' and d[:4] == b'\x10\0\0\0':
        # v16 = v15's header, path, slices and 13-byte trailer + per-slice articulator blocks; v15 ends with u32 0
        o = 33 + d[32]; n = struct.unpack_from('<I', d, o)[0]
        assert n == 0, 'v16 slices not decoded'  # ponytail: FL 2026's Moholithic has none (its sample is gone)
        return bytearray(struct.pack('<I', 15) + d[4:o + 4 + 13] + bytes(4))
    if name == 'Fruity 7 Band EQ' and len(d) == 32 and d[:4] == b'\x01\0\0\0':
        return bytearray(d[4:])  # FL 20+ prefixes FL 8's 7 floats with a u32 version
    if name in NEWER: print(f'warning: {name} state kept as FL 12 wrote it (FL 8 has an older version)', file=sys.stderr)
    return bytearray(d)

# FL 8 vs FL 12.3 state versions differ (SolidInc / newstuff pairs), not converted yet
NEWER = {'Fruity Love Philter', 'Slicex', 'Maximus', 'Fruity Parametric EQ 2', 'MIDI Out', 'Fruity Limiter'}

def plugins(ev):
    """201 name / 212 window / 213 state per plugin: legacy-effect wrappers unwrapped (212 byte 8 = 0, as FL 8
    hosts them), states converted"""
    out, warned = [], set()
    for i, d in ev:
        if i == 213 and out and any(e[0] == 201 for e in out[-3:]):
            k = max(n for n in range(len(out) - 3, len(out)) if out[n][0] == 201)
            name = bytes(out[k][1]).rstrip(b'\0').decode('latin1')
            u = unwrap(d) if name == 'Fruity Wrapper' else None
            if u:
                name, d = u
                out[k][1] = bytearray(name.encode('latin1') + b'\0')
                w = next((e for e in out[k:] if e[0] == 212), None)
                if w: w[1] = bytearray(w[1]); w[1][8] = 0
            else:
                d = state(name, d)
                if name in VST8:  # native in FL 12+, a VST in FL 8: byte 8 = 2 makes FL 8 report it missing
                    w = next((e for e in out[k:] if e[0] == 212), None)
                    if w: w[1] = bytearray(w[1]); w[1][8] = 0
        out.append([i, d])
    return out

# FL 8's Plugins/VST: Fruity effects FL 8 hosts as VSTs (FL 8's own saves: 212 byte 8 = 0)
VST8 = {'Fruity 7 Band EQ', 'Fruity Balance', 'Fruity Bass Boost', 'Fruity Blood Overdrive', 'Fruity Center',
        'Fruity Chorus', 'Fruity Compressor', 'Fruity Delay', 'Fruity Fast LP', 'Fruity Filter', 'Fruity Flanger',
        'Fruity Free Filter', 'Fruity Mute 2', 'Fruity Phase Inverter', 'Fruity Phaser', 'Fruity Reeverb'}

from paths import IL, NEWEST
FL8_DATA, NEW_DATA = IL + 'FL Studio 8/Data', NEWEST + '/Data'
_fl8 = None

def sample(d, outdir):
    r"""196 sample path %FLStudioData%\X (FL 12+): FL 8 Data-relative \Y when FL 8's Data has a file of that name
    (FL 12+ moved packs, e.g. Packs\Legacy\Drums\Dance -> FL 8's Packs\Dance), else copied from FL 2026's Data
    into outdir and pointed to by absolute path"""
    global _fl8
    bs = chr(92)
    p = bytes(d).rstrip(b'\0').decode('latin1')
    if not p.startswith('%FLStudioData%' + bs): return d
    rel = p[len('%FLStudioData%'):]
    if _fl8 is None:
        _fl8 = {}
        for r, _, fs in os.walk(FL8_DATA):
            for f in fs: _fl8.setdefault(f.lower(), bs + os.path.relpath(os.path.join(r, f), FL8_DATA).replace('/', bs))
    name = rel.rsplit(bs, 1)[-1]
    if os.path.exists(FL8_DATA + rel.replace(bs, '/')): return bytearray(rel.encode('latin1') + b'\0')
    if name.lower() in _fl8: return bytearray(_fl8[name.lower()].encode('latin1') + b'\0')
    src = NEW_DATA + rel.replace(bs, '/')
    if not os.path.exists(src): return d
    os.makedirs(outdir, exist_ok=True)
    dst = os.path.join(outdir, os.path.splitext(name)[0] + '.wav')
    pcm_wav(src, dst)
    return bytearray(os.path.abspath(dst).encode('latin1') + b'\0')

def steps(ev, ppq, size=20, vel=None):
    """FL 12+ saves step-sequencer steps as length-0 notes (224); FL 8 doesn't play those, it keeps steps as 91
    `[u8 step][u8 1]` after the channel's block, under a 65 (pattern) - FL 8's Moholithic: 65 1, 91 00 01 / 04 01 /
    08 01 / 0c 01 after the kick channel. ponytail: per-step velocity/pan dropped (FL 8 keeps them elsewhere).
    size / vel: note record size and its velocity offset; with vel, FL 11's 72 [step][velocity] before each 91"""
    per, out, pat = {}, [], None
    for i, d in ev:
        if i == 65: pat = struct.unpack('<H', bytes(d))[0]
        if i == 224 and pat is not None:
            keep = b''
            for k in range(0, len(d), size):
                r = bytes(d[k:k + size]); pos, ch, ln = struct.unpack_from('<I', r)[0], struct.unpack_from('<H', r, 6)[0], struct.unpack_from('<I', r, 8)[0]
                if ln == 0 and pos % (ppq // 4) == 0 and pos // (ppq // 4) < 256:
                    per.setdefault(ch, {}).setdefault(pat, []).append((pos // (ppq // 4), r[vel] if vel else None))
                else: keep += r
            if not keep:
                if out and out[-1][0] == 65: out.pop()  # pattern left with no notes
                continue
            d = bytearray(keep)
        out.append([i, d])
    if not per: return out
    res, ch = [], None
    last = max(n for n, e in enumerate(out) if e[0] == 64)
    end = next((n for n in range(last, len(out)) if out[n][0] in (152, 233, 149)), len(out))
    for n, (i, d) in enumerate(out):
        if (i == 64 or n == end) and ch in per:
            for p in sorted(per[ch]):
                res.append([65, bytearray(struct.pack('<H', p))])
                for s, v in sorted(per[ch][p]):
                    if v not in (None, 100): res.append([72, bytearray([s, v])])  # FL 11: only off-default velocities
                    res.append([91, bytearray([s, 1])])
            ch = None
        if i == 64: ch = struct.unpack('<H', bytes(d))[0]
        res.append([i, d])
    return res

def sizes(ev):
    """212 window size (i32 w, h at 44) = 0: the plugin's default size. FL 8 never saves 0 but reads it fine, and
    FL 11+ then uses its own default instead of FL 8's size (user-tested on Moholithic in FL 8 and FL 11)"""
    for i, d in ev:
        if i == 212 and len(d) >= 52: d[44:52] = bytes(8)
    return ev

def convert(ev, outdir='samples', ppq=96):
    out = []
    for i, d in ev:
        d = bytearray(d)
        if i == 199: d = bytearray(b'8.0.0\0')
        elif i == 156:  # tempo: BPM x 1000 -> 93 (fine) + 66 (BPM)
            bpm = struct.unpack('<I', bytes(d))[0]
            out += [[93, bytearray(struct.pack('<H', bpm % 1000))], [66, bytearray(struct.pack('<H', bpm // 1000))]]
            continue
        if i in TEXT: d = text(d)
        if i == 196: d = sample(d, outdir)
        if i == 195: i = 198
        if i == 224: d = notes(d)
        if i == 215: d = d[:112]  # FL 12 appends 13 bytes
        if i == 233: d = playlist(d)
        if i == 234: d = autoclip(d)
        if i == 225: d = mixer225(d)
        if i == 216: d = links(d, 12, 4)
        if i in (226, 227): d = links(d, 20, 8)
        if i == 22 and len(d) == 1: d = bytearray([tmap(d[0]) or 0])  # channel's mixer track
        if i == 150:  # pattern color: FL 12's default 0x565148 is FL 8's 0xc8c1b8; FL 8 adds 151 (0)
            if bytes(d) == b'HQV\0': d = bytearray.fromhex('b8c1c800')
            out += [[i, d], [151, bytearray(4)]]; continue
        if i == 65 and not any(e[0] == 216 for e in out): out.append([216, bytearray()])
        if i in DROP: continue
        out.append([i, d])
    out = sizes(steps(plugins(mixer(channels(out))), ppq))
    k = next(n for n, e in enumerate(out) if e[0] == 233)
    if out[k - 1][0] != 152: out.insert(k, [152, bytearray(4)])  # FL 8 writes 152 (0) before the playlist
    return out

if __name__ == '__main__':
    h, ev = read(sys.argv[1])
    write(sys.argv[2], h, convert(ev, os.path.splitext(sys.argv[2])[0] + ' samples', int.from_bytes(h[12:14], 'little')))
