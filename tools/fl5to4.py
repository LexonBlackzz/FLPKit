"""fl5to4.py in.flp out.flp: FL 5.0 -> FL 4.5.2 rewrite. Rules from the 3 demos FL 4 ships as 4.5.2 saves and
FL 5 as 5.0.0 saves (Fuego-FL303, Fuego-Momentum, NewStuff) plus both versions' other demos. fl8to4.py chains
fl8to6 + fl6to5 + this."""
import sys, os, struct, zlib, wave
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fl8to6 import read, write
import sy7to6, clips4

DROP = {206, 207, 234}  # FL 5 only (206/207 song genre/author strings)

def links(d):
    """226/227 20-byte records: FL 4 writes u32 128 at 16 (FL 5 a signed value, e.g. -512 where FL 4 has 128).
    ponytail: field not decoded, FL 4's constant"""
    d = bytearray(d)
    for k in range(0, len(d) - 19, 20): d[k + 16:k + 20] = struct.pack('<I', 128)
    return d

def playlist(d, types):
    """233 channel items (u16 0x5000 at 4, channel at 6): automation clips (type 5) dropped; audio clips keep, their
    trims at 20/24 go from FL 5's f32 ms (-1 = untrimmed) to FL 4's u32 start / end frames at 44.1 kHz
    (untrimmed ffffffff / 7fffffff; NewStuff: 0 / 881999 = 20 s)"""
    out = b''
    for k in range(0, len(d), 28):
        r = bytearray(d[k:k + 28]); base, ch = struct.unpack_from('<HH', r, 4)
        if base == 0x5000:
            if types.get(ch) == 5: continue
            s, e = struct.unpack_from('<ff', r, 20)
            r[20:28] = struct.pack('<II', 0xffffffff, 0x7fffffff) if s < 0 else \
                       struct.pack('<II', round(s * 44.1), max(0, round(e * 44.1) - 1))
        out += r
    return bytearray(out)

def sytrus(d):
    """[u32 7][zlib v7] -> [u32 6][zlib v6] (sy7to6)"""
    if d[:4] != b'\x07\0\0\0': return d
    return bytearray(b'\x06\0\0\0' + zlib.compress(sy7to6.convert(zlib.decompress(bytes(d[4:])))))

def slicer(d):
    """Fruity slicer v13 -> v6: header [u32 6][i32 bars][i32 bpm][i32 0][i32 0] (v13: [u32 13][f32 beats]
    [f32 bpm][5 x i32]); slices drop their i32 -1 / f32 -1 (name, u32 pos, u8 0); same 13-byte trailer, last
    byte 0 (FL 4 saves). ponytail: bars/bpm rounded to ints; v13 pitch/stretch/mode dropped"""
    if d[:4] != b'\x0d\0\0\0': return d
    d = bytes(d)
    beats, bpm = struct.unpack_from('<2f', d, 4)
    o = 32; out = struct.pack('<5i', 6, max(1, round(beats / 4)), round(bpm), 0, 0)
    out += d[o:o + 1 + d[o]]; o += 1 + d[o]
    n = struct.unpack_from('<I', d, o)[0]; out += d[o:o + 4]; o += 4
    for _ in range(n):
        name = d[o:o + 1 + d[o]]; o += len(name)
        out += name + d[o:o + 4] + b'\0'; o += 13
    t = d[o:o + 13]
    return bytearray(out + t[:12] + b'\0')

def convert(ev):
    ev = clips4.convert(ev)
    types, rate, steps, ch = {}, {}, {}, None
    for i, d in ev:
        if i == 64: ch = struct.unpack('<H', bytes(d))[0]
        if i == 21 and ch is not None: types[ch] = d[0]
        # FL 5/8 stretch time: 215 u32 at 96, 192 per beat (AutoPhresh 768 = 4 beats); FL 4 stretches to steps
        # (event 140, user's FL 4.5.2 save: AutoPhresh 16). ponytail: Mul / stretch pitch / mode dropped
        if i == 215 and ch is not None and len(d) >= 100:
            steps[ch] = round(struct.unpack_from('<I', d, 96)[0] / 48)
        if i == 196 and ch is not None and types.get(ch) == 4:
            try:
                with wave.open(bytes(d).rstrip(b'\0').decode('latin1')) as w: rate[ch] = w.getframerate()
            except Exception: pass
    out, plugin, ch4 = [], None, None
    has226 = any(i == 226 for i, d in ev)
    for i, d in ev:
        if i in DROP: continue
        if not has226 and i in (227, 64):  # every FL 4/5 save has this record before its 227 list; FL 4 fails without
            out.append([226, bytearray(bytes.fromhex('ff000000ff0000000400ff0f0000000080000000'))]); has226 = True
        if i == 199: d = bytearray(b'4.5.2\0')
        # FL 4 has no automation clips (channel type 5, FL 4 knows 0..4; it refuses the file): keep the channel as
        # an empty sampler and drop its playlist items (233 records with u16 0x5000 at 4 = clip, channel at 6);
        # clips4 has already moved the linked clips into X-Y Controller pattern automation
        if i == 21 and d[0] == 5: d = bytearray(b'\0')
        if i == 233: d = playlist(d, types)
        if i == 64: plugin = None
        if i == 201: plugin = bytes(d).rstrip(b'\0').lower()
        if i in (226, 227): d = links(d)
        if i == 213 and plugin == b'sytrus': d = sytrus(d)
        if i == 213 and plugin == b'fruity slicer': d = slicer(d)
        if i == 215: d = d[:96]  # FL 4.5.2 saves 96 bytes (its u32 at 92 = 1, as FL 5's); older 4.x demos 92
        if i == 143 and types.get(ch4) == 4:  # FL 4 writes audio clips' sample rate here (user's FL 4.5.2 save)
            out.append([153, bytearray(struct.pack('<I', rate.get(ch4, 44100)))])
        if i == 64: ch4 = struct.unpack('<H', bytes(d))[0]
        if i == 143:  # channel flags: FL 4 only bit 0; FL 5's bit 3 is FL 4's event 19 right after
            f = struct.unpack('<I', bytes(d))[0]
            out.append([143, bytearray(struct.pack('<I', f & 1))])
            if f & 8: out.append([19, bytearray(b'\x01')])
            continue
        if i == 135 and out and out[-1][0] == 196:  # FL 4 writes 135 before the sample path
            out.insert(len(out) - 1, [i, d]); continue
        if i == 22 and out and out[-1][0] == 70:  # 140 = FL 4's time stretch in steps (0 = off)
            out.append([140, bytearray(struct.pack('<I', steps.get(ch4, 0)))])
        out.append([i, d])
    return out

if __name__ == '__main__':
    h, ev = read(sys.argv[1])
    write(sys.argv[2], h, convert(ev))
