"""FL 5 automation clips -> FL 4.5 (no clips there): each linked clip becomes a Fruity X-Y Controller in a free
mixer slot whose X knob (param 0, 0..65536) is automated by pattern event automation (223); the clip's own 227
links get the X-Y output as their source, so FL 4 maps X onto each target's range (and keeps 230 formulas).
Each clip placement (channel, start offset, length) becomes one pattern, placed on the playlist where the clip was.
Runs on FL 5 events before fl5to4 (which turns the clip channels into empty samplers).
ponytail: sampled every 6 ticks (1/16 beat at PPQ 96) + at points; clip LFOs and unlinked clips ignored."""
import struct, math

XY_212 = bytes.fromhex('0d0000000100000002000000000000002000000088f96400011f00208554390100004003040000007c000000b9000000'
                       '1b010000')  # NewStuff's X-Y window info; u32 0 = mixer track, u32 4 = slot
XY_213 = bytes.fromhex('010000000080000000000000000001000000000000800000000000000000010000000000000600000004000000074465'
                       '6661756c74000000000000000000010000000000000046000000')  # Default.fst: [1][X][min][max 65536]..
STEP = 6

def curve(pts, b):
    """clip value at beat b: points [(Δbeat, value, tension, mode)], segment shape from its end point (mode 0)"""
    x = 0.0
    for j, (dx, v, t, _) in enumerate(pts):
        x0, x = x, x + dx
        if b < x and j:
            v0 = pts[j - 1][1]; f = max(0.0, (b - x0) / (x - x0)) if x > x0 else 1.0
            a = -t * math.log(1e6)
            g = f if abs(a) < 1e-9 else math.expm1(a * f) / math.expm1(a)
            return v0 + (v - v0) * g
    return pts[-1][1] if b >= x else pts[0][1]

def convert(ev):
    ch, clips, mix, track, has_fx, routed = None, {}, False, 0, set(), set()
    for i, d in ev:
        if i == 233: mix = True
        if not mix:
            if i == 64: ch = int.from_bytes(d, 'little')
            if ch is None: continue
            if i == 21 and d[0] == 5: clips[ch] = {'name': b'Automation', 'lo': 0, 'hi': 128}
            c = clips.get(ch)
            if c is None: continue
            if i == 192: c['name'] = bytes(d)
            if i == 219: c['lo'], c['hi'] = struct.unpack_from('<ii', d, 0)
            if i == 234:
                n = struct.unpack_from('<I', d, 17)[0]
                c['pts'] = [struct.unpack_from('<fffI', d, 21 + 16 * k) for k in range(n)]
            if i == 22: routed.add(d[0])
        else:
            if i == 201: has_fx.add(track)
            if i == 147:
                if 0 < int.from_bytes(d, 'little') < 65: routed.add(int.from_bytes(d, 'little'))
                track += 1
    linked = sorted({struct.unpack_from('<H', d, 2)[0] for i, d in ev if i == 227 and d[1] == 0
                     and struct.unpack_from('<H', d, 2)[0] in clips})
    linked = [c for c in linked if clips[c].get('pts')]
    if not linked: return ev
    free = [t for t in range(64, 0, -1) if t not in has_fx and t not in routed]
    slots = {c: (free[k // 8], k % 8) for k, c in enumerate(linked)}
    addr = lambda c: 0x2000 + (slots[c][0] << 6) + slots[c][1]
    xval = lambda c, v: max(0, min(65536, round(65536 * (clips[c]['lo'] + v * (clips[c]['hi'] - clips[c]['lo'])) / 128)))

    pats = [int.from_bytes(d, 'little') for i, d in ev if i == 65]
    nxt = max(pats) + 1
    placements, newpats, items = {}, [], []
    for i, d in ev:
        if i != 233: continue
        for k in range(0, len(d), 28):
            pos, base, c, ln = struct.unpack_from('<IHHI', d, k)
            if base != 0x5000 or c not in slots: continue
            s = struct.unpack_from('<f', d, k + 20)[0]; s = 0.0 if s < 0 else s
            key = (c, round(s, 4), ln)
            if key not in placements:
                pts = clips[c]['pts']; recs, last = [], None
                xs = sorted({t for t in range(0, ln, STEP)} | {round((x - s) * 96) for x in
                            [sum(p[0] for p in pts[:j + 1]) for j in range(len(pts))] if 0 <= (x - s) * 96 < ln})
                for t in xs:
                    v = xval(c, curve(pts, s + t / 96))
                    if v != last: recs.append(struct.pack('<IHHi', t, 0, addr(c), v)); last = v
                recs.append(struct.pack('<IHHi', ln - 1, 0, addr(c), last))  # pattern length = clip length
                placements[key] = nxt; newpats.append((nxt, c, b''.join(recs))); nxt += 1
            items.append(struct.pack('<IHHIHHIii', pos, 0, 0x5000, ln, (0x78E7 - placements[key]) & 0xffff, 3,
                                     0x80806440, -1, -1))
    first = {c: xval(c, clips[c]['pts'][0][1]) for c in slots if not any(pc == c for _, pc, _ in newpats)}
    for p, c, r in newpats:
        first.setdefault(c, struct.unpack_from('<i', r, 8)[0])

    out, ids = [], [i for i, d in ev]
    pat_end = min(ids.index(k) for k in (226, 227, 64) if k in ids)
    name_at = ids.index(233) if 233 in ids else len(ids)
    name_at = max(j for j in range(name_at) if ids[j] == 151 or j == 0) + 1  # after the last pattern name block
    color = next((d for i, d in ev if i == 150), bytearray(struct.pack('<I', 13156792)))
    track = 0; mix = False
    for j, (i, d) in enumerate(ev):
        if j == pat_end:
            for p, c, r in newpats: out += [[65, bytearray(struct.pack('<H', p))], [223, bytearray(r)]]
        if j == name_at:
            for p, c, r in newpats:
                out += [[65, bytearray(struct.pack('<H', p))], [193, bytearray(clips[c]['name'])],
                        [150, bytearray(color)], [151, bytearray(4)]]
        if i == 233:
            mix = True
            d = bytearray(bytes(d) + b''.join(items))
            recs = sorted((bytes(d[k:k + 28]) for k in range(0, len(d), 28)), key=lambda r: struct.unpack_from('<I', r)[0])
            d = bytearray(b''.join(recs))
        if i == 227:
            c = struct.unpack_from('<H', d, 2)[0]
            if d[1] == 0 and c in slots:
                d = bytearray(d); d[0:8] = struct.pack('<BBHBBH', 0, 0, addr(c), 0, 0, addr(c))
        if mix and i == 149:
            for c in linked:
                t, s = slots[c]
                if t == track:
                    st = bytearray(XY_213); st[4:8] = struct.pack('<I', first[c])
                    out += [[201, bytearray(b'Fruity X-Y Controller\0')],
                            [212, bytearray(struct.pack('<II', t, s) + XY_212[8:])], [213, st]]
        if mix and i == 147: track += 1
        out.append([i, d])
    return out

if __name__ == '__main__':  # curve self-check
    pts = [(0, 0.0, 0, 0), (4, 1.0, 0, 0), (4, 0.5, 0.5, 0)]
    assert curve(pts, 2) == 0.5 and curve(pts, 9) == 0.5 and curve(pts, -1) == 0.0
    assert 0.5 < curve(pts, 6) < 1.0  # tensioned segment stays between its ends
    print('ok')
