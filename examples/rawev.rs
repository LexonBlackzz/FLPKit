// rawev <file.flp> <event id> [chan...]: full hex of that event per rack channel
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = flparser::read_input(&a[1]).unwrap();
    let id: u8 = a[2].parse().unwrap();
    let want: Vec<u32> = a[3..].iter().map(|s| s.parse().unwrap()).collect();
    let (_, evs) = flparser::parse(&b).unwrap();
    let mut chan = u32::MAX;
    for e in &evs {
        if e.id == 64 { chan = e.value() }
        if e.id == id && (want.is_empty() || want.contains(&chan)) {
            println!("chan {chan:>3}: {}", e.data.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "));
        }
    }
}
