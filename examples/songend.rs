// songend <file.flp>: song end tick, tail tick, project tempo
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = flparser::Project::parse(&flparser::read_input(&a[1]).unwrap()).unwrap();
    let end = p.clips.iter().map(|c| c.pos as u64 + c.len as u64).max().unwrap_or(0);
    let auto = p.auto_clips.iter().chain(&p.tempo_clips).map(|c| c.pos as u64 + c.len as u64).max().unwrap_or(0);
    println!("auto end {auto}");
    println!("song end {end} tempo {} ppq {} tempo clips {} legacy {}", p.tempo, p.ppq, p.tempo_clips.len(), p.tempo_legacy);
}
