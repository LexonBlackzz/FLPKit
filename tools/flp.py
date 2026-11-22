"""FLP event reader / writer for the Python tools."""

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
