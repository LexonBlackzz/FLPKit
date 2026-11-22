//! FL 12.3 -> FL 8.0 (ported from the Python prototype; rules and evidence in FORMAT.md "Downgrade FL 12.3 → FL 8.0").
//! Rules from newstuff / SolidInc saved by FL 8.0 and FL 12.3 and the Scott Fisher - Moholithic Filter demo (FL 8.0
//! save in FL 8's Data, FL 9.1 save in FL 12's).
use crate::fl11::{i32_at, records, steps, text, trim0, u16_at, u32_at, Ev};
use std::path::{Path, PathBuf};

/// UTF-16 in FL 12, 8-bit in FL 8
const TEXT: [u8; 15] = [193, 194, 195, 196, 197, 200, 201, 203, 204, 206, 207, 230, 231, 239, 241];
/// FL 11/12 only
const DROP: [u8; 10] = [28, 29, 31, 32, 97, 98, 202, 237, 238, 239];
/// FL 8 hosted these as VSTs (Plugins\VST\*.dll)
const LEGACY: [&str; 16] = [
    "Fruity Balance", "Fruity Blood Overdrive", "Fruity Chorus", "Fruity Compressor", "Fruity Delay", "Fruity Fast LP",
    "Fruity Filter", "Fruity Flanger", "Fruity Free Filter", "Fruity Mute 2", "Fruity Phaser", "Fruity Reeverb",
    "Fruity Bass Boost", "Fruity 7 Band EQ", "Fruity Center", "Fruity Phase Inverter",
];
/// Fruity effects FL 8 hosts as VSTs (FL 8's own saves: 212 byte 8 = 0; 2 makes FL 8 report them missing)
const VST8: [&str; 16] = [
    "Fruity 7 Band EQ", "Fruity Balance", "Fruity Bass Boost", "Fruity Blood Overdrive", "Fruity Center", "Fruity Chorus",
    "Fruity Compressor", "Fruity Delay", "Fruity Fast LP", "Fruity Filter", "Fruity Flanger", "Fruity Free Filter",
    "Fruity Mute 2", "Fruity Phase Inverter", "Fruity Phaser", "Fruity Reeverb",
];
/// FL 8 vs FL 12.3 state versions differ (SolidInc / newstuff pairs), not converted yet
const NEWER: [&str; 6] = ["Fruity Love Philter", "Slicex", "Maximus", "Fruity Parametric EQ 2", "MIDI Out", "Fruity Limiter"];

/// Install folders: FLPKIT_PF (default C:\Program Files (x86)), FLPKIT_IL (default its Image-Line folder)
pub(crate) fn image_line() -> PathBuf {
    let pf = std::env::var("FLPKIT_PF").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    std::env::var("FLPKIT_IL").map(PathBuf::from).unwrap_or_else(|_| Path::new(&pf).join("Image-Line"))
}

/// The newest FL 2025 / 2026 install (FL 12+ sample paths are relative to its Data)
fn newest_fl() -> PathBuf {
    let il = image_line();
    std::fs::read_dir(&il)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("FL Studio 202"))
        .max()
        .map_or_else(|| il.join("FL Studio 2026.1.1"), |n| il.join(n))
}

/// FL 12 mixer track -> FL 8 (None: no FL 8 track). FL 9+ keeps FL 8's inserts 1..64, moves its 4 send tracks to
/// 100..103 and the current track to 104; 65..99 are new
fn tmap(t: i64) -> Option<u16> {
    match t {
        0..=64 => Some(t as u16),
        100..=104 => Some((t - 35) as u16),
        _ => None,
    }
}

/// Automation / link address (param, target) FL 12 -> FL 8, or None. Mixer targets 0x2000 + track << 6 + slot: track
/// remapped, route-level params 64+j remapped; plugin params flagged 0x8000 in FL 12 written bare on mixer effects,
/// 0x2000 + param on channel generators (Sytrus in SolidInc). ponytail: the generator rule is from 2 Sytrus links
fn addr(mut p: u16, mut tgt: u16) -> Option<(u16, u16)> {
    if (0x2000..0x4000).contains(&tgt) {
        let t = tmap((tgt >> 6) as i64 - 128)?;
        tgt = 0x2000 + (t << 6) + (tgt & 63);
        if p >> 8 == 31 && (64..=168).contains(&(p & 255)) {
            p = 0x1f00 + 64 + tmap((p & 255) as i64 - 64)?;
        } else if p & 0x8000 != 0 {
            p &= 0x7fff;
        }
    } else if tgt < 0x2000 && p & 0x8000 != 0 {
        p = 0x2000 | (p & 0x7fff);
    }
    Some((p, tgt))
}

