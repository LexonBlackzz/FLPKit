//! flpkit midi <in.flp> [-o out.mid] [--cut] [--zero-vel]
//!     --cut: FL's "Cut remainder" export option; --zero-vel: velocity-0 notes as velocity 1
//! flpkit downgrade <in.flp> --to <version> [--force] [-o out.flp]
//!     version: 20.9, 20.6, 12.3 or a full version string (e.g. 20.6.2.1549)
const USAGE: &str = "usage: flpkit midi <in.flp> [-o out.mid] [--cut] [--zero-vel]
       flpkit downgrade <in.flp> --to <20.9|20.6|12.3|full version> [--force] [-o out.flp]";

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
            let v = TARGETS.iter().find(|t| t.0 == to).map_or(to.as_str(), |t| t.1);
            let (d, w) = flparser::downgrade::downgrade(&bytes, v, flag("--force")).unwrap_or_else(|e| fail(&format!("{input}: {e}")));
            let stem = p.file_stem().unwrap().to_string_lossy();
            let out = opt("-o").unwrap_or_else(|| p.with_file_name(format!("{stem} (FL{to}).flp")).to_string_lossy().into());
            (d, w, format!("FL {v} project"), out)
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
