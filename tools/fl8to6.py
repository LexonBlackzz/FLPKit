"""fl8to6.py in.flp out.flp: prototype FL 8.0 -> FL 6.0 rewrite (rules from the 6 demo pairs)."""
import sys, struct, os, shutil, subprocess, zlib, math, wave
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sy18to15

def read(p):
    b = open(p, 'rb').read()
    hdr = b[:8 + int.from_bytes(b[4:8], 'little')]
    o = len(hdr) + 8
    ev = []
    while o < len(b):
        i = b[o]; o += 1
        if i < 64: n = 1
        elif i < 128: n = 2
        elif i < 192: n = 4
        else:
            n = s = 0
            while True:
                c = b[o]; o += 1; n |= (c & 127) << s; s += 7
                if c < 128: break
        ev.append([i, bytearray(b[o:o + n])]); o += n
    return hdr, ev

def write(p, hdr, ev):
    out = bytearray()
    for i, d in ev:
        out.append(i)
        if i >= 192:
            n = len(d)
            while True:
                c = n & 127; n >>= 7
                out.append(c | (128 if n else 0))
                if not n: break
        out += d
    open(p, 'wb').write(hdr + b'FLdt' + len(out).to_bytes(4, 'little') + out)

def automation(d):
    # v2 (FL 8): 24-byte points [f64 dbeat][f64 value][f32 tension][u32 mode]; v1: all f32, 16 bytes
    if d[13] != 2: return d
    n = struct.unpack_from('<I', d, 17)[0]
    out = bytearray(d[:21]); out[13] = 1
    for k in range(n):
        dt, v, t, m = struct.unpack_from('<ddfI', d, 21 + 24 * k)
        out += struct.pack('<fffI', dt, v, t, m)
    r = d[21 + 24 * n:]
    # tail: [u32 1][16 ff][6 u32, the 6th = version 2 -> 1][u32 m][m x [f64][f64][8 bytes] -> f32 f32 + 8]
    if len(r) >= 48 and struct.unpack_from('<I', r, 40)[0] == 2:
        t = bytearray(r[:48]); t[40] = 1
        m = struct.unpack_from('<I', r, 44)[0]
        for k in range(m):
            a, b = struct.unpack_from('<dd', r, 48 + 24 * k)
            t += struct.pack('<ff', a, b) + r[48 + 24 * k + 16:48 + 24 * k + 24]
        t += r[48 + 24 * m:]
        r = t
    return out + r

from paths import IL
FL6_DATA, FL8_DATA = IL + 'FL Studio 6/Data', IL + 'FL Studio 8/Data'

def sample(d, outdir):
    """196 sample path: FL 6 loads it if it is in its own Data; otherwise copy it from FL 8's Data
    into outdir (.ogg -> .wav, FL 6 has no ogg) and point to it by absolute path"""
    p = d.rstrip(b'\0').decode('latin1')
    if not p.startswith('\\') or os.path.exists(FL6_DATA + p.replace('\\', '/')): return d
    src = FL8_DATA + p.replace('\\', '/')
    if not os.path.exists(src): return d
    os.makedirs(outdir, exist_ok=True)
    stem, ext = os.path.splitext(os.path.basename(src))
    dst = os.path.join(outdir, stem + '.wav')
    if not os.path.exists(dst):
        pcm_wav(src, dst)
    return bytearray(os.path.abspath(dst).encode('latin1') + b'\0')

def sytrus(d):
    # state: u32 version (FL 8 0x12, FL 6 0x0f) + zlib
    if d[:4] != b'\x12\0\0\0': return d
    return bytearray(b'\x0f\0\0\0' + zlib.compress(sy18to15.convert(zlib.decompress(bytes(d[4:])))))

def peq2(d):
    """Parametric EQ 2 (FL 7+) -> Parametric EQ: both store 7 bands as i32 gains (1/100 dB), freqs
    (0..65536), widths, types (5 low shelf, 6 peaking, 7 high shelf); PEQ 2 has a u32 version first.
    ponytail: widths scaled by the default ratio 17500/25412 and same freq scale assumed (by eye
    from defaults, not measured); measure in FL 6 if bands sound off."""
    v = struct.unpack_from('<29i', d, 4)
    g, f, w, t = v[0:7], v[7:14], v[14:21], v[21:28]
    w = [max(1, min(65536, round(x * 17500 / 25412))) for x in w]
    # high/low pass (types 3/1): scaled PEQ 2 widths resonate and distort in PEQ 1; 0.05 per the user
    w = [round(0.05 * 65536) if ty in (1, 3) else x for x, ty in zip(w, t)]
    return bytearray(struct.pack('<29i', *g, *f, *w, *t, 128))