/// 216 (12-byte records, address at 4) / 226, 227 (20-byte, address at 8)
fn links(d: &[u8], size: usize, at: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for r in d.chunks_exact(size) {
        if let Some((p, t)) = addr(u16_at(r, at), u16_at(r, at + 2)) {
            let mut r = r.to_vec();
            r[at..at + 2].copy_from_slice(&p.to_le_bytes());
            r[at + 2..at + 4].copy_from_slice(&t.to_le_bytes());
            out.extend(r);
        }
    }
    out
}

/// 224: 24-byte notes -> 20: key u16 -> u8, the u16 group and the u8 after fine go
fn notes(d: &[u8]) -> Vec<u8> {
    d.chunks_exact(24).flat_map(|r| [&r[..13], &r[16..17], &r[18..24]].concat()).collect()
}

/// 233: 32-byte items -> 28: track u16 at 12 (199 - track) -> u8 (99 - track), the u8 at 16 moves to 13; untrimmed
/// start/end i32 -1 -> f32 -1. Clips on tracks past 99 dropped.
fn playlist(d: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in d.chunks_exact(32) {
        let Some(t) = u16_at(r, 12).checked_sub(100) else { continue };
        out.extend(&r[..12]);
        out.extend([t as u8, r[16], 0, 0]);
        out.extend(&r[20..24]);
        for o in [24, 28] {
            out.extend(if r[o..o + 4] == [0xff; 4] { &[0, 0, 0x80, 0xbf][..] } else { &r[o..o + 4] });
        }
    }
    out
}

/// 234 automation clip: [21-byte head, version at 13][n x 24-byte points (f64 x, f64 y, f32 tension, u32 mode)][tail,
/// version at 40]: versions 3 -> 2, the mode's top byte cleared, FL 12 appends 4 bytes
fn autoclip(mut d: Vec<u8>) -> Vec<u8> {
    if d.len() < 21 || d[13] != 3 {
        return d;
    }
    d[13] = 2;
    let n = u32_at(&d, 17) as usize;
    for k in 0..n {
        d[21 + 24 * k + 23] = 0;
    }
    let e = 21 + 24 * n;
    if d.len() > e + 40 && d[e + 40] == 3 {
        d[e + 40] = 2;
    }
    d.truncate(d.len() - 4);
    d
}

/// 225 [u32 0][u8 param][u8 31][u16 0x2000 + track << 6 + slot][i32]: per FL 8 track: slot params 0/1 (slots 0..7),
/// track params 192.., then route levels 64..133 (to tracks 0..69: 12800 to tracks, 0 to the sends unless FL 12 has
/// a level; FL 12 writes only the levels it uses)
fn mixer225(d: &[u8]) -> Vec<u8> {
    let (mut head, mut per): (Vec<u8>, std::collections::BTreeMap<u16, Vec<Vec<u8>>>) = (Vec::new(), Default::default());
    for r in d.chunks_exact(12) {
        let mut r = r.to_vec();
        if r[5] != 31 {
            head.extend(r);
            continue;
        }
        let a = u16_at(&r, 6);
        let (Some(t), slot) = (tmap((a >> 6) as i64 - 128), a & 63) else { continue };
        if slot >= 8 {
            continue;
        }
        r[6..8].copy_from_slice(&(0x2000 + (t << 6) + slot).to_le_bytes());
        per.entry(t).or_default().push(r);
    }
    let mut out = head;
    for (t, rs) in per {
        let mut routes = std::collections::HashMap::new();
        for r in &rs {
            if (64..=168).contains(&r[4]) {
                if let Some(j) = tmap(r[4] as i64 - 64) {
                    routes.insert(64 + j, r[8..12].to_vec());
                }
            }
        }
        rs.iter().filter(|r| r[4] < 2).for_each(|r| out.extend(r));
        rs.iter().filter(|r| r[4] > 1 && !(64..=168).contains(&r[4])).for_each(|r| out.extend(r));
        let a = (0x2000 + (t << 6)).to_le_bytes();
        for q in 64u16..134 {
            out.extend(0u32.to_le_bytes());
            out.extend([q as u8, 31]);
            out.extend(a);
            match routes.get(&q) {
                Some(v) => out.extend(v),
                None => out.extend((if q < 129 { 12800i32 } else { 0 }).to_le_bytes()),
            }
        }
    }
    out
}

