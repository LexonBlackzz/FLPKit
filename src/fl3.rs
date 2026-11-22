//! FL 4.x -> FruityLoops 3.5.4 (ported from the Python prototype; rules and evidence in FORMAT.md "Downgrade FL 4.x →
//! FruityLoops 3.5.4"). Rules from the demos both ship (Saw, Clouds, LogDrumThing, BeepMap Ambi: 4.1.2 vs 3.5.4 saves;
//! Hoovalicious, SubwooferTest, NewStuff) and the user's fader / EQ measurements.
//! - header: 199 3.5.4, 200 one 0 byte; 17 = 16 then 24 = 16, 23 = 1, 217 (16 bytes) after 18
//! - 226 / 227 links: 16 bytes (FL 4 appends u32 128)
//! - mixer: 21 tracks (0 master, 1-16 inserts, 17-20 sends = FL 4's 65-68), 8 slots; effects all before the first
//!   channel; after the playlist: 84 0, 225 (send levels of inserts 1-16 only), per track 204 name + 147, then 133
//! - channels: 22 (insert) = FL 4's - 1; 215 first 80 bytes; 3x Osc state v12 -> v11 (first 88 bytes)
//! - patterns: no 150 / 151 / 152; playlist 233 -> one 129 per item: u16 bar, u16 pattern
//! - 219 byte 20: FL 4 6 = FL 3 7. ponytail: other values kept
use crate::fl11::{i32_at, put_u32, u16_at, u32_at, Ev};
use crate::fl6::{latin1, name, z};
use crate::fl8::program_files;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Plugin names as FL 3.5 spells them
const NAMES: [&str; 21] = [
    "3x Osc", "Fruit kick", "Fruity reeverb", "Fruity compressor", "Fruity delay", "Fruity delay 2", "Fruity parametric EQ",
    "Fruity soft clipper", "Fruity stereo enhancer", "Fruity free filter", "Fruity flanger", "Fruity 7 band EQ",
    "Fruity filter", "Fruity slicer", "Fruity phaser", "Fruity chorus", "Fruity blood overdrive", "Fruity balance",
    "Fruity fast dist", "Fruity peak controller", "Fruity send",
];

fn fl3_name(n: &[u8]) -> Option<&'static str> {
    let n = latin1(n).to_lowercase();
    NAMES.iter().find(|x| x.to_lowercase() == n).copied()
}

/// FL 4 mixer track -> FL 3.5 track, in FL 4 order (masters and inserts 0-16, then sends 65-68)
fn tracks() -> Vec<(u32, u32)> {
    (0..17).map(|t| (t, t)).chain((0..4).map(|k| (65 + k, 17 + k))).collect()
}

/// first match of `name` under `dir`: the folder itself, then subfolders top-down in directory order (Python's glob
/// `**`, hidden folders skipped)
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    if dir.join(name).exists() {
        return Some(dir.join(name));
    }
    let mut subs: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().filter(|e| e.path().is_dir()).collect();
    subs.retain(|e| !e.file_name().to_string_lossy().starts_with('.'));
    subs.iter().find_map(|e| find(&e.path(), name))
}

/// 196: Data-relative paths found in FL 3.5's Samples by name (FL 3.5 paths are Samples-relative), else copied
fn sample(d: &[u8], outdir: &Path, warnings: &mut Vec<String>) -> Vec<u8> {
    let p = latin1(&name(d));
    if !p.starts_with('\\') {
        return d.to_vec();
    }
    let file = p.rsplit('\\').next().unwrap();
    let samples = program_files().join(r"FruityLoops3.5\Samples");
    if let Some(hit) = find(&samples, file) {
        let rel = hit.strip_prefix(&samples).unwrap().to_string_lossy().replace('/', "\\");
        return z(&format!("\\{rel}"));
    }
    let src = program_files().join(r"FLStudio4\Data").join(&p[1..]);
    if !src.exists() {
        warnings.push(format!("missing sample {p}"));
        return d.to_vec();
    }
    let _ = std::fs::create_dir_all(outdir);
    let dst = outdir.join(file);
    if let Err(e) = std::fs::copy(&src, &dst) {
        warnings.push(format!("{}: {e}", src.display()));
        return d.to_vec();
    }
    z(&std::path::absolute(&dst).unwrap_or(dst).to_string_lossy())
}

