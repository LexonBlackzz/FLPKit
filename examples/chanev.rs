// chanev <file.flp> <chan>: every event (id, len, hex head) between that channel's select and the next
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = flparser::read_input(&a[1]).unwrap();
    let want: u32 = a[2].parse().unwrap();
    let (_, evs) = flparser::parse(&b).unwrap();
    let mut chan = u32::MAX;
    for e in &evs {
        if e.id == 64 { chan = e.value() }
        if chan == want && e.id != 64 {
            let h: Vec<_> = e.data.iter().take(usize::MAX).map(|x| format!("{x:02x}")).collect();
            println!("{:>3} {:>5} {}", e.id, e.data.len(), h.join(" "));
        }
    }
}
