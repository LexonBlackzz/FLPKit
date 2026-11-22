// tickat <file.mid> <seconds>: tick at that time from the file's tempo map
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    let want: f64 = a[2].parse().unwrap();
    let ppq = u16::from_be_bytes([d[12], d[13]]) as f64;
    let (mut i, mut tempos) = (14, vec![(0u64, 500000u32)]);
    while i + 8 <= d.len() {
        let l = u32::from_be_bytes(d[i + 4..i + 8].try_into().unwrap()) as usize;
        let t = &d[i + 8..i + 8 + l];
        let (mut j, mut rs, mut tick) = (0, 0u8, 0u64);
        let vlq = |j: &mut usize| { let mut v = 0u64; loop { let c = t[*j]; *j += 1; v = (v << 7) | (c & 127) as u64; if c < 128 { return v } } };
        while j < t.len() {
            tick += vlq(&mut j);
            if t[j] == 0xff { let ty = t[j + 1]; j += 2; let n = vlq(&mut j) as usize; if ty == 0x51 { tempos.push((tick, u32::from_be_bytes([0, t[j], t[j + 1], t[j + 2]]))) } j += n; continue }
            if t[j] & 0x80 != 0 { rs = t[j]; j += 1 }
            j += if matches!(rs >> 4, 0xc | 0xd) { 1 } else { 2 };
        }
        i += 8 + l;
        if !tempos.is_empty() && i > 14 + 8 + 4 && tempos.len() > 1 { break }
    }
    tempos.sort_by_key(|x| x.0);
    let (mut us, mut prev) = (0f64, tempos[0]);
    for &(tk, tp) in &tempos[1..] {
        let next = us + (tk - prev.0) as f64 * prev.1 as f64 / ppq;
        if next / 1e6 > want { break }
        us = next; prev = (tk, tp);
    }
    println!("{}", prev.0 + ((want * 1e6 - us) * ppq / prev.1 as f64) as u64);
}
