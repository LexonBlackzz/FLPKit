// lfoscan <dir>: every 234 event whose header byte 4 has 0x80 (LFO on?), with its bytes after the points
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut stack = vec![std::path::PathBuf::from(&a[1])];
    while let Some(p) = stack.pop() {
        if p.is_dir() { if let Ok(r) = std::fs::read_dir(&p) { stack.extend(r.flatten().map(|e| e.path())) } continue }
        if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case("flp")) { continue }
        let Ok(b) = std::fs::read(&p) else { continue };
        let Ok((_, evs)) = flparser::parse(&b) else { continue };
        let mut chan = 0;
        for e in &evs {
            if e.id == 64 { chan = e.value() }
            if e.id == 234 && e.data.len() > 21 {
                let n = u32::from_le_bytes(e.data[17..21].try_into().unwrap()) as usize;
                let t = &e.data[(21 + 24 * n).min(e.data.len())..];
                let hdr: Vec<_> = e.data[..17].iter().map(|x| format!("{x:02x}")).collect();
                let tl: Vec<_> = t.iter().map(|x| format!("{x:02x}")).collect();
                println!("{} | ch {chan} | {} | {}", p.display(), hdr.join(" "), tl.join(" "));
            }
        }
    }
}
