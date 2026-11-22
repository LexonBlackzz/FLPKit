// bucket <a.mid> <b.mid> <track> [bar]: note-on/off counts per bar where the two files differ
use std::collections::BTreeMap;
fn track(path: &str, t: usize) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).unwrap();
    f.seek(SeekFrom::Start(14)).unwrap();
    for k in 0..=t {
        let mut h = [0u8; 8];
        f.read_exact(&mut h).unwrap();
        let l = u32::from_be_bytes(h[4..].try_into().unwrap()) as usize;
        if k < t { f.seek(SeekFrom::Current(l as i64)).unwrap(); } else { let mut d = vec![0; l]; f.read_exact(&mut d).unwrap(); return d; }
    }
    unreachable!()
}
fn ons(d: &[u8], bar: u64) -> BTreeMap<u64, (usize, BTreeMap<(u16, u8), usize>)> {
    let (mut i, mut tick, mut rs, mut out) = (0, 0u64, 0u8, BTreeMap::new());
    let vlq = |i: &mut usize| { let mut v = 0u64; loop { let c = d[*i]; *i += 1; v = (v << 7) | (c & 127) as u64; if c < 128 { return v } } };
    while i < d.len() {
        tick += vlq(&mut i);
        if d[i] == 0xff { i += 2; let l = vlq(&mut i) as usize; i += l; continue }
        if d[i] & 0x80 != 0 { rs = d[i]; i += 1 }
        let n = if matches!(rs >> 4, 0xc | 0xd) { 1 } else { 2 };
        let on = rs >> 4 == 9 && d[i + 1] > 0;
        if on || rs >> 4 == 8 || rs >> 4 == 9 {
            // note-offs counted as velocity 255
            let e: &mut (usize, BTreeMap<_, _>) = out.entry(tick / bar).or_default();
            e.0 += 1; *e.1.entry(((rs & 15) as u16 * 1000 + d[i] as u16, if on { d[i + 1] } else { 255 })).or_default() += 1; // key = channel*1000 + key
        }
        i += n;
    }
    out
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = a[3].parse().unwrap();
    let bar = a.get(4).map_or(3840, |s| s.parse().unwrap());
    let (x, y) = (ons(&track(&a[1], t), bar), ons(&track(&a[2], t), bar));
    let keys: std::collections::BTreeSet<_> = x.keys().chain(y.keys()).collect();
    let mut shown = 0;
    let mut tot = (0i64, 0usize);
    for k in keys {
        let (p, q) = (x.get(k).map_or(0, |v| v.0), y.get(k).map_or(0, |v| v.0));
        if x.get(k).map(|v| &v.1) == y.get(k).map(|v| &v.1) { continue }
        tot.0 += p as i64 - q as i64; tot.1 += 1;
        if shown < 30 {
            shown += 1;
            let sample = |m: Option<&(usize, BTreeMap<(u16, u8), usize>)>, o: Option<&(usize, BTreeMap<(u16, u8), usize>)>| -> Vec<((u16, u8), i64)> {
                m.map_or(vec![], |m| m.1.iter().map(|(kk, &c)| (*kk, c as i64 - o.and_then(|o| o.1.get(kk)).map_or(0, |&z| z as i64))).filter(|z| z.1 > 0).take(4).collect())
            };
            println!("bar {k} tick {} ours {p} fl {q} ours+ {:?} fl+ {:?}", k * bar, sample(x.get(k), y.get(k)), sample(y.get(k), x.get(k)));
        }
    }
    println!("{} bars differ, net {}", tot.1, tot.0);
}
