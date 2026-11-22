// ev225 <file.flp> [chan]: event 225 as 12-byte records [u32 a][u16 param][u16 target][i32 value]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = flparser::read_input(&a[1]).unwrap();
    let want: Option<u16> = a.get(2).map(|s| s.parse().unwrap());
    let (_, evs) = flparser::parse(&b).unwrap();
    for e in evs.iter().filter(|e| e.id == 225) {
        for r in e.data.chunks_exact(12) {
            let (x, p, t, v) = (u32::from_le_bytes(r[0..4].try_into().unwrap()), u16::from_le_bytes([r[4], r[5]]), u16::from_le_bytes([r[6], r[7]]), i32::from_le_bytes(r[8..12].try_into().unwrap()));
            if want.is_none_or(|w| w == t) { println!("{x:>6} p {p:#06x} t {t:#06x} v {v}") }
        }
    }
}