def pcm_wav(src, dst):
    """src as a PCM .wav FL 6 can load: .ogg, and .wav holding Vorbis (format 0x6771, data = Ogg
    stream) are decoded with ffmpeg"""
    if os.path.exists(dst): return
    b = open(src, 'rb').read()
    if b[:4] == b'RIFF' and struct.unpack_from('<H', b, b.find(b'fmt ') + 8)[0] == 1:
        shutil.copyfile(src, dst); return
    if b[:4] == b'RIFF':
        o = b.find(b'data'); n = struct.unpack_from('<I', b, o + 4)[0]
        src = dst + '.ogg'; open(src, 'wb').write(b[o + 8:o + 8 + n])
    subprocess.run(['ffmpeg', '-v', 'fatal', '-y', '-i', src, dst], check=True)
    if src.endswith('.wav.ogg'): os.remove(src)

def slicex(d, outdir):
    """Slicex (FL 8) -> Fruity slicer, decks A and B joined in one wav. Slicex keeps the
    deck A path as a short string at 96 (deck B at 2112), slices as the wavs' cue points (keys C5 up,
    deck B right after deck A: What I Found plays deck B slices 23-26 on keys 104-107). Fruity
    slicer v13 state (as FL 6 saves it): [u32 13][f32 loop beats][f32 loop bpm][i32 pitch cents]
    [i32 stretch][u32 mode][u32 0][u32 0..100][str path][u32 n][n x (str name, u32 sample pos, i32 -1,
    f32 -1, u8 0)][u8 1][u8 root 60][u32 0][u32 rate][01 00 01]. Slicex plays slices unstretched, so the
    loop bpm = project tempo and beats = its length at that tempo (stretch ratio 1).
    ponytail: Slicex envelopes/pitch/articulators dropped;
    mode/last fields 0 as in Pilchard-Tzachitx (meaning not decoded)"""
    os.makedirs(outdir, exist_ok=True)
    cues, pcm = [], b''
    for at in (96, 2112):  # deck A, deck B path (short strings); deck B keys follow deck A's
        if not d[at]: continue
        src = d[at + 1:at + 1 + d[at]].decode('latin1')
        b = open(src, 'rb').read()
        o = b.find(b'cue '); n = struct.unpack_from('<I', b, o + 8)[0]
        tmp = os.path.join(outdir, os.path.basename(src))
        pcm_wav(src, tmp)
        with wave.open(tmp) as w:
            rate, params, base = w.getframerate(), w.getparams(), len(pcm) // (w.getsampwidth() * w.getnchannels())
            pcm += w.readframes(w.getnframes())
        cues += sorted(base + struct.unpack_from('<6I', b, o + 12 + 24 * k)[5] for k in range(n))
    # one sample for both decks: B's slices go after A's, so key 60+k stays slice k
    dst = os.path.abspath(os.path.join(outdir, 'Slicex decks.wav'))
    with wave.open(dst, 'wb') as w: w.setparams(params); w.writeframes(pcm)
    n, frames = len(cues), len(pcm) // (params.sampwidth * params.nchannels)
    p = dst.encode('latin1')
    st = struct.pack('<I2f5i', 13, frames / rate * TEMPO / 60, TEMPO, 0, 0, 0, 0, 0)
    st += bytes([len(p)]) + p + struct.pack('<I', n)
    st += b''.join(bytes(1) + struct.pack('<Iif', c, -1, -1.0) + bytes(1) for c in cues)
    return bytearray(st + bytes([1, 60]) + struct.pack('<II', 0, rate) + bytes([1, 0, 1]))

def maximus(d, _):
    """Maximus (FL 7+) -> Fruity Multiband Compressor (3 bands, FL 6 native) with FL 6's own "Soft
    limiter" preset state (1 ms attack, thresholds near 0 dB, 0 dB output).
    ponytail: preset, not a translation of Maximus's bands/curves (state not decoded)"""
    _, ev = read(IL + 'FL Studio 6/Data/Patches/Plugin presets/Effects/Fruity Multiband Compressor/Soft limiter.fst')
    return next(e[1] for e in ev if e[0] == 213)

SUBST = {b'Maximus': (b'Fruity Multiband Compressor', maximus),
         b'Fruity Parametric EQ 2': (b'Fruity Parametric EQ', lambda d, _: peq2(d)),
         b'Slicex': (b'Fruity slicer', slicex)}

# Fruity Free Filter freq knob (0..1) -> Hz: the user read 0.2001 = 253 Hz; its presets LowPass ~5512 / ~11025 /
# 16000 are at 0.7747 / 0.918 / 1. Not one exponential (steeper low down), so log-interpolated between points.
FF_HZ = [(0.2001, 253), (0.2703, 407), (0.7747, 5512), (0.918, 11025), (1.0, 16000)]