/// FL 4's speech synth (.speech files, rendered on load) doesn't exist in FL 3.5: point the slicer at <stem>.wav in
/// the samples folder, where a render of the phrase (made in FL 4) is expected. State v6: 20-byte header, then the
/// path as [u8 len][text]
fn slicer(d: &[u8], outdir: &Path, warnings: &mut Vec<String>) -> Vec<u8> {
    let Some(&n) = d.get(20) else { return d.to_vec() };
    let path = latin1(&d[21..21 + n as usize]);
    if !path.to_lowercase().ends_with(".speech") {
        return d.to_vec();
    }
    let file = path.rsplit('\\').next().unwrap();
    let wav = std::path::absolute(outdir.join(format!("{}.wav", &file[..file.len() - 7]))).unwrap_or_default();
    let mut w = z(&wav.to_string_lossy());
    w.pop();
    warnings.push(format!("the slicer needs a WAV render of {path} at {}", wav.display()));
    [&d[..20], &[w.len() as u8], &w, &d[21 + n as usize..]].concat()
}

/// FL 4 mixer defaults of the params a Parametric EQ stands in for: volume 96, pan 97, separation 98, EQ gains
/// 112-114, freqs 120-122, widths 128-130
const EQ_DEFAULT: [(u8, i32); 12] =
    [(96, 128), (97, 0), (98, 0), (112, 0), (113, 0), (114, 0), (120, 5777), (121, 33145), (122, 55825), (128, 17500), (129, 17500), (130, 17500)];
/// Saw's Parametric EQ window info (u32 0 / 4 = track / slot)
const EQ_212: &str = "030000000200000002000000000000000000000010000000000000000000000000000000000000000000000000000000000000000000";

/// FL 4 inserts have a volume fader and a 3-band EQ, FL 3.5's don't: a Fruity parametric EQ in the track's next free
/// slot does both (same coding; FL 4's fader equals its global gain, sample-exact by the user's exports).
/// ponytail: pan / stereo separation not carried (warned)
fn fader_eq(trackp: &HashMap<(i32, u8), i32>, fx: &Ev, warnings: &mut Vec<String>) -> Ev {
    let mut out = Vec::new();
    let eq212: Vec<u8> = (0..EQ_212.len()).step_by(2).map(|k| u8::from_str_radix(&EQ_212[k..k + 2], 16).unwrap()).collect();
    for (t4, t3) in tracks() {
        let v: HashMap<u8, i32> = EQ_DEFAULT.iter().map(|&(p, dv)| (p, *trackp.get(&(t4 as i32, p)).unwrap_or(&dv))).collect();
        if EQ_DEFAULT.iter().all(|(p, dv)| v[p] == *dv) {
            continue;
        }
        if v[&97] != 0 || v[&98] != 0 {
            warnings.push(format!("FL 4 track {t4}: pan / separation not carried ({}, {})", v[&97], v[&98]));
        }
        let slot = fx.iter().filter(|e| e.0 == 212 && u32_at(&e.1, 0) == t3).map(|e| u32_at(&e.1, 4) as i64).max().unwrap_or(-1) + 1;
        if slot > 7 {
            warnings.push(format!("FL 3 track {t3} has no free slot for its fader / EQ"));
            continue;
        }
        let st = [
            v[&112], v[&113], v[&114], 0, 0, 0, 0, v[&120], v[&121], v[&122], 0, 0, 0, 0, v[&128], v[&129], v[&130], 0, 0, 0, 0, 5, 6, 7,
            0, 0, 0, 0, v[&96],
        ];
        out.push((201, b"Fruity parametric EQ\0".to_vec()));
        out.push((212, [&t3.to_le_bytes()[..], &(slot as u32).to_le_bytes(), &eq212[8..]].concat()));
        out.push((213, st.iter().flat_map(|x| x.to_le_bytes()).collect()));
    }
    out
}

