"""fl4to3.py in.flp out.flp: FL 4.x -> FruityLoops 3.5.4. Rules from the demos both ship (Saw, Clouds, LogDrumThing,
BeepMap Ambi: 4.1.2 vs 3.5.4 saves; Hoovalicious, SubwooferTest, NewStuff).
- header: 199 3.5.4, 200 one 0 byte; 17 = 16 then 24 = 16 (FL 4: 17 = 4), 23 = 1, 217 (16 bytes) after 18
- 226/227 links: 16 bytes (FL 4 appends u32 128)
- mixer: 21 tracks (0 master, 1-16 inserts, 17-20 sends = FL 4's 65-68), 8 slots; effects (201 212 213, 212 u32 0/4 =
  track/slot, byte 20 = 0x10) all before the first channel; after the playlist: 84 0, 225 (send levels 64-67 of
  inserts 1-16 only), then per track 204 name + 147 (0 on the master, -1 elsewhere), then 133
- channels: 22 (insert) = FL 4's - 1; 215 first 80 bytes; 3x Osc state v12 -> v11 (first 88 bytes)
- patterns: no 150/151/152; playlist 233 -> one 129 per item: u16 bar (ticks / 4 PPQ), u16 pattern
- 219 byte 20: FL 4 6 = FL 3 7 (every pair). ponytail: other values kept; FL 4's 1 is 1 or 5 in FL 3, 4 doesn't exist
- plugin names as FL 3.5 spells them; samples: Data-relative paths found in FL 3.5 by name, else copied"""
import sys, os, struct, shutil, glob
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fl8to6 import read, write

from paths import PF
FL3 = PF + 'FruityLoops3.5'
FL4_DATA = PF + 'FLStudio4/Data'
NAMES = {n.lower(): n for n in ('3x Osc', 'Fruit kick', 'Fruity reeverb', 'Fruity compressor', 'Fruity delay',
         'Fruity delay 2', 'Fruity parametric EQ', 'Fruity soft clipper', 'Fruity stereo enhancer', 'Fruity free filter',
         'Fruity flanger', 'Fruity 7 band EQ', 'Fruity filter', 'Fruity slicer', 'Fruity phaser', 'Fruity chorus',
         'Fruity blood overdrive', 'Fruity balance', 'Fruity fast dist', 'Fruity peak controller', 'Fruity send')}
TRACK = {**{t: t for t in range(17)}, **{65 + k: 17 + k for k in range(4)}}

def sample(d, outdir):
    p = bytes(d).rstrip(b'\0').decode('latin1')
    if not p.startswith('\\'): return d
    name = p.rsplit('\\', 1)[-1]
    hit = glob.glob(FL3 + '/Samples/**/' + glob.escape(name), recursive=True)  # FL 3.5 paths are Samples-relative
    if hit: return bytearray(('\\' + os.path.relpath(hit[0], FL3 + '/Samples').replace('/', '\\')).encode('latin1') + b'\0')
    src = FL4_DATA + p.replace('\\', '/')
    if not os.path.exists(src): print('missing sample', p, file=sys.stderr); return d
    os.makedirs(outdir, exist_ok=True); dst = os.path.join(outdir, name); shutil.copyfile(src, dst)
    return bytearray(os.path.abspath(dst).encode('latin1') + b'\0')

def slicer(d, outdir):
    """FL 4's speech synth (.speech files, rendered on load) doesn't exist in FL 3.5: point the slicer at <stem>.wav
    in the samples folder, where a render of the phrase (made in FL 4) is expected. State v6: 20-byte header,
    then the path as [u8 len][text]"""
    n = d[20]; path = bytes(d[21:21 + n]).decode('latin1')
    if not path.lower().endswith('.speech'): return d
    wav = os.path.abspath(os.path.join(outdir, path.rsplit(chr(92), 1)[-1][:-7] + '.wav')).encode('latin1')
    print('slicer needs a WAV render of', path, '->', wav.decode('latin1'), file=sys.stderr)
    return bytearray(bytes(d[:20]) + bytes([len(wav)]) + wav + bytes(d[21 + n:]))

EQ_DEFAULT = {96: 128, 97: 0, 98: 0, 112: 0, 113: 0, 114: 0, 120: 5777, 121: 33145, 122: 55825,
              128: 17500, 129: 17500, 130: 17500}
EQ_212 = bytes.fromhex('03000000020000000200000000000000000000001000000000000000000000000000000000000000000000000000'
                       '0000000000000000')  # Saw's Parametric EQ window info (u32 0/4 = track/slot)