/// FL 12 channel: 64 21 201 212 203 155 128 [213] ... 143 20 [196]; FL 8: 64 21 [201 212 213] ... 143 20 [196] 192
/// 128 (name and color at the end; samplers have no empty 201/212)
fn channels(ev: Ev) -> Ev {
    let (mut out, mut k) = (Vec::with_capacity(ev.len()), 0);
    while k < ev.len() {
        if ev[k].0 != 64 {
            out.push(ev[k].clone());
            k += 1;
            continue;
        }
        let j = (k + 1..ev.len()).find(|&j| [64, 65, 233, 149, 226, 227].contains(&ev[j].0)).unwrap_or(ev.len());
        let (mut blk, mut name, mut color): (Ev, Option<Vec<u8>>, Option<Vec<u8>>) = (Vec::new(), None, None);
        for (i, d) in &ev[k..j] {
            if *i == 203 {
                name = Some(d.clone());
                continue;
            }
            if *i == 128 && blk.last().is_some_and(|e| e.0 == 155) {
                color = Some(d.clone());
                blk.pop();
                continue;
            }
            if *i == 201 && d.iter().all(|&b| b == 0) {
                continue;
            }
            if *i == 212 && blk.last().is_some_and(|e| e.0 == 21) {
                continue; // the empty 201's window
            }
            blk.push((*i, d.clone()));
        }
        if let Some(n) = name {
            blk.push((192, n));
        }
        if let Some(c) = color {
            blk.push((128, c));
        }
        out.extend(blk);
        k = j;
    }
    out
}

/// FL 12 track (105): [149] [95] [204] 236(8) [98 slot]* [201 212 155 128 213]* 235(105) 154 147; FL 8 (70): 149 [95]
/// 204 27 236(5) [201 212 213]* 235(70) 154 147. FL 12 colors = FL 8's + 0x24 per channel. FL 12 has 10 slots, FL 8
/// 8: slots 8/9 move to the track's free slots below 8 (dropped if none). ponytail: automation of a moved slot keeps
/// its old address
fn mixer(ev: Ev, warnings: &mut Vec<String>) -> Ev {
    let Some(first) = ev.iter().position(|e| e.0 == 149 || e.0 == 236) else { return ev };
    let (mut k, mut blocks) = (first, Vec::new());
    while k < ev.len() && [149, 95, 204, 236].contains(&ev[k].0) {
        let Some(j) = (k..ev.len()).find(|&j| ev[j].0 == 147) else { break };
        blocks.push(ev[k..=j].to_vec());
        k = j + 1;
    }
    let mut out: Ev = ev[..first].to_vec();
    let mut order: Vec<(u16, &Ev)> = blocks.iter().enumerate().filter_map(|(t, b)| Some((tmap(t as i64)?, b))).collect();
    order.sort_by_key(|o| o.0);
    let mut skip = false;
    for (t, blk) in order {
        let get = |n: u8| blk.iter().find(|e| e.0 == n).map(|e| e.1.clone());
        let name = get(204).unwrap_or_else(|| vec![0]);
        let flags = get(236).and_then(|d| d.get(4).copied()).unwrap_or(0x2c);
        let color = match get(149) {
            Some(c) if !c.is_empty() => [c[..3.min(c.len())].iter().map(|&x| x.saturating_sub(0x24)).collect::<Vec<_>>(), c[3.min(c.len())..].to_vec()].concat(),
            _ => vec![0x54, 0x5d, 0x62, 0],
        };
        let mut head: Ev = vec![(149, color)];
        head.extend(blk.iter().take(3).filter(|e| e.0 == 95).cloned());
        let used: Vec<u32> = blk.iter().filter(|e| e.0 == 212).map(|e| u32_at(&e.1, 4)).collect();
        let mut free = (0..8).filter(|s| !used.contains(s));
        let mut body: Ev = Vec::new();
        for (i, d) in blk {
            let (i, mut d) = (*i, d.clone());
            if [149, 95, 204, 236, 98, 155].contains(&i) || (i == 128 && body.last().is_some_and(|e| e.0 == 212)) {
                continue;
            }
            if i == 212 {
                let s = u32_at(&d, 4);
                let s = if s >= 8 { free.next() } else { Some(s) };
                let Some(s) = s else {
                    warnings.push(format!("FL 8 mixer track {t} has no free slot: effect dropped"));
                    while body.last().is_some_and(|e| e.0 == 201) {
                        body.pop();
                    }
                    skip = true;
                    continue;
                };
                d[0..4].copy_from_slice(&(t as u32).to_le_bytes());
                d[4..8].copy_from_slice(&s.to_le_bytes());
                skip = false;
            }
            if skip && (i == 203 || i == 213) {
                if i == 213 {
                    skip = false;
                }
                continue;
            }
            if i == 235 {
                // FL 8: inserts 1..64 always route to the 4 send tracks; the current track (69) nowhere
                let mut r = vec![0u8; 70];
                for (j, &v) in d.iter().enumerate() {
                    if let (true, Some(m)) = (v != 0, tmap(j as i64)) {
                        r[m as usize] = v;
                    }
                }
                if (1..=64).contains(&t) {
                    r[65..69].fill(1);
                }
                if t == 69 {
                    r[0] = 0;
                }
                d = r;
            }
            if i == 147 {
                let o = i32_at(&d, 0);
                d = (if o < 0 { -1 } else { tmap(o as i64).map_or(-1, |m| m as i32) }).to_le_bytes().to_vec();
            }
            body.push((i, d));
        }
        out.extend(head);
        out.extend([(204, name), (27, vec![flags & 0x1f]), (236, vec![0, 0, 0, 0, 1])]);
        out.extend(body);
    }
    out.extend(ev[k..].iter().cloned());
    out
}