/// FL 4.x events -> FruityLoops 3.5.4
pub(crate) fn from_4(ev: Ev, ppq: u32, outdir: &Path, warnings: &mut Vec<String>) -> Ev {
    let mix0 = ev.iter().position(|e| e.0 == 233).or_else(|| ev.iter().position(|e| e.0 == 149)).unwrap_or(ev.len() - 1);
    let (head, mixer) = ev.split_at(mix0 + 1);
    let track: HashMap<u32, u32> = tracks().into_iter().collect();
    // mixer: tracks end with 147; effects before 149
    let (mut fx, mut names, mut sends, mut trackp, mut trk): (Ev, HashMap<u32, Vec<u8>>, HashMap<(i32, u8), i32>, HashMap<(i32, u8), i32>, u32) =
        (Vec::new(), HashMap::new(), HashMap::new(), HashMap::new(), 0);
    let mut cur: Ev = Vec::new();
    for (i, d) in mixer {
        let t3 = track.get(&trk).copied();
        match (i, t3) {
            (201, _) => cur = vec![(201, z(fl3_name(&name(d)).unwrap_or(&latin1(&name(d)))))],
            (212 | 203, Some(_)) => cur.push((*i, d.clone())),
            (213, Some(t3)) => {
                if let Some(w) = cur.iter_mut().find(|e| e.0 == 212) {
                    put_u32(&mut w.1, 0, t3);
                    w.1[20] = 0x10;
                }
                fx.append(&mut cur);
                fx.push((213, d.clone()));
            }
            (213, None) => warnings.push(format!("dropped {} on FL 4 mixer track {trk}", latin1(&name(&cur[0].1)))),
            (204, Some(t3)) => {
                names.insert(t3, d.clone());
            }
            (147, _) => trk += 1,
            (225, _) => {
                for r in d.chunks_exact(12) {
                    let (p, ty, t, v) = (r[4], r[5], (u16_at(r, 6) >> 6) as i32 - 128, i32_at(r, 8));
                    if ty == 31 && (64..68).contains(&p) && (1..=16).contains(&t) {
                        sends.insert((t, p), v);
                    }
                    if ty == 31 && p >= 96 {
                        trackp.insert((t, p), v);
                    }
                }
            }
            _ => {}
        }
    }
    fx.extend(fader_eq(&trackp, &fx, warnings));
    let (mut out, mut plugin, mut fx): (Ev, Vec<u8>, Option<Ev>) = (Vec::new(), Vec::new(), Some(fx));
    for (i, d) in head {
        let (i, mut d) = (*i, d.clone());
        // FL 3.5 never writes these (26 after pattern names, 142 in a channel)
        if [150, 151, 152, 26, 142].contains(&i) {
            continue;
        }
        match i {
            199 => d = b"3.5.4\0".to_vec(),
            200 => d = vec![0],
            17 => {
                out.push((17, vec![16]));
                out.push((24, vec![16]));
                continue;
            }
            23 => d = vec![1],
            226 | 227 => d.truncate(16),
            64 => {
                plugin.clear();
                out.extend(fx.take().unwrap_or_default());
            }
            201 => {
                plugin = name(&d).to_ascii_lowercase();
                if let Some(n) = fl3_name(&plugin) {
                    d = z(n);
                }
            }
            213 if plugin == b"fruity slicer" => d = slicer(&d, outdir, warnings),
            213 if plugin == b"3x osc" && d.first() == Some(&12) => d = [&[11u8][..], &d[1..88.min(d.len())]].concat(),
            22 => d = vec![d[0].saturating_sub(1)],
            215 => d.truncate(80),
            219 if d.len() > 20 && d[20] == 6 => d[20] = 7,
            196 => d = sample(&d, outdir, warnings),
            233 => {
                let mut items: Vec<(u32, u16, u16, u32, u16)> =
                    d.chunks_exact(28).map(|r| (u32_at(r, 0), u16_at(r, 4), u16_at(r, 6), u32_at(r, 8), u16_at(r, 12))).collect();
                items.sort();
                // bar = 4 beats; FL 3 lists by bar
                for (pos, _, base, _, row) in items {
                    if base == 0x5000 {
                        out.push((129, [((pos / (4 * ppq)) as u16).to_le_bytes(), 0x78E7u16.wrapping_sub(row).to_le_bytes()].concat()));
                    }
                }
                continue;
            }
            _ => {}
        }
        out.push((i, d));
        if i == 18 {
            out.push((217, [[0u8; 8], [255; 8]].concat()));
        }
    }
    // selects left empty by 26
    let keep: Vec<bool> = (0..out.len()).map(|k| !(out[k].0 == 65 && out.get(k + 1).is_some_and(|e| e.0 == 64 || e.0 == 65))).collect();
    let mut out: Ev = out.into_iter().zip(keep).filter(|(_, k)| *k).map(|(e, _)| e).collect();
    out.push((84, vec![0; 2]));
    let mut m = Vec::new();
    for t in 1..17 {
        for p in 64..68u8 {
            m.extend(0u32.to_le_bytes());
            m.extend([p, 31]);
            m.extend((0x2000u16 + ((t as u16) << 6)).to_le_bytes());
            m.extend(sends.get(&(t, p)).unwrap_or(&0).to_le_bytes());
        }
    }
    out.push((225, m));
    for t in 0..21 {
        out.push((204, names.get(&t).cloned().unwrap_or(vec![0])));
        out.push((147, (if t == 0 { 0i32 } else { -1 }).to_le_bytes().to_vec()));
    }
    out.extend(mixer.iter().filter(|e| e.0 == 133).cloned());
    out
}

/// Converts an FL 4.x project to FruityLoops 3.5.4. Samples FL 3.5 lacks are copied into `outdir`.
pub fn from_4_x(b: &[u8], outdir: &Path) -> Result<(Vec<u8>, Vec<String>), String> {
    let (h, events) = crate::parse(b)?;
    let mut warnings = Vec::new();
    let ev = from_4(events.iter().map(|e| (e.id, e.data.to_vec())).collect(), h.ppq as u32, outdir, &mut warnings);
    Ok((crate::write(h, &ev), warnings))
}
