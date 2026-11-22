"""fl8to7.py in.flp out.flp: FL 8.0 -> FL 7.0 rewrite (rules from the 9 FL 7.0.0 / 8.0.0 demo pairs).
FL 7 already has FL 8's Sytrus v18, window info, free clips, automation v2, release byte, Love Philter
and Parametric EQ 2; FL 8 only adds a 70th mixer track and mixer param 133. Maximus and Slicex (FL 8
only) get the same substitutes as for FL 6."""
import sys, os, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fl8to6
from fl8to6 import read, write, IL

fl8to6.FL6_DATA = IL + 'FL Studio 7/Data'  # sample(): copy only samples FL 7 lacks

SUBST = {b'Maximus': fl8to6.SUBST[b'Maximus'], b'Slicex': fl8to6.SUBST[b'Slicex']}

def convert(ev, outdir):
    i149 = max(k for k, e in enumerate(ev) if e[0] == 149)
    i225 = next(k for k, e in enumerate(ev) if e[0] == 225)
    ev = ev[:i149] + ev[i225:]  # drop the 70th mixer track
    fl8to6.slicex_notes(ev)
    fl8to6.TEMPO = next((struct.unpack('<H', d)[0] if i == 66 else struct.unpack('<I', d)[0] / 1000
                         for i, d in ev if i in (66, 156)), 140)
    out = []; plugin = None; nenv = 0
    for i, d in ev:
        if i == 64: plugin = None; nenv = 0
        if i == 218:
            nenv += 1  # slicer gets Slicex's tail from the channel volume envelope (see fl8to6)
            if plugin == b'Slicex' and nenv == 2: d[4:32] = struct.pack('<7i', -1, 100, 100, 100, 100, 128, 6500)
        if i == 201:
            plugin = bytes(d).rstrip(b'\0')
            if plugin in SUBST: d = bytearray(SUBST[plugin][0] + b'\0')
        if i == 199: d = bytearray(b'7.0.0\0')
        elif i == 196: d = fl8to6.sample(d, outdir)
        elif i == 213 and plugin in SUBST: d = SUBST[plugin][1](d, outdir)
        elif i == 213 and plugin == b'Fruity Parametric EQ 2': d[0] = 1  # FL 8 v2, same layout
        elif i == 225:
            recs = [d[k:k + 12] for k in range(0, len(d), 12)]
            recs = [r for r in recs if not (r[5] == 31 and (r[4] == 133 or (r[7] == 49 and r[6] & 0xC0 == 64)))]
            d = bytearray(b''.join(recs))
        elif i == 235: d = d[:69]
        out.append([i, d])
    return out

if __name__ == '__main__':
    h, ev = read(sys.argv[1])
    write(sys.argv[2], h, convert(ev, os.path.splitext(sys.argv[2])[0] + ' samples'))
