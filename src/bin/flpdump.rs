//! flpdump <file.flp | -> [--full]   ("-": stdin, e.g. `xz -dc x.flp.xz | flpdump -`)
//! Default: header, text events, and per-ID histogram. --full: every event.
use std::collections::BTreeMap;

fn hex(d: &[u8], max: usize) -> String {
    let s: Vec<String> = d.iter().take(max).map(|b| format!("{b:02x}")).collect();
    let more = if d.len() > max { " .." } else { "" };
    format!("{}{more}", s.join(" "))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: flpdump <file.flp> [--full]");
    let full = args.iter().any(|a| a == "--full");
    // --id N [--w W]: print every N event as rows of W bytes
    let opt = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)?.parse::<usize>().ok());
    if let Some(id) = opt("--id") {
        let w = opt("--w").unwrap_or(24);
        let bytes = flparser::read_input(path).expect("read failed");
        let (_, events) = flparser::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
        for e in events.iter().filter(|e| e.id as usize == id) {
            println!("{:#08x} len={}", e.offset, e.data.len());
            for row in e.data.chunks(w) {
                println!("  {}", hex(row, w));
            }
        }
        return;
    }
    let bytes = flparser::read_input(path).expect("read failed");
    let (h, events) = flparser::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));

    println!("header: format={} channels={} ppq={} events={}", h.format, h.channels, h.ppq, events.len());

    let mut hist: BTreeMap<u8, (usize, usize)> = BTreeMap::new();
    for e in &events {
        let h = hist.entry(e.id).or_default();
        h.0 += 1;
        h.1 += e.data.len();
        if full {
            let shown = if e.id < 192 { format!("= {}", e.value()) } else { hex(e.data, 32) };
            println!("{:#08x} id={:3} len={:6} {shown}", e.offset, e.id, e.data.len());
        } else if e.id >= 192 && e.data.len() < 256 {
            let t = flparser::text(e.data);
            if t.len() > 1 && t.chars().all(|c| !c.is_control()) {
                println!("text id={:3}: {t:?}", e.id);
            }
        }
    }
    println!("id    count   bytes");
    for (id, (n, sz)) in hist {
        println!("{id:3} {n:7} {sz:7}");
    }
}
