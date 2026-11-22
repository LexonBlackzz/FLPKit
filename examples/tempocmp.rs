// tempocmp <file.flp> <reference.mid>: our tempo track vs the reference's, tick by tick
fn tempos(d: &[u8]) -> Vec<(u64, u32)> {
    let (mut i, mut tick, mut out) = (0, 0u64, vec![]);
    let vlq = |i: &mut usize| { let mut v = 0u64; loop { let c = d[*i]; *i += 1; v = (v << 7) | (c & 127) as u64; if c < 128 { return v } } };
    while i < d.len() {
        tick += vlq(&mut i);
        if d[i] == 0xff {
            let ty = d[i + 1];
            i += 2;
            let l = vlq(&mut i) as usize;
            if ty == 0x51 { out.push((tick, u32::from_be_bytes([0, d[i], d[i + 1], d[i + 2]]))) }
            i += l;
        } else { panic!("non-meta event in tempo track") }
    }
    out
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = flparser::Project::parse(&flparser::read_input(&a[1]).unwrap()).unwrap();
    let ours = tempos(&p.tempo_track_data());
    let r = std::fs::read(&a[2]).unwrap();
    let (mut i, mut k) = (14, 0);
    let theirs = loop {
        let l = u32::from_be_bytes(r[i + 4..i + 8].try_into().unwrap()) as usize;
        if k == 1 { break tempos(&r[i + 8..i + 8 + l]) }
        i += 8 + l; k += 1;
    };
    // by tick: the last tempo written on each tick
    let o: std::collections::BTreeMap<u64, u32> = ours.iter().copied().collect();
    let t: std::collections::BTreeMap<u64, u32> = theirs.iter().copied().collect();
    let both = t.keys().filter(|k| o.contains_key(k)).count();
    let same = t.iter().filter(|(k, v)| o.get(k) == Some(v)).count();
    let near = t.iter().filter(|(k, v)| o.get(k).is_some_and(|x| x.abs_diff(**v) <= 4)).count();
    println!("events ours {} FL {}; FL ticks {}: on our ticks {both}, same {same}, within 4us {near}", ours.len(), theirs.len(), t.len());
}
