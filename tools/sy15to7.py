"""Sytrus state v15 (FL 6) -> v7 (FL 5), inflated zlib payload (after the u32 version). Learned from the one
patch both ship (ScottFisher-Moholithic Filter: FL 5's 4.9 save v7, FL 6's v15); the layout is otherwise equal.
ponytail: harmonic tables cut to FL 5's 24 entries (FL 6 has 128); where zero runs make the alignment ambiguous
the cut points are guesses that give the same bytes on that pair."""
import struct, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sy18to15 import _curve_at

def _curves(a):
    """(start, end) of each v1 curve object [u32 type][u32 1][u32 n][n x 16 bytes]"""
    out, k = [], 0
    while k < len(a) - 12:
        if a[k + 4:k + 8] == b'\x01\0\0\0':
            t, v, n = struct.unpack_from('<III', a, k)
            if 0 < t <= 8 and 0 < n <= 4096 and k + 12 + 16 * n <= len(a) and all(
                    abs(struct.unpack_from('<f', a, k + 12 + 16 * j)[0]) < 1e6 and
                    abs(struct.unpack_from('<f', a, k + 20 + 16 * j)[0]) <= 1.0001 for j in range(n)):
                out.append((k, k + 12 + 16 * n)); k += 12 + 16 * n; continue
        k += 1
    return out

def _op(g, h):
    """operator block (h = where its fields start): v15 adds a u32 at h+48, 2 tables of 128 f32 (v7: 24), and
    reshapes the 12 bytes from E-28: [80..] v15 = 8 + 1 + 3 bytes, v7 = 8 + 4 zero + 3"""
    e = len(g)
    p = g[e - 28:e - 16]
    return (g[:h + 48] + g[h + 52:h + 148] + g[h + 564:h + 660] + g[h + 1076:e - 28]
            + p[:4] + g[h + 48:h + 52] + bytes(4) + p[8:11] + g[e - 16:])  # v15 h+48 = noise 0..128, v7 keeps it here (user's FL 5 save)

def convert(b):
    # header before the RTF text: v15 adds 6 bytes at 966, 38 at 1043 and a u32 index on 3 17-byte records
    cut = [(966, 972), (1043, 1081), (1095, 1099), (1112, 1116), (1129, 1133)]
    assert struct.unpack_from('<I', b, 1129)[0] == 2, 'unexpected v15 header'
    head, o = b'', 0
    for s, e in cut: head += b[o:s]; o = e
    rtf_end = 1137 + struct.unpack_from('<I', b, 1133)[0]
    head += b[o:rtf_end]
    cv = _curves(b)
    out = head + _op(b[rtf_end:cv[0][0]], 0)
    for j, (s, e) in enumerate(cv):
        out += b[s:e]
        g = b[e:cv[j + 1][0]] if j + 1 < len(cv) else b[e:]
        if len(g) == 1140 and g[4:20] == b'\xff' * 16: g = _op(g, 20)
        elif len(g) == 108: g = g[:-7]
        elif len(g) == 37: g = g[:20] + g[21:]
        out += g
    return out

if __name__ == '__main__':  # self-check on the pair: python sy15to7.py v15.bin v7.bin
    a, b = open(sys.argv[1], 'rb').read(), open(sys.argv[2], 'rb').read()
    c = convert(a)
    d = [k for k in range(min(len(b), len(c))) if b[k] != c[k]]
    print(len(c), len(b), 'diff bytes', len(d), d[:20])
