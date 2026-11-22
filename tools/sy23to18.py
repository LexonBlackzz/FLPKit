"""Sytrus state v23 (FL 12, `[u16 3][u16 23][zlib]`) -> v18 (FL 8, `[u32 18][zlib]`), inflated payloads. Learned
from SolidInc's 2 patches saved by FL 8.0 and FL 12.3: equal after these rules except filter cutoff knobs that
SolidInc automates (FL 12 saved them at their automated position).
- byte 977: v23 inserts one
- patch text: v23 [varint n][n UTF-16 chars, plain text], v18 [u32 len][RTF, ends "}\\r\\n\\0"]
- curves [type][u32 version][n][n x (f64 x, f64 y, f32 tension, u32 mode)]: version 3 -> 2, mode's top byte
  (0..2 in v23, 0 in v18) cleared
- version bytes 3 -> 2: op + 2128 (op 0 right after the text, ops 1..5 20 bytes into their 2164-byte section),
  36-byte sections at 20, 37-byte at 21, 76-byte at 60
- 62 sections of 40 bytes lose their last 4 (v18: 36)
ponytail: RTF rebuilt from the plain text with FL 8's preset template (colors/fonts as FL 8's default patch)"""
import struct, sys, os, math

def _curve_at(a, k, ver):
    if k + 12 > len(a): return None
    t, v, n = struct.unpack_from('<III', a, k)
    if not 0 < t <= 8 or v != ver or not 0 < n <= 4096 or k + 12 + 24 * n > len(a): return None
    for j in range(n):
        x, y, te, _ = struct.unpack_from('<ddfI', a, k + 12 + 24 * j)
        if not (math.isfinite(x) and math.isfinite(y) and abs(te) <= 1.0001 and -1e6 < x < 1e6): return None
    return n

def _curves(a, ver):
    out, k = [], 0
    while k < len(a) - 12:
        n = _curve_at(a, k, ver) if a[k + 4:k + 8] == bytes([ver, 0, 0, 0]) else None
        if n: out.append((k, k + 12 + 24 * n)); k += 12 + 24 * n
        else: k += 1
    return out

def rtf(text):
    esc = text.replace('\\', '\\\\').replace('{', '\\{').replace('}', '\\}')
    lines = esc.replace('\r\n', '\r').replace('\n', '\r').split('\r')
    body = '\\cf1\\b\\f0\\fs16 ' + lines[0] + '\\b0\r\n' + ''.join('\\par \\cf2 ' + l + '\r\n' for l in lines[1:])
    s = ('{\\rtf1\\ansi\\ansicpg1252\\deff0\\deflang1033{\\fonttbl{\\f0\\fswiss\\fprq2 Arial;}}\r\n'
         '{\\colortbl ;\\red196\\green198\\blue211;\\red131\\green134\\blue154;}\r\n'
         '\\viewkind4\\uc1\\pard ' + body + '\\par }\r\n')
    return s.encode('latin1', 'replace') + b'\0'

def _flag(g, k):
    return g[:k] + (b'\x02' if g[k] == 3 else g[k:k + 1]) + g[k + 1:]

def convert(b):
    b = b[:977] + b[978:]
    o, n, sh = 1136, 0, 0
    while True:  # varint char count
        x = b[o]; o += 1; n |= (x & 0x7f) << sh; sh += 7
        if x < 0x80: break
    text = b[o:o + 2 * n].decode('utf-16le', 'replace'); o += 2 * n
    r = rtf(text)
    cv = _curves(b, 3)
    op0 = _flag(b[o:cv[0][0]], 2128)
    out = bytearray(b[:1136] + struct.pack('<I', len(r)) + r + op0)
    for j, (s, e) in enumerate(cv):
        c = bytearray(b[s:e]); c[4] = 2
        for k in range(12, len(c), 24): c[k + 23] = 0
        out += c
        g = b[e:cv[j + 1][0]] if j + 1 < len(cv) else b[e:]
        if len(g) == 2164: g = _flag(g, 2148)
        elif len(g) == 40: g = g[:36]
        elif len(g) == 36: g = _flag(g, 20)
        elif len(g) == 37: g = _flag(g, 21)
        elif len(g) == 76: g = _flag(g, 60)
        out += g
    return bytes(out)

if __name__ == '__main__':  # self-check: python sy23to18.py v23.bin v18.bin (the RTF text region is skipped)
    a, b = open(sys.argv[1], 'rb').read(), open(sys.argv[2], 'rb').read()
    c = convert(a)
    ra = 1140 + struct.unpack_from('<I', c, 1136)[0]; rb = 1140 + struct.unpack_from('<I', b, 1136)[0]
    c, b = c[ra:], b[rb:]
    d = [k for k in range(min(len(b), len(c))) if b[k] != c[k]]
    print(len(c), len(b), 'diff bytes after the text', len(d), d[:20])