def ff_x(hz):
    """Free Filter knob for hz: log-linear between FF_HZ points, the end segments extended"""
    for (x0, h0), (x1, h1) in zip(FF_HZ, FF_HZ[1:]):
        if hz <= h1 or x1 == 1.0:
            return min(1.0, max(0.0, x0 + (x1 - x0) * math.log(hz / h0) / math.log(h1 / h0)))

assert abs(ff_x(253) - 0.2001) < 1e-6 and abs(ff_x(16000) - 1) < 1e-6 and 0.2001 < ff_x(368) < 0.2703

def philter_ff(d):
    """Love Philter state -> Fruity Free Filter state (4 normalized f32: type = index/6 for LP, BP, HP,
    notch, low shelf, peak, high shelf; freq; Q; gain), or None if slot 0 has no filter (type 0: the
    slot only shapes volume/pan with envelopes, e.g. the Gate presets). Love Philter: [u32 2][zlib];
    inflated, a u32 text length at 60, the text, then 8 slots of [12 u32 knobs 0..256 (1 cutoff,
    2 width, 8 pan signed)][u32 order: 0 OFF..5][u32 type: 0 SVF, 1 LP, 2 HP, 4 BP, 11 resonant LP][envelopes].
    ponytail: slot 0 only, envelopes (gates, sweeps) dropped; Love Philter cutoff log, calibrated by ear
    (it shows only %, no Hz: 42 % = 500 Hz, 100 % = 20 kHz); Free Filter freq from FF_HZ (measured)"""
    s = zlib.decompress(bytes(d[4:]))
    b = 64 + struct.unpack_from('<I', s, 60)[0]
    knobs = struct.unpack_from('<12i', s, b); order, ty = struct.unpack_from('<II', s, b + 48)
    if order == 0: return None  # filter order OFF (x1, x2, x3, alt x2, alt x3 = 1..5); type 0 is SVF, not off
    return bytearray(struct.pack('<4f', {2: 2, 4: 1, 6: 1}.get(ty, 0) / 6, philter_x(knobs[1] / 256), 0.165, 0.66))

def philter_x(v):
    """Love Philter cutoff 0..1 (knob / 256) -> Free Filter freq 0..1: log, 108/256 (42 %) = 500 Hz by ear vs
    FL 8 (user), 256 = 20 kHz"""
    return ff_x(500 * 40 ** ((256 * v - 108) / 148))

def philter_links(ev, addrs):
    """links (227) to a Love Philter turned Free Filter: Filter 1 cutoff (param 9) -> Free Filter freq (param 1)
    with the source clip's values mapped through philter_x; other Love Philter params have no Free Filter
    counterpart, so those links (and their 230 formula) go. ponytail: assumes the clip spans the full range (219)"""
    out, clips, drop = [], set(), False
    for i, d in ev:
        if i == 230 and drop: drop = False; continue
        drop = False
        if i == 227 and len(d) >= 12:
            prm, tgt = struct.unpack_from('<HH', d, 8)
            if tgt in addrs:
                if prm != 9: drop = True; continue
                d = bytearray(d); d[8:10] = struct.pack('<H', 1); clips.add(struct.unpack_from('<H', d, 2)[0])
        out.append([i, d])
    ch = None
    for e in out:
        if e[0] == 64: ch = struct.unpack('<H', bytes(e[1]))[0]
        if e[0] == 234 and ch in clips:
            n = struct.unpack_from('<I', e[1], 17)[0]
            for k in range(n):
                o = 21 + 16 * k + 4
                e[1][o:o + 4] = struct.pack('<f', philter_x(struct.unpack_from('<f', e[1], o)[0]))
    return out

def slicex_notes(ev):
    """Slicex notes' mod X/Y (note bytes 18/19) drive its articulator filters; Fruity slicer's channel would
    turn them into filter cutoff/resonance spikes, so set them neutral (128)"""
    ch = None
    for i, d in ev:
        if i == 64: cur = struct.unpack('<H', d)[0]
        if i == 201 and bytes(d).rstrip(b'\0') == b'Slicex': ch = cur
    if ch is None: return
    for i, d in ev:
        if i == 224:
            for k in range(0, len(d) - 19, 20):
                if struct.unpack_from('<H', d, k + 6)[0] == ch: d[k + 18] = d[k + 19] = 128

def love_philter(ev):
    """replace each Love Philter insert slot (201, 212, 213) by Free Filter, or drop it"""
    out, k = [], 0
    while k < len(ev):
        if ev[k][0] == 201 and bytes(ev[k][1]).rstrip(b'\0') == b'Fruity Love Philter' and ev[k + 2][0] == 213:
            ff = philter_ff(ev[k + 2][1])
            if ff is not None:
                out += [[201, bytearray(b'Fruity Free Filter\0')], ev[k + 1], [213, ff]]
                t, s = struct.unpack_from('<II', bytes(ev[k + 1][1]))  # 212: mixer track, slot
                FF_ADDR.add(0x2000 + (t << 6) + s)
            k += 3; continue
        out.append(ev[k]); k += 1
    return out

