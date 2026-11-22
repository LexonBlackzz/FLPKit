"""fl8to5.py in.flp out.flp: FL 8.0 -> FL 6.0 (fl8to6) -> FL 5.0 (fl6to5), plus FL 5 plugin substitutes.
(Sytrus via sy15to7, Multiband Compressor and Delay Bank via FL 5 substitutes below.)"""
import sys, os, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fl8to6, fl6to5
from fl8to6 import read, write, IL

fl8to6.FL6_DATA = IL + 'FLStudio5/Data'  # copy every sample FL 5's Data lacks

def delay2(_, ev):
    """Delay Bank (FL 6) -> Fruity Delay 2 with the song's own first Delay 2 state, dry 0: Delay Bank sits on a
    send, which should add only the echoes. Delay 2 state: 8 i32 [input pan][input vol][feedback vol 0..160]
    [feedback cut][time][offset][mode 0 normal, 1 invert, 2 ping-pong][dry 0..128].
    ponytail: not a translation of Delay Bank's taps"""
    plugin, st = None, struct.pack('<8i', 0, 128, 128, 117, 192, 0, 2, 0)
    for i, d in ev:
        if i == 201: plugin = bytes(d).rstrip(b'\0')
        if i == 213 and plugin == b'Fruity Delay 2': st = bytes(d); break
    st = bytearray(st); struct.pack_into('<i', st, 28, 0)
    return st

PRESETS = IL + 'FLStudio5/Data/Patches/Plugin presets/Effects/Fruity Compressor/'

def compressor(d, _):
    """Multiband Compressor (FL 6) -> Fruity Compressor: FL 5's "FastMaster" preset where it stands in for
    Maximus (FL 6's "Soft limiter" state), else "Light". ponytail: presets, not a translation of the bands"""
    soft = fl8to6.maximus(None, None)
    _, ev = read(PRESETS + ('FastMaster.fst' if bytes(d) == bytes(soft) else 'Light.fst'))
    return bytearray(next(e[1] for e in ev if e[0] == 213))

fl6to5.SUBST.update({b'Fruity Multiband Compressor': (b'Fruity Compressor', compressor),
                     b'Fruity Delay Bank': (b'Fruity Delay 2', delay2)})

if __name__ == '__main__':
    h, ev = read(sys.argv[1])
    write(sys.argv[2], h, fl6to5.convert(fl8to6.convert(ev, os.path.splitext(sys.argv[2])[0] + ' samples')))
