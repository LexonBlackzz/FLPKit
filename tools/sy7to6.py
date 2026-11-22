"""Sytrus state v7 (FL 5) -> v6 (FL 4.5), inflated zlib payload (after the u32 version). Learned from the one
patch both ship: "Synth string - fat texture" (FL 4.5.2 save v6, FL 4.9.0 save v7); same length after these rules.
ponytail: the 1 -> 0 flag bytes are all 1 in v7 and 0 in v6 on that pair (gap36 byte 20 also holds 64/128 values,
left as is); meaning not decoded."""
import struct, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sy15to7 import _curves

def _flag(g, k):
    return g[:k] + (b'\0' if g[k] == 1 else g[k:k + 1]) + g[k + 1:]

def convert(b):
    cv = _curves(b)
    r = 1081 + struct.unpack_from('<I', b, 1077)[0]  # op 0 fields start (v7 coords)
    head = _flag(b[:cv[0][0]], r + 271)
    out = bytearray(head[:967] + head[968:])  # v7 adds a byte at 967
    for j, (s, e) in enumerate(cv):
        t, _, n = struct.unpack_from('<III', b, s)
        out += struct.pack('<III', t, 0, n)  # curve version 1 -> 0, points 16 -> 12 bytes
        for k in range(n): out += b[s + 12 + 16 * k:s + 24 + 16 * k]
        g = b[e:cv[j + 1][0]] if j + 1 < len(cv) else b[e:]
        if len(g) == 307: g = _flag(g, 20 + 271)
        elif len(g) == 36: g = _flag(g, 20)
        elif len(g) == 76: g = _flag(g, 60)
        out += g
    return bytes(out)

if __name__ == '__main__':  # self-check on the pair: python sy7to6.py v7.bin v6.bin
    a, b = open(sys.argv[1], 'rb').read(), open(sys.argv[2], 'rb').read()
    c = convert(a)
    d = [k for k in range(min(len(b), len(c))) if b[k] != c[k]]
    print(len(c), len(b), 'diff bytes', len(d), d[:20])
