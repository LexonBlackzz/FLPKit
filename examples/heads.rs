// heads <file.mid> <track> [<key> <lo> <hi>]: a track's first and last 30 non-note events
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(&a[1]).unwrap();
        f.seek(SeekFrom::Start(14)).unwrap();
        let t: usize = a[2].parse().unwrap();
        let mut d = vec![];
        for k in 0..=t {
            let mut h = [0u8; 8];
            f.read_exact(&mut h).unwrap();
            let l = u32::from_be_bytes(h[4..].try_into().unwrap()) as usize;
            if k < t { f.seek(SeekFrom::Current(l as i64)).unwrap(); } else { d = vec![0; l]; f.read_exact(&mut d).unwrap(); }
        }
        d
    };
    let (mut i, mut tick, mut rs, mut ev) = (0, 0u64, 0u8, vec![]);
    let vlq = |i: &mut usize| { let mut v = 0u64; loop { let c = d[*i]; *i += 1; v = (v << 7) | (c & 127) as u64; if c < 128 { return v } } };
    while i < d.len() {
        tick += vlq(&mut i);
        let s = i;
        if d[i] == 0xff { i += 2; let l = vlq(&mut i) as usize; i += l; ev.push((tick, d[s..i].to_vec())); continue }
        let b = if d[i] & 0x80 != 0 { rs = d[i]; i += 1; vec![] } else { vec![rs] };
        i += if matches!(rs >> 4, 0xc | 0xd) { 1 } else { 2 };
        ev.push((tick, [b, d[s..i].to_vec()].concat()));
    }
    let all = ev.len();
    if let (Some(k), Some(lo), Some(hi)) = (a.get(3), a.get(4), a.get(5)) {
        // heads <file> <track> <key> <lo> <hi>: that key's note events in [lo, hi)
        let (k, lo, hi): (u8, u64, u64) = (k.parse().unwrap(), lo.parse().unwrap(), hi.parse().unwrap());
        for (t, m) in ev.iter().filter(|e| matches!(e.1[0] >> 4, 8 | 9) && e.1[1] == k && (lo..hi).contains(&e.0)) {
            println!("  {t:>9} {}", m.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
        }
        return;
    }
    let ev: Vec<_> = ev.into_iter().filter(|e| !matches!(e.1[0] >> 4, 8 | 9)).collect();
    let show = |e: &[(u64, Vec<u8>)]| for (t, m) in e {
        println!("  {t:>9} {}", m.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
    };
    println!("{all} events, {} non-note; head:", ev.len());
    if std::env::var("ALL").is_ok() { show(&ev); return; }
    show(&ev[..30.min(ev.len())]);
    println!(" tail:");
    show(&ev[ev.len().saturating_sub(30)..]);
}