def fader_eq(trackp, fx):
    """FL 4 inserts have a volume fader (225 param 96, 0..128) and a 3-band EQ (112-114 gain in 1/100 dB, 120-122
    freq 0..65536, 128-130 width), FL 3.5's don't: a Fruity parametric EQ in the track's next free slot does both.
    Its state is the same coding (FL 4's mixer defaults = its Default preset): [7 gains][7 freqs][7 widths]
    [7 types: 0 off, 5 low shelf, 6 peak, 7 high shelf][global gain 0..128, 128 default].
    ponytail: pan (97) / stereo separation (98) not carried (warned)"""
    out = []
    for t4, t3 in TRACK.items():
        v = {p: trackp.get((t4, p), dv) for p, dv in EQ_DEFAULT.items()}
        if v == EQ_DEFAULT: continue
        if v[97] or v[98]: print('FL 4 track', t4, 'pan/separation not carried', v[97], v[98], file=sys.stderr)
        used = [struct.unpack_from('<I', e[1], 4)[0] for e in fx if e[0] == 212 and struct.unpack_from('<I', e[1])[0] == t3]
        slot = max(used, default=-1) + 1
        if slot > 7: print('FL 3 track', t3, 'has no free slot for its fader/EQ', file=sys.stderr); continue
        st = struct.pack('<29i', *(v[112], v[113], v[114], 0, 0, 0, 0, v[120], v[121], v[122], 0, 0, 0, 0,
                                   v[128], v[129], v[130], 0, 0, 0, 0, 5, 6, 7, 0, 0, 0, 0, v[96]))
        out += [[201, bytearray(b'Fruity parametric EQ\0')],
                [212, bytearray(struct.pack('<II', t3, slot) + EQ_212[8:])], [213, bytearray(st)]]
    return out

def convert(ev, outdir, ppq=96):
    ids = [i for i, d in ev]
    mix0 = ids.index(233) if 233 in ids else ids.index(149)
    head, mixer = ev[:mix0 + 1], ev[mix0 + 1:]
    # mixer: tracks end with 147; effects before 149
    fx, names, sends, trackp, trk = [], [], {}, {}, 0
    for i, d in mixer:
        if i == 201: cur = [[201, bytearray(NAMES.get(bytes(d).rstrip(b'\0').lower().decode('latin1'),
                                                         bytes(d).rstrip(b'\0').decode('latin1')).encode('latin1') + b'\0')]]
        elif i in (212, 203) and trk in TRACK: cur.append([i, bytearray(d)])
        elif i == 213:
            if trk in TRACK:
                w = [e for e in cur if e[0] == 212][0][1]
                w[0:4] = struct.pack('<I', TRACK[trk]); w[20] = 0x10
                fx += cur + [[213, d]]
            else: print('dropped effect on FL 4 mixer track', trk, bytes(cur[0][1]), file=sys.stderr)
        elif i == 204 and trk in TRACK: names.append((TRACK[trk], d))
        elif i == 147: trk += 1
        elif i == 225:
            for k in range(0, len(d), 12):
                _, p, ty, t, v = struct.unpack_from('<IBBHi', d, k)
                if ty == 31 and 64 <= p < 68 and 1 <= (t >> 6) - 128 <= 16: sends[((t >> 6) - 128, p)] = v
                if ty == 31 and p >= 96: trackp[((t >> 6) - 128, p)] = v
    fx += fader_eq(trackp, fx)
    out, plugin, first64 = [], None, True
    for i, d in head:
        if i in (150, 151, 152, 26, 142): continue  # FL 3.5 never writes these (26 after pattern names, 142 in a channel)
        if i == 199: d = bytearray(b'3.5.4\0')
        elif i == 200: d = bytearray(1)
        elif i == 17: out += [[17, bytearray([16])], [24, bytearray([16])]]; continue
        elif i == 23: d = bytearray([1])
        elif i in (226, 227): d = bytearray(d[:16])
        elif i == 64:
            plugin = None
            if first64: out += fx; first64 = False
        elif i == 201: plugin = bytes(d).rstrip(b'\0').lower(); d = bytearray(NAMES.get(plugin.decode('latin1'), plugin.decode('latin1')).encode('latin1') + b'\0') if plugin.decode('latin1') in NAMES else d
        elif i == 213 and plugin == b'fruity slicer': d = slicer(d, outdir)
        elif i == 213 and plugin == b'3x osc' and d[0] == 12: d = bytearray(b'\x0b' + bytes(d[1:88]))
        elif i == 22: d = bytearray([max(0, d[0] - 1)])
        elif i == 215: d = bytearray(d[:80])
        elif i == 219: d = bytearray(d); d[20] = {6: 7}.get(d[20], d[20])
        elif i == 196: d = sample(d, outdir)
        elif i == 233:
            items = sorted(struct.unpack_from('<IHHIH', d, k) for k in range(0, len(d), 28))
            out += [[129, bytearray(struct.pack('<HH', pos // (4 * ppq), (0x78E7 - row) & 0xffff))]
                    for pos, _, base, ln, row in items if base == 0x5000]  # bar = 4 beats; FL 3 lists by bar
            continue
        out.append([i, d])
        if i == 18: out.append([217, bytearray(bytes.fromhex('0000000000000000ffffffffffffffff'))])
    out = [e for k, e in enumerate(out) if not (e[0] == 65 and k + 1 < len(out) and out[k + 1][0] in (64, 65))]  # selects left empty by 26
    out.append([84, bytearray(2)])
    out.append([225, bytearray(b''.join(struct.pack('<IBBHi', 0, p, 31, 0x2000 + (t << 6), sends.get((t, p), 0))
                                       for t in range(1, 17) for p in range(64, 68)))])
    nm = dict(names)
    for t in range(21): out += [[204, bytearray(nm.get(t, b'\0'))], [147, bytearray(struct.pack('<i', 0 if t == 0 else -1))]]
    out += [e for e in mixer if e[0] == 133]
    return out

if __name__ == '__main__':
    h, ev = read(sys.argv[1])
    write(sys.argv[2], h, convert(ev, os.path.splitext(sys.argv[2])[0] + ' samples', int.from_bytes(h[12:14], 'little')))
