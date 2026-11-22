// lfos <file.flp>: automation channels' LFO settings
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = flparser::Project::parse(&flparser::read_input(&a[1]).unwrap()).unwrap();
    for (i, c) in p.channels.iter().enumerate().filter(|c| c.1.lfo.is_some()) {
        println!("{i:>3} on {} {:?}", c.lfo_on, c.lfo.unwrap());
    }
}