/// FL 12 Fruity Wrapper (kind 8) hosting one of FL 8's legacy effect VSTs -> (name, FL 8 state): FL 8 saves those as
/// the plugin name with the VST's f32 parameters (SolidInc: equal to FL 12's chunk floats)
fn unwrap(d: &[u8]) -> Option<(String, Vec<u8>)> {
    if d.len() < 4 || d[..4] != [8, 0, 0, 0] {
        return None;
    }
    let r = records(d);
    let get = |id: u32| r.iter().find(|e| e.0 == id).map(|e| e.1.clone()).unwrap_or_default();
    let (name, c) = (trim0(&get(54)).iter().map(|&b| b as char).collect::<String>(), get(53));
    if !LEGACY.contains(&name.as_str()) || c.first() != Some(&0xf7) || c.len() < 25 {
        return None;
    }
    let n = u32_at(&c, 21) as usize;
    Some((name, c[25..(25 + 4 * n).min(c.len())].to_vec()))
}

/// FL 12 / 2026 native states FL 8 reads differently; others as is (same versions: 3x Osc, Parametric EQ, Stereo
/// Enhancer, Delay 2, Flangus, ...)
fn state(name: &str, d: &[u8], warnings: &mut Vec<String>) -> Vec<u8> {
    use miniz_oxide::{deflate::compress_to_vec_zlib, inflate::decompress_to_vec_zlib};
    match name {
        "Sytrus" if d.len() > 4 && d[..4] == [3, 0, 0x17, 0] => {
            let v18 = decompress_to_vec_zlib(&d[4..]).ok().and_then(|p| crate::sytrus::v23_to_v18(&p));
            match v18 {
                Some(p) => [&18u32.to_le_bytes()[..], &compress_to_vec_zlib(&p, 6)].concat(),
                None => {
                    warnings.push("Sytrus v23 state didn't convert; kept as is".into());
                    d.to_vec()
                }
            }
        }
        // v16 = v15's header, path, slices and 13-byte trailer + per-slice articulator blocks; v15 ends with u32 0.
        // ponytail: only slicers with no slices (FL 2026's Moholithic, its sample gone)
        "Fruity slicer" if d.len() > 33 && d[..4] == [16, 0, 0, 0] => {
            let o = 33 + d[32] as usize;
            if d.len() < o + 17 || u32_at(d, o) != 0 {
                warnings.push("Fruity slicer v16 slices not decoded; state kept".into());
                return d.to_vec();
            }
            [&15u32.to_le_bytes()[..], &d[4..o + 17], &[0; 4]].concat()
        }
        // FL 20+ prefixes FL 8's 7 floats with a u32 version
        "Fruity 7 Band EQ" if d.len() == 32 && d[..4] == [1, 0, 0, 0] => d[4..].to_vec(),
        n if NEWER.contains(&n) => {
            warnings.push(format!("{n} state kept as FL 12 wrote it (FL 8 has an older version)"));
            d.to_vec()
        }
        _ => d.to_vec(),
    }
}

