// tracks <file.mid>: per track: name, note-ons, MIDI channels, program changes / bank selects (first few)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    let mut i = 14;
    let mut n = 0;
    while i + 8 <= d.len() {
        let l = u32::from_be_bytes(d[i + 4..i + 8].try_into().unwrap()) as usize;
        let t = &d[i + 8..i + 8 + l];
        let (mut j, mut rs, mut tick) = (0, 0u8, 0u64);
        let (mut name, mut ons, mut chans, mut progs, mut ctl) = (String::new(), 0, std::collections::BTreeSet::new(), vec![], 0);
        let vlq = |j: &mut usize| { let mut v = 0u64; loop { let c = t[*j]; *j += 1; v = (v << 7) | (c & 127) as u64; if c < 128 { return v } } };
        while j < t.len() {
            tick += vlq(&mut j);
            if t[j] == 0xff { let ty = t[j + 1]; j += 2; let k = vlq(&mut j) as usize; if ty == 3 && name.is_empty() { name = t[j..j + k].iter().map(|&b| b as char).collect() } j += k; continue }
            if t[j] == 0xf0 || t[j] == 0xf7 { j += 1; let k = vlq(&mut j) as usize; j += k; continue }
            if t[j] & 0x80 != 0 { rs = t[j]; j += 1 }
            let (st, d1) = (rs >> 4, t[j]);
            let d2 = if matches!(st, 0xc | 0xd) { 0 } else { t[j + 1] };
            if st == 9 && d2 > 0 { ons += 1; chans.insert(rs & 15); }
            if st == 0xc && progs.len() < 6 { progs.push(format!("{tick}:ch{} P{d1}", rs & 15)) }
            if st == 0xb && (d1 == 0 || d1 == 32) && progs.len() < 6 { progs.push(format!("{tick}:ch{} B{d1}={d2}", rs & 15)) }
            if st == 0xb || st == 0xe { ctl += 1 }
            j += if matches!(st, 0xc | 0xd) { 1 } else { 2 };
        }
        println!("t{n:<2} {:<24} ons {ons:<7} ctl {ctl:<6} ch {:?} {}", name, chans, progs.join(" "));
        i += 8 + l; n += 1;
    }
}
