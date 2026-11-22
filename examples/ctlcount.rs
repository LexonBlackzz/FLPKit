// ctlcount <a.mid> <b.mid>: controller / pitch-bend events per track, a/b, where they differ
fn counts(path: &str) -> Vec<usize> {
    let d = std::fs::read(path).unwrap();
    let (mut i, mut out) = (14, vec![]);
    while i + 8 <= d.len() {
        let l = u32::from_be_bytes(d[i + 4..i + 8].try_into().unwrap()) as usize;
        let t = &d[i + 8..i + 8 + l];
        let (mut j, mut rs, mut n) = (0, 0u8, 0);
        let vlq = |j: &mut usize| { let mut v = 0u64; loop { let c = t[*j]; *j += 1; v = (v << 7) | (c & 127) as u64; if c < 128 { return v } } };
        while j < t.len() {
            vlq(&mut j);
            if t[j] == 0xff { j += 2; let l = vlq(&mut j) as usize; j += l; continue }
            if t[j] & 0x80 != 0 { rs = t[j]; j += 1 }
            if matches!(rs >> 4, 0xb | 0xe) { n += 1 }
            j += if matches!(rs >> 4, 0xc | 0xd) { 1 } else { 2 };
        }
        out.push(n);
        i += 8 + l;
    }
    out
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (x, y) = (counts(&a[1]), counts(&a[2]));
    for (t, (p, q)) in x.iter().zip(&y).enumerate().filter(|(_, (p, q))| p != q) { print!("t{t} {p}/{q}  ") }
    println!();
}
