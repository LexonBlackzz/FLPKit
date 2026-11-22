//! flpkit midi <in.flp> [-o out.mid] [--cut] [--zero-vel]
//!     --cut: FL's "Cut remainder" export option; --zero-vel: velocity-0 notes as velocity 1
//! flpkit downgrade <in.flp> --to <version> [--force] [-o out.flp]
//!     version: 20.9, 20.6, 12.3, 11.1, 8.0 or a full 20.x / 12.3 version string (e.g. 20.6.2.1549)
const USAGE: &str = "usage: flpkit midi <in.flp> [-o out.mid] [--cut] [--zero-vel]
       flpkit downgrade <in.flp> --to <20.9|20.6|12.3|11.1|8.0|full version> [--force] [-o out.flp]";

/// Short target names -> the version string written into the project (builds the rules were checked against)
const TARGETS: &[(&str, &str)] = &[("20.9", "20.9.1.2826"), ("20.6", "20.6.2.1549"), ("12.3", "12.3.0.72")];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(cmd), Some(input)) = (args.get(1), args.get(2)) else { usage() };
    let opt = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let flag = |k: &str| args.iter().any(|a| a == k);
    let p = std::path::Path::new(input);
    let bytes = flparser::read_input(input).unwrap_or_else(|e| fail(&format!("{input}: {e}")));

    let (data, warnings, what, out) = match cmd.as_str() {
        "midi" => {
            let mut proj = flparser::Project::parse(&bytes).unwrap_or_else(|e| fail(&format!("{input}: {e}")));
            proj.cut_remainder = flag("--cut");
            proj.zero_vel_as_one = flag("--zero-vel");
            let out = opt("-o").unwrap_or_else(|| p.with_extension("mid").to_string_lossy().into());
            (proj.to_smf(), proj.warnings, format!("MIDI (from FL {})", proj.version), out)
        }
        "downgrade" => {
            let Some(to) = opt("--to") else { usage() };
            // FL 11 and 8 run the 12.3 step, then their own
            let old = match to.as_str() {
                "11.1" | "11" => Some("11.1.1"),
                "8.0" | "8" => Some("8.0.0"),
                _ => None,
            };
            let v = if old.is_some() { "12.3.0.72" } else { TARGETS.iter().find(|t| t.0 == to).map_or(to.as_str(), |t| t.1) };
            let stem = p.file_stem().unwrap().to_string_lossy();
            let out = opt("-o").unwrap_or_else(|| p.with_file_name(format!("{stem} (FL{to}).flp")).to_string_lossy().into());
            // an FL 12.3 save goes straight to the FL 11 / 8 step
            let src = flparser::parse(&bytes).ok().and_then(|(_, ev)| ev.iter().find(|e| e.id == 199).map(|e| flparser::version(&flparser::text(e.data))));
            let (mut d, mut w) = if old.is_some() && src.is_some_and(|s| s[..2] == [12, 3]) {
                (bytes.clone(), Vec::new())
            } else {
                flparser::downgrade::downgrade(&bytes, v, flag("--force")).unwrap_or_else(|e| fail(&format!("{input}: {e}")))
            };
            let step = match old {
                Some("11.1.1") => Some(flparser::fl11::from_12_3(&d)),
                // samples FL 8 lacks go next to the output
                Some(_) => Some(flparser::fl8::from_12_3(&d, std::path::Path::new(&format!("{} samples", out.trim_end_matches(".flp"))))),
                None => None,
            };
            if let Some(r) = step {
                let (d2, w2) = r.unwrap_or_else(|e| fail(&format!("{input}: {e}")));
                (d, w) = (d2, [w, w2].concat());
            }
            (d, w, format!("FL {} project", old.unwrap_or(v)), out)
        }
        _ => usage(),
    };
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    std::fs::write(&out, data).unwrap_or_else(|e| fail(&format!("{out}: {e}")));
    eprintln!("{what} -> {out}");
}

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2);
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}
