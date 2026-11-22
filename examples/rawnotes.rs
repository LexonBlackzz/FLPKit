// rawnotes <file.flp> [n]: the first n raw 24-byte notes of each pattern (pos len chan key, rest hex)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = flparser::read_input(&a[1]).unwrap();
    let n: usize = a.get(2).map_or(16, |s| s.parse().unwrap());
    let (_, evs) = flparser::parse(&b).unwrap();
    let mut pat = 0;
    for e in &evs {
        match e.id {
            65 => pat = e.value(),
            224 => {
                let slides = e.data.chunks_exact(24).filter(|r| r[4] & 8 != 0).count();
                println!("pattern {pat}: {} notes, {slides} slide", e.data.len() / 24);
                for r in e.data.chunks_exact(24).take(n) {
                    let u = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                    println!("  pos {:>6} len {:>5} key {:>3} | {}", u(0), u(8), r[12], r.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "));
                }
            }
            _ => {}
        }
    }
}
