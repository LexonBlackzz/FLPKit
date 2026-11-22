"""Sytrus state v18 (FL 8) -> v15 (FL 6), inflated zlib payload (after the u32 version)."""
import struct, math

def _curve_at(a, k):
    """[u32 t in (1,3)][u32 2][u32 n][n x (f64 x, f64 y, f32 tension, u32 mode)] at k? -> n or None"""
    if k + 12 > len(a): return None
    t, v, n = struct.unpack_from('<III', a, k)
    if t > 8 or v != 2 or not 0 < n <= 4096 or k + 12 + 24 * n > len(a): return None
    for j in range(n):
        x, y, te, m = struct.unpack_from('<ddfI', a, k + 12 + 24 * j)
        if not (math.isfinite(x) and math.isfinite(y) and abs(te) <= 1.0001 and m < 0x10000 and -1e6 < x < 1e6): return None
    return n

def curves(a):
    """curve objects v2 (f64 points) -> v1 (f32 points), as in automation clips (event 234)"""
    out = bytearray(); o = k = 0
    while k < len(a) - 12:
        n = _curve_at(a, k) if a[k + 4:k + 8] == b'\x02\x00\x00\x00' else None
        if n is None: k += 1; continue
        head = bytearray(a[o:k])
        # type-1 curves sit in a wrapper [u32 2][u32 0][f32][f32]: its version 2 -> 1 too
        if a[k] == 1 and k - o >= 16 and head[-16:-12] == b'\x02\x00\x00\x00' and head[-12:-8] == bytes(4):
            head[-16] = 1
        out += head + a[k:k + 4] + b'\x01\x00\x00\x00' + a[k + 8:k + 12]
        for j in range(n):
            x, y, te, m = struct.unpack_from('<ddfI', a, k + 12 + 24 * j)
            out += struct.pack('<fffI', x, y, te, m)
        k = o = k + 12 + 24 * n
    return bytes(out + a[o:])

FF8 = bytes([255]) * 8
RTF_END = bytes([0x7d, 0x0d, 0x0a, 0x00])
Z512 = bytes(512)

def tables(a):
    """per operator: two f32 tables of 256 (FL 8) -> 128 (FL 6) entries, 52 bytes after the operator
    start (after the preset's RTF text or an 8 x ff run); FL 8 pads the upper halves with zeros"""
    out = bytearray(); o = h = 0
    while h < len(a) - 2100:
        x = h + 52
        if (a[h - 8:h] == FF8 or a[h - 4:h] == RTF_END) and a[x + 512:x + 1024] == Z512 and a[x + 1536:x + 2048] == Z512:
            out += a[o:x] + a[x:x + 512] + a[x + 1024:x + 1536]
            o = h = x + 2048
            continue
        h += 1
    return bytes(out + a[o:])

def convert(a):
    # FL 8 inserts 3 bytes (01 00 01 in all 20 demo presets) at 974 of the global header,
    # (the f32 at 768 also differs in 18 of 20 pairs, 0x3f74026b vs 0x3f741077: a parameter, kept)
    assert a[974:977] == bytes([1, 0, 1]), a[974:977]
    a = a[:974] + a[977:]
    return tables(curves(a))