VST6 = {b'Fruity Compressor', b'Fruity compressor', b'Fruity Reeverb', b'Fruity Phaser', b'Fruity Free Filter'}

def convert(ev, outdir):
    # drop FL 8's 70th mixer track: from the last 149 up to the mixer block (225)
    i149 = max(k for k, e in enumerate(ev) if e[0] == 149)
    i225 = next(k for k, e in enumerate(ev) if e[0] == 225)
    FF_ADDR.clear(); ev = love_philter(ev[:i149] + ev[i225:]); slicex_notes(ev)
    out = []; plugin = None; nenv = 0
    global TEMPO  # project tempo, for the slicer (66: u16 bpm; 156: u32 bpm x 1000)
    TEMPO = next((struct.unpack('<H', d)[0] if i == 66 else struct.unpack('<I', d)[0] / 1000
                  for i, d in ev if i in (66, 156)), 140)
    for i, d in ev:
        if i == 216 and not d: continue  # FL 6 writes no empty 216 (Pilchard)
        if i == 64: plugin = None; nenv = 0  # new channel (sampler channels have no 201)
        if i == 218:
            nenv += 1
            # Fruity slicer has no envelope of its own; FL 6 demos give slices their tail with the channel
            # volume envelope (2nd 218: flags, on, predelay, attack, hold, decay, sustain 0..128, release)
            # ponytail: NewStuff.flp's release 6500, not Slicex's own (its 29 KB state isn't decoded)
            if plugin == b'Slicex' and nenv == 2: d[4:32] = struct.pack('<7i', -1, 100, 100, 100, 100, 128, 6500)
        if i == 201:
            plugin = bytes(d).rstrip(b'\0')
            if plugin in SUBST: d = bytearray(SUBST[plugin][0] + b'\0')
        if i == 199: d = bytearray(b'6.0.4\0')
        elif i == 196: d = sample(d, outdir)
        elif i == 213 and plugin == b'Sytrus': d = sytrus(d)
        elif i == 213 and plugin in SUBST: d = SUBST[plugin][1](d, outdir)
        elif i == 212 and len(d) >= 36:
            # window info: FL 8 sets 0x40 in byte 16 and moved the u32 at 20 to 24; the rest is
            # window junk (pointers) in both
            f, v = d[16] & ~0x40, d[24:28]
            d[16:36] = bytes([f, 0, 0, 0]) + v + bytes(8) + v
            # byte 8: 2 = native Fruity plugin, 0 = VST-hosted (FL 6 runs these as Plugins/VST/*.dll)
            if (SUBST[plugin][0] if plugin in SUBST else plugin) in VST6: d[8] = 0
        elif i == 224:
            for k in range(0, len(d) - 19, 20): d[k + 14] = 0  # release: FL 6 has none
        elif i == 225:
            recs = [d[k:k + 12] for k in range(0, len(d), 12)]
            # FL 8 adds param 133 on every track and a 70th track (b7 49, b6 64..71)
            recs = [r for r in recs if not (r[5] == 31 and (r[4] == 133 or (r[7] == 49 and r[6] & 0xC0 == 64)))]
            d = bytearray(b''.join(recs))
        elif i == 235: d = d[:69]
        elif i == 233:
            for k in range(0, len(d) - 27, 28):
                if d[k + 4:k + 8] == b'\0\0\0\x60': d[k + 7] = 0x50  # row items
                elif d[k + 4:k + 6] == b'\0\x50' and d[k + 7] >= 0x50:
                    # FL 8 free pattern clip (item 0x5000 + p) -> FL 6 row item on pattern p's row
                    p = struct.unpack_from('<H', d, k + 6)[0] - 0x5000
                    d[k + 4:k + 8] = b'\0\0\0\x50'
                    d[k + 12], d[k + 14] = 0xE7 - p, 3  # byte 14: 3 on every FL 6 row item, 0 on FL 8 free clips
                    d[k + 20:k + 28] = struct.pack('<ff', -1.0, -1.0)
        elif i == 234: d = automation(d)
        out.append([i, d])
    return philter_links(out, FF_ADDR)

FF_ADDR = set()  # mixer addresses (0x2000 + track << 6 + slot) of Love Philters replaced by Free Filter

if __name__ == '__main__':
    h, ev = read(sys.argv[1])
    write(sys.argv[2], h, convert(ev, os.path.splitext(sys.argv[2])[0] + ' samples'))
