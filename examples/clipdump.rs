// clipdump <flp> <track#> <lo> <hi> [vel]: clips putting notes of that MIDI track's channel into [lo,hi)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = flparser::read_input(&a[1]).unwrap();
    let p = flparser::Project::parse(&b).unwrap();
    let t: usize = a[2].parse().unwrap(); let (lo, hi): (u64, u64) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let ch = p.used_channels()[t - 2];
    println!("track {t} = rack channel {ch} {:?}", p.channels[ch as usize].name);
    for (i, c) in p.clips.iter().enumerate() {
        let vel: Option<u8> = a.get(5).map(|v| v.parse().unwrap());
        let n = p.patterns[&c.pattern].notes.iter().filter(|n| n.chan == ch && vel.is_none_or(|v| n.vel == v)).filter_map(|n| c.at(n.pos)).filter(|&x| x >= lo && x < hi).count();
        if n > 0 { println!("clip#{i} pos {} len {} play {} pat {} start {} track {} notes {n}", c.pos, c.len, c.play, c.pattern, c.start, c.track); }
    }
}
