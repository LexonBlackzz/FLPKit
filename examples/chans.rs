// chans <file.flp>: every rack channel: index, plugin, MIDI channel, exported?, notes in any pattern, name
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = flparser::Project::parse(&flparser::read_input(&a[1]).unwrap()).unwrap();
    let used = p.used_channels();
    for (i, c) in p.channels.iter().enumerate() {
        let n: usize = p.patterns.values().map(|pt| pt.notes.iter().filter(|x| x.chan as usize == i).count()).sum();
        println!("{i:>3} {:<14} ch{:<4} {} notes {n:>8}  {}", c.plugin, c.midi_ch.map_or("-".into(), |m| m.to_string()), if used.contains(&(i as u16)) { "OUT " } else { "skip" }, c.name);
    }
}