/// 201 name / 212 window / 213 state per plugin: legacy-effect wrappers unwrapped, states converted; 212 byte 8 = 0
/// for effects FL 8 hosts as VSTs
fn plugins(ev: Ev, warnings: &mut Vec<String>) -> Ev {
    let mut out: Ev = Vec::with_capacity(ev.len());
    for (i, mut d) in ev {
        let lo = out.len().saturating_sub(3);
        if let (213, Some(k)) = (i, (lo..out.len()).rev().find(|&n| out[n].0 == 201)) {
            let mut name: String = trim0(&out[k].1).iter().map(|&b| b as char).collect();
            let u = if name == "Fruity Wrapper" { unwrap(&d) } else { None };
            let vst = match u {
                Some((n, s)) => {
                    out[k].1 = [n.as_bytes(), &[0]].concat();
                    (name, d) = (n, s);
                    true
                }
                None => {
                    d = state(&name, &d, warnings);
                    VST8.contains(&name.as_str())
                }
            };
            if vst {
                if let Some(w) = out[k..].iter_mut().find(|e| e.0 == 212) {
                    if w.1.len() > 8 {
                        w.1[8] = 0;
                    }
                }
            }
        }
        out.push((i, d));
    }
    out
}

/// `src` as a PCM .wav FL 8 can load: .ogg and Vorbis-in-.wav (format 0x6771, data = Ogg stream) decoded by ffmpeg
fn pcm_wav(src: &Path, dst: &Path) -> Result<(), String> {
    if dst.exists() {
        return Ok(());
    }
    let b = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let fmt = b.windows(4).position(|w| w == b"fmt ").map(|o| u16_at(&b, o + 8));
    if b.starts_with(b"RIFF") && fmt == Some(1) {
        return std::fs::copy(src, dst).map(|_| ()).map_err(|e| e.to_string());
    }
    let mut input = src.to_path_buf();
    if b.starts_with(b"RIFF") {
        let o = b.windows(4).position(|w| w == b"data").ok_or("wav without data")?;
        let n = u32_at(&b, o + 4) as usize;
        input = dst.with_extension("wav.ogg");
        std::fs::write(&input, &b[o + 8..(o + 8 + n).min(b.len())]).map_err(|e| e.to_string())?;
    }
    let ok = std::process::Command::new("ffmpeg").args(["-v", "fatal", "-y", "-i"]).arg(&input).arg(dst).status();
    if input != src {
        let _ = std::fs::remove_file(&input);
    }
    match ok {
        Ok(s) if s.success() => Ok(()),
        _ => Err(format!("ffmpeg couldn't decode {} (is ffmpeg on PATH?)", src.display())),
    }
}

struct Samples {
    fl8: PathBuf,
    new: PathBuf,
    /// FL 8 Data file name (lowercase) -> \Data-relative path, first found walking top-down
    index: Option<std::collections::HashMap<String, String>>,
    outdir: PathBuf,
}

impl Samples {
    fn walk(dir: &Path, root: &Path, idx: &mut std::collections::HashMap<String, String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let entries: Vec<_> = rd.flatten().collect(); // directory order, as Python's os.walk
        let (dirs, files): (Vec<_>, Vec<_>) = entries.into_iter().partition(|e| e.path().is_dir());
        for f in files {
            let rel = f.path().strip_prefix(root).unwrap().to_string_lossy().replace('/', "\\");
            idx.entry(f.file_name().to_string_lossy().to_lowercase()).or_insert(format!("\\{rel}"));
        }
        for d in dirs {
            Self::walk(&d.path(), root, idx);
        }
    }

