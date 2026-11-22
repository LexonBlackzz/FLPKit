// autopts <file.flp> <chan>...: raw automation points (event 234) of those rack channels
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = flparser::read_input(&a[1]).unwrap();
    let want: Vec<u32> = a[2..].iter().map(|s| s.parse().unwrap()).collect();
    let (_, evs) = flparser::parse(&b).unwrap();
    let mut chan = u32::MAX;
    for e in &evs {
        match e.id {
            64 => chan = e.value(),
            234 if want.contains(&chan) && e.data.len() >= 21 => {
                let d = e.data;
                let n = u32::from_le_bytes(d[17..21].try_into().unwrap()) as usize;
                println!("chan {chan}: header {:02x?}, {n} points", &d[..17]);
                for k in 0..n {
                    let o = 21 + 24 * k;
                    let f = |o: usize| f64::from_le_bytes(d[o..o + 8].try_into().unwrap());
                    let t = f32::from_le_bytes(d[o + 16..o + 20].try_into().unwrap());
                    println!("  dbeat {:>8.4} value {:.6} tension {t:.4} flags {:02x?}", f(o), f(o + 8), &d[o + 20..o + 24]);
                }
                println!("  tail {} bytes: {:02x?}", d.len() - 21 - 24 * n, &d[21 + 24 * n..]);
            }
            _ => {}
        }
    }
}
