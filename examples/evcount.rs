// evcount <file.flp>: event ids with count and sizes; IDS=a,b dumps those (index, len, hex head)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = flparser::read_input(&a[1]).unwrap();
    let (_, evs) = flparser::parse(&b).unwrap();
    let mut m = std::collections::BTreeMap::new();
    for e in &evs { let x = m.entry(e.id).or_insert((0, usize::MAX, 0)); x.0 += 1; x.1 = x.1.min(e.data.len()); x.2 = x.2.max(e.data.len()); }
    let ids: Vec<u8> = std::env::var("IDS").map(|s| s.split(',').map(|x| x.parse().unwrap()).collect()).unwrap_or_default();
    if ids.is_empty() { for (id, (n, lo, hi)) in m { println!("{id:>3} x{n:<7} {lo}..{hi}") } return }
    for (i, e) in evs.iter().enumerate().filter(|e| ids.contains(&e.1.id)) {
        println!("{i:>7} {:>3} {:>5} {}", e.id, e.data.len(), e.data.iter().take(64).map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "));
    }
}
