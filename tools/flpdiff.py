"""flpdiff.py a.flp b.flp [ids]: per event id, compare two saves of one song (counts, sizes,
which byte offsets differ). Aligns events of an id by occurrence order."""
import sys, collections

def events(p):
    b = open(p, 'rb').read()
    assert b[:4] == b'FLhd'
    o = 8 + int.from_bytes(b[4:8], 'little')
    assert b[o:o + 4] == b'FLdt'
    o += 8
    out = []
    while o < len(b):
        i = b[o]; o += 1
        if i < 64: n = 1
        elif i < 128: n = 2
        elif i < 192: n = 4
        else:
            n = 0; s = 0
            while True:
                c = b[o]; o += 1; n |= (c & 127) << s; s += 7
                if c < 128: break
        out.append((i, b[o:o + n])); o += n
    return out

a, b = events(sys.argv[1]), events(sys.argv[2])
only = set(map(int, sys.argv[3].split(','))) if len(sys.argv) > 3 else None
A = collections.defaultdict(list); B = collections.defaultdict(list)
for i, d in a: A[i].append(d)
for i, d in b: B[i].append(d)
for i in sorted(set(A) | set(B)):
    if only and i not in only: continue
    x, y = A[i], B[i]
    sx = sorted(set(len(d) for d in x)); sy = sorted(set(len(d) for d in y))
    diffpos = collections.Counter(); same = 0
    for d, e in zip(x, y):
        if d == e: same += 1; continue
        if len(d) == len(e):
            for k in range(len(d)):
                if d[k] != e[k]: diffpos[k] += 1
    tag = '' if len(x) == len(y) and same == len(x) else ' *'
    print(f"{i:>3} n {len(x)}/{len(y)} same {same} sizes {sx[:4]}{'..' if len(sx)>4 else ''}/{sy[:4]}{'..' if len(sy)>4 else ''}"
          f" diffpos {sorted(diffpos.items())[:12]}{tag}")