    /// 196 sample path %FLStudioData%\X (FL 12+): FL 8 Data-relative \Y when FL 8's Data has a file of that name (FL 12+
    /// moved packs, e.g. Packs\Legacy\Drums\Dance -> FL 8's Packs\Dance), else copied from the newest FL's Data into
    /// outdir and pointed to by absolute path
    fn path(&mut self, d: &[u8], warnings: &mut Vec<String>) -> Vec<u8> {
        let p: String = trim0(d).iter().map(|&b| b as char).collect();
        let Some(rel) = p.strip_prefix("%FLStudioData%").filter(|r| r.starts_with('\\')) else { return d.to_vec() };
        let z = |s: &str| [s.chars().map(|c| c as u8).collect::<Vec<_>>(), vec![0]].concat();
        if self.fl8.join(&rel[1..]).exists() {
            return z(rel);
        }
        let name = rel.rsplit('\\').next().unwrap();
        let (fl8, idx) = (&self.fl8, &mut self.index);
        let idx = idx.get_or_insert_with(|| {
            let mut m = Default::default();
            Self::walk(fl8, fl8, &mut m);
            m
        });
        if let Some(r) = idx.get(&name.to_lowercase()) {
            return z(r);
        }
        let src = self.new.join(&rel[1..]);
        if !src.exists() {
            return d.to_vec();
        }
        let _ = std::fs::create_dir_all(&self.outdir);
        let dst = self.outdir.join(Path::new(name).with_extension("wav"));
        if let Err(e) = pcm_wav(&src, &dst) {
            warnings.push(e);
            return d.to_vec();
        }
        z(&std::path::absolute(&dst).unwrap_or(dst).to_string_lossy())
    }
}

/// Converts an FL 12.3 project (an FL 12.3 save, or `downgrade` output for 12.3) to FL 8.0. Samples FL 8 lacks are
/// copied into `outdir`.
pub fn from_12_3(b: &[u8], outdir: &Path) -> Result<(Vec<u8>, Vec<String>), String> {
    let (h, events) = crate::parse(b)?;
    let mut warnings = Vec::new();
    let il = image_line();
    let mut samples = Samples { fl8: il.join("FL Studio 8").join("Data"), new: newest_fl().join("Data"), index: None, outdir: outdir.into() };
    let mut out: Ev = Vec::with_capacity(events.len());
    let mut has216 = false;
    for e in &events {
        let (mut i, mut d) = (e.id, e.data.to_vec());
        if i == 199 {
            d = b"8.0.0\0".to_vec();
        } else if i == 156 {
            // tempo: BPM x 1000 -> 93 (fine) + 66 (BPM)
            let bpm = u32_at(&d, 0);
            out.push((93, ((bpm % 1000) as u16).to_le_bytes().to_vec()));
            out.push((66, ((bpm / 1000) as u16).to_le_bytes().to_vec()));
            continue;
        }
        if TEXT.contains(&i) {
            d = text(&d);
        }
        match i {
            196 => d = samples.path(&d, &mut warnings),
            195 => i = 198,
            224 => d = notes(&d),
            215 => d.truncate(112), // FL 12 appends 13 bytes
            233 => d = playlist(&d),
            234 => d = autoclip(d),
            225 => d = mixer225(&d),
            216 => d = links(&d, 12, 4),
            226 | 227 => d = links(&d, 20, 8),
            22 if d.len() == 1 => d = vec![tmap(d[0] as i64).unwrap_or(0) as u8], // channel's mixer track
            150 => {
                // pattern color: FL 12's default 0x565148 is FL 8's 0xc8c1b8; FL 8 adds 151 (0)
                if d == b"HQV\0" {
                    d = vec![0xb8, 0xc1, 0xc8, 0];
                }
                out.push((150, d));
                out.push((151, vec![0; 4]));
                continue;
            }
            _ => {}
        }
        has216 |= i == 216 && !DROP.contains(&i);
        if i == 65 && !has216 {
            out.push((216, Vec::new()));
            has216 = true;
        }
        if DROP.contains(&i) {
            continue;
        }
        out.push((i, d));
    }
    let out = mixer(channels(out), &mut warnings);
    let mut out = steps(plugins(out, &mut warnings), h.ppq as u32, 20, None);
    // window size 0 = the plugin's default; FL 8 never saves 0 but reads it, and FL 11+ then uses its own default
    for (i, d) in out.iter_mut() {
        if *i == 212 && d.len() >= 52 {
            d[44..52].fill(0);
        }
    }
    if let Some(k) = out.iter().position(|e| e.0 == 233) {
        if k == 0 || out[k - 1].0 != 152 {
            out.insert(k, (152, vec![0; 4])); // FL 8 writes 152 (0) before the playlist
        }
    }
    Ok((crate::write(h, &out), warnings))
}
