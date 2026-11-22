// autoclips <file.flp> [target]: playlist automation clips (param, target, placement, points)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = flparser::Project::parse(&flparser::read_input(&a[1]).unwrap()).unwrap();
    let only: Option<u16> = a.get(2).map(|s| s.parse().unwrap());
    for c in p.auto_clips.iter().chain(&p.tempo_clips).filter(|c| only.is_none_or(|t| c.target == t)) {
        let pts: Vec<String> = c.pts.iter().take(if std::env::var("ALLPTS").is_ok() { 999 } else { 6 }).map(|(b, v, t, m)| format!("{b}:{v}~{t}{}", if *m != 0 { format!("/m{m}") } else { String::new() })).collect();
        if std::env::var("AT").is_ok() { let b: f64 = std::env::var("AT").unwrap().parse().unwrap(); println!("  value_at({b}) = {}", c.value_at(b)); }
        println!("src {:>3} param {:#06x} target {:#06x} pos {} len {} start {:.3} pts {} [{}]", c.src, c.param, c.target, c.pos, c.len, c.start, c.pts.len(), pts.join(" "));
    }
}
