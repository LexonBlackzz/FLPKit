//! FL 8.0 -> FL 7.0 / 6.0, and the entry point for the older targets (ported from the Python prototypes; rules and
//! evidence in FORMAT.md "Downgrade FL 8.0 → FL 6.0"). Rules from the FL 6 / 7 / 8 demo pairs and SolidInc's What I
//! Found (FL 8's NewStuff.flp), tested by ear in FL 6 and FL 7.
use crate::fl11::{i32_at, trim0, u16_at, u32_at, Ev};
use crate::fl8::{image_line, pcm_wav};
use miniz_oxide::inflate::decompress_to_vec_zlib;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(crate) fn f32_at(d: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
/// Python's round(): ties to even
pub(crate) fn round(x: f64) -> i64 {
    x.round_ties_even() as i64
}
pub(crate) fn name(d: &[u8]) -> Vec<u8> {
    trim0(d).to_vec()
}
pub(crate) fn latin1(d: &[u8]) -> String {
    d.iter().map(|&b| b as char).collect()
}
pub(crate) fn z(s: &str) -> Vec<u8> {
    s.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).chain([0]).collect()
}

/// First 213 (plugin state) of a preset .fst
pub(crate) fn preset(p: &Path) -> Result<Vec<u8>, String> {
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let (_, ev) = crate::parse(&b)?;
    ev.iter().find(|e| e.id == 213).map(|e| e.data.to_vec()).ok_or_else(|| format!("{}: no state", p.display()))
}

/// PCM .wav: (channels, sample width, rate, frames' bytes), walking RIFF chunks like Python's wave
pub(crate) struct Wav {
    pub ch: u16,
    pub width: u16,
    pub rate: u32,
    pub pcm: Vec<u8>,
}

pub(crate) fn read_wav(p: &Path) -> Result<Wav, String> {
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(format!("{}: not a wav", p.display()));
    }
    let (mut o, mut fmt) = (12, None);
    while o + 8 <= b.len() {
        let n = u32_at(&b, o + 4) as usize;
        let body = &b[o + 8..(o + 8 + n).min(b.len())];
        match &b[o..o + 4] {
            b"fmt " if body.len() >= 16 && u16_at(body, 0) == 1 => fmt = Some((u16_at(body, 2), u32_at(body, 4), u16_at(body, 14))),
            b"data" => {
                let (ch, rate, bits) = fmt.ok_or_else(|| format!("{}: not PCM", p.display()))?;
                let width = bits.div_ceil(8);
                let frame = (ch * width) as usize;
                return Ok(Wav { ch, width, rate, pcm: body[..body.len() / frame * frame].to_vec() });
            }
            _ => {}
        }
        o += 8 + n + (n & 1);
    }
    Err(format!("{}: no data chunk", p.display()))
}

pub(crate) fn write_wav(p: &Path, w: &Wav) -> Result<(), String> {
    let mut b = b"RIFF".to_vec();
    b.extend((36 + w.pcm.len() as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(w.ch.to_le_bytes());
    b.extend(w.rate.to_le_bytes());
    b.extend((w.rate * (w.ch * w.width) as u32).to_le_bytes());
    b.extend((w.ch * w.width).to_le_bytes());
    b.extend((w.width * 8).to_le_bytes());
    b.extend(b"data");
    b.extend((w.pcm.len() as u32).to_le_bytes());
    b.extend(&w.pcm);
    std::fs::write(p, b).map_err(|e| format!("{}: {e}", p.display()))
}

pub(crate) struct Ctx {
    /// the target FL's Data folder: samples there stay referenced, others get copied
    pub data: PathBuf,
    pub outdir: PathBuf,
    /// project BPM, for the slicer
    pub tempo: f64,
    pub warnings: Vec<String>,
}

impl Ctx {
    /// 196 sample path: kept if the target FL has it in its Data; otherwise copied from FL 8's Data into outdir (.ogg
    /// -> .wav, FL 6 has no ogg) and pointed to by absolute path
    fn sample(&mut self, d: &[u8]) -> Vec<u8> {
        let p = latin1(trim0(d));
        let Some(rel) = p.strip_prefix('\\') else { return d.to_vec() };
        if self.data.join(rel).exists() {
            return d.to_vec();
        }
        let src = image_line().join("FL Studio 8").join("Data").join(rel);
        if !src.exists() {
            return d.to_vec();
        }
        let _ = std::fs::create_dir_all(&self.outdir);
        let dst = self.outdir.join(format!("{}.wav", src.file_stem().unwrap().to_string_lossy()));
        if let Err(e) = pcm_wav(&src, &dst) {
            self.warnings.push(e);
            return d.to_vec();
        }
        z(&std::path::absolute(&dst).unwrap_or(dst).to_string_lossy())
    }
}

/// automation clip (234) v2 (FL 8): 24-byte points [f64 dbeat][f64 value][f32 tension][u32 mode] -> v1, all f32
fn automation(d: Vec<u8>) -> Vec<u8> {
    if d.len() < 21 || d[13] != 2 {
        return d;
    }
    let n = u32_at(&d, 17) as usize;
    let mut out = d[..21].to_vec();
    out[13] = 1;
    let f64_at = |b: &[u8], o: usize| f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    for k in 0..n {
        let o = 21 + 24 * k;
        out.extend((f64_at(&d, o) as f32).to_le_bytes());
        out.extend((f64_at(&d, o + 8) as f32).to_le_bytes());
        out.extend(&d[o + 16..o + 24]);
    }
    let r = &d[21 + 24 * n..];
    // tail: [u32 1][16 ff][6 u32, the 6th = version 2 -> 1][u32 m][m x [f64][f64][8 bytes] -> f32 f32 + 8]
    if r.len() >= 48 && u32_at(r, 40) == 2 {
        let mut t = r[..48].to_vec();
        t[40] = 1;
        let m = u32_at(r, 44) as usize;
        for k in 0..m {
            let o = 48 + 24 * k;
            t.extend((f64_at(r, o) as f32).to_le_bytes());
            t.extend((f64_at(r, o + 8) as f32).to_le_bytes());
            t.extend(&r[o + 16..o + 24]);
        }
        t.extend(&r[48 + 24 * m..]);
        out.extend(t);
    } else {
        out.extend(r);
    }
    out
}

/// Parametric EQ 2 (FL 7+) -> Parametric EQ: both store 7 bands as i32 gains, freqs, widths, types; PEQ 2 has a u32
/// version first. ponytail: widths scaled by the default ratio 17500/25412, freq scale assumed equal (by eye)
fn peq2(d: &[u8]) -> Vec<u8> {
    let v: Vec<i32> = (0..29).map(|k| i32_at(d, 4 + 4 * k)).collect();
    let mut out: Vec<i32> = v[..28].to_vec();
    for k in 0..7 {
        let w = round(out[14 + k] as f64 * 17500.0 / 25412.0).clamp(1, 65536) as i32;
        // high / low pass (types 3 / 1): scaled widths resonate in PEQ 1; 0.05 per the user
        out[14 + k] = if [1, 3].contains(&v[21 + k]) { round(0.05 * 65536.0) as i32 } else { w };
    }
    out.push(128);
    out.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Slicex (FL 8) -> Fruity slicer v13, decks A and B joined in one wav (slices = the wavs' cue points, deck B's after
/// deck A's, so key 60 + k stays slice k). Loop bpm = project tempo, beats = the length at that tempo.
/// ponytail: Slicex envelopes / pitch / articulators dropped
fn slicex(d: &[u8], ctx: &mut Ctx) -> Result<Vec<u8>, String> {
    let _ = std::fs::create_dir_all(&ctx.outdir);
    let (mut cues, mut joined): (Vec<u32>, Option<Wav>) = (Vec::new(), None);
    for at in [96, 2112] {
        let n = *d.get(at).ok_or("short Slicex state")? as usize;
        if n == 0 {
            continue;
        }
        let src = PathBuf::from(latin1(&d[at + 1..at + 1 + n]));
        let b = std::fs::read(&src).map_err(|e| format!("Slicex sample {}: {e}", src.display()))?;
        let o = b.windows(4).position(|w| w == b"cue ").ok_or_else(|| format!("{}: no cue points", src.display()))?;
        let tmp = ctx.outdir.join(src.file_name().unwrap());
        pcm_wav(&src, &tmp)?;
        let w = read_wav(&tmp)?;
        let j = joined.get_or_insert(Wav { ch: w.ch, width: w.width, rate: w.rate, pcm: Vec::new() });
        let base = (j.pcm.len() / (w.width * w.ch) as usize) as u32;
        j.pcm.extend(&w.pcm);
        (j.ch, j.width, j.rate) = (w.ch, w.width, w.rate);
        let mut c: Vec<u32> = (0..u32_at(&b, o + 8) as usize).map(|k| base + u32_at(&b, o + 12 + 24 * k + 20)).collect();
        c.sort();
        cues.extend(c);
    }
    let w = joined.ok_or("Slicex without samples")?;
    let dst = std::path::absolute(ctx.outdir.join("Slicex decks.wav")).map_err(|e| e.to_string())?;
    write_wav(&dst, &w)?;
    let frames = w.pcm.len() / (w.width * w.ch) as usize;
    let mut p = z(&dst.to_string_lossy());
    p.pop();
    let mut st = 13u32.to_le_bytes().to_vec();
    st.extend((((frames as f64) / w.rate as f64 * ctx.tempo / 60.0) as f32).to_le_bytes());
    st.extend((ctx.tempo as f32).to_le_bytes());
    st.extend([0; 20]);
    st.push(p.len() as u8);
    st.extend(&p);
    st.extend((cues.len() as u32).to_le_bytes());
    for c in cues {
        st.push(0);
        st.extend(c.to_le_bytes());
        st.extend((-1i32).to_le_bytes());
        st.extend((-1f32).to_le_bytes());
        st.push(0);
    }
    st.extend([1, 60]);
    st.extend(0u32.to_le_bytes());
    st.extend(w.rate.to_le_bytes());
    st.extend([1, 0, 1]);
    Ok(st)
}

/// FL 6's Multiband Compressor "Soft limiter" preset state (Maximus's substitute)
pub(crate) fn soft_limiter() -> Result<Vec<u8>, String> {
    preset(&image_line().join(r"FL Studio 6\Data\Patches\Plugin presets\Effects\Fruity Multiband Compressor\Soft limiter.fst"))
}

/// Fruity Free Filter freq knob (0..1) -> Hz, measured by the user; log-interpolated between points
const FF_HZ: [(f64, f64); 5] = [(0.2001, 253.0), (0.2703, 407.0), (0.7747, 5512.0), (0.918, 11025.0), (1.0, 16000.0)];

fn ff_x(hz: f64) -> f64 {
    for w in FF_HZ.windows(2) {
        let ((x0, h0), (x1, h1)) = (w[0], w[1]);
        if hz <= h1 || x1 == 1.0 {
            return (x0 + (x1 - x0) * (hz / h0).ln() / (h1 / h0).ln()).clamp(0.0, 1.0);
        }
    }
    unreachable!()
}

/// Love Philter cutoff 0..1 -> Free Filter freq 0..1: log, 42 % = 500 Hz by ear vs FL 8 (user), 100 % = 20 kHz
fn philter_x(v: f64) -> f64 {
    ff_x(500.0 * 40f64.powf((256.0 * v - 108.0) / 148.0))
}

/// Love Philter state -> Free Filter state (4 f32: type / 6, freq, Q, gain), or None when slot 0 has no filter
/// (order OFF). Love Philter: [u32 2][zlib]; inflated, a u32 text length at 60, the text, then 8 slots of
/// [12 u32 knobs (1 = cutoff)][u32 order][u32 type][envelopes]. ponytail: slot 0 only, envelopes dropped
fn philter_ff(d: &[u8]) -> Option<Vec<u8>> {
    let s = decompress_to_vec_zlib(d.get(4..)?).ok()?;
    let b = 64 + u32_at(&s, 60) as usize;
    let cutoff = i32_at(s.get(b..b + 56)?, 4);
    let (order, ty) = (u32_at(&s, b + 48), u32_at(&s, b + 52));
    if order == 0 {
        return None;
    }
    let t = match ty {
        2 => 2.0,
        4 | 6 => 1.0,
        _ => 0.0,
    };
    Some([t / 6.0, philter_x(cutoff as f64 / 256.0), 0.165, 0.66].iter().flat_map(|&x| (x as f32).to_le_bytes()).collect())
}

/// Each Love Philter insert slot (201 212 213) -> Free Filter, or dropped; returns the replaced slots' addresses
fn love_philter(ev: Ev) -> (Ev, HashSet<u16>) {
    let (mut out, mut addrs, mut k) = (Vec::new(), HashSet::new(), 0);
    while k < ev.len() {
        if ev[k].0 == 201 && trim0(&ev[k].1) == b"Fruity Love Philter" && ev.get(k + 2).is_some_and(|e| e.0 == 213) {
            if let Some(ff) = philter_ff(&ev[k + 2].1) {
                let w = &ev[k + 1].1;
                addrs.insert((0x2000 + (u32_at(w, 0) << 6) + u32_at(w, 4)) as u16);
                out.push((201, b"Fruity Free Filter\0".to_vec()));
                out.push(ev[k + 1].clone());
                out.push((213, ff));
            }
            k += 3;
            continue;
        }
        out.push(ev[k].clone());
        k += 1;
    }
    (out, addrs)
}

/// Links (227) to a Love Philter turned Free Filter: Filter 1 cutoff (param 9) -> Free Filter freq (param 1), the
/// source clip's values mapped through philter_x; other params' links (and their 230 formula) go.
/// ponytail: assumes the clip spans the full range
fn philter_links(ev: Ev, addrs: &HashSet<u16>) -> Ev {
    let (mut out, mut clips, mut drop): (Ev, HashSet<u16>, bool) = (Vec::new(), HashSet::new(), false);
    for (i, mut d) in ev {
        if i == 230 && drop {
            drop = false;
            continue;
        }
        drop = false;
        if i == 227 && d.len() >= 12 && addrs.contains(&u16_at(&d, 10)) {
            if u16_at(&d, 8) != 9 {
                drop = true;
                continue;
            }
            d[8..10].copy_from_slice(&1u16.to_le_bytes());
            clips.insert(u16_at(&d, 2));
        }
        out.push((i, d));
    }
    let mut ch = None;
    for (i, d) in out.iter_mut() {
        if *i == 64 {
            ch = Some(u16_at(d, 0));
        }
        if *i == 234 && ch.is_some_and(|c| clips.contains(&c)) {
            for k in 0..u32_at(d, 17) as usize {
                let o = 21 + 16 * k + 4;
                let v = philter_x(f32_at(d, o) as f64) as f32;
                d[o..o + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}

/// Slicex notes' mod X / Y (note bytes 18 / 19) drive its articulator filters; the slicer channel would turn them
/// into filter spikes, so set them neutral (128)
fn slicex_notes(ev: &mut Ev) {
    let (mut cur, mut ch) = (0, None);
    for (i, d) in ev.iter() {
        if *i == 64 {
            cur = u16_at(d, 0);
        }
        if *i == 201 && trim0(d) == b"Slicex" {
            ch = Some(cur);
        }
    }
    let Some(ch) = ch else { return };
    for (i, d) in ev.iter_mut() {
        if *i == 224 {
            for k in (0..d.len().saturating_sub(19)).step_by(20) {
                if u16_at(d, k + 6) == ch {
                    d[k + 18] = 128;
                    d[k + 19] = 128;
                }
            }
        }
    }
}

/// Fruity effects FL 6 runs as VSTs (Plugins\VST\*.dll): 212 byte 8 = 0
const VST6: [&[u8]; 5] = [b"Fruity Compressor", b"Fruity compressor", b"Fruity Reeverb", b"Fruity Phaser", b"Fruity Free Filter"];

fn subst(p: &[u8], fl6: bool) -> Option<&'static [u8]> {
    match p {
        b"Maximus" => Some(b"Fruity Multiband Compressor"),
        b"Slicex" => Some(b"Fruity slicer"),
        b"Fruity Parametric EQ 2" if fl6 => Some(b"Fruity Parametric EQ"),
        _ => None,
    }
}

/// FL 8 events -> FL 7 (fl6 false: FL 7 already has FL 8's Sytrus, window info, free clips, automation v2, Love
/// Philter and PEQ 2) or FL 6
pub(crate) fn from_8(ev: Ev, ctx: &mut Ctx, fl6: bool) -> Ev {
    // drop FL 8's 70th mixer track: from the last 149 up to the mixer block (225)
    let mut ev = match (ev.iter().rposition(|e| e.0 == 149), ev.iter().position(|e| e.0 == 225)) {
        (Some(a), Some(b)) if a < b => [&ev[..a], &ev[b..]].concat(),
        _ => ev,
    };
    let mut addrs = HashSet::new();
    if fl6 {
        (ev, addrs) = love_philter(ev);
    }
    slicex_notes(&mut ev);
    ctx.tempo = ev
        .iter()
        .find_map(|(i, d)| match i {
            66 => Some(u16_at(d, 0) as f64),
            156 => Some(u32_at(d, 0) as f64 / 1000.0),
            _ => None,
        })
        .unwrap_or(140.0);
    let (mut out, mut plugin, mut nenv): (Ev, Option<Vec<u8>>, u32) = (Vec::new(), None, 0);
    for (i, mut d) in ev {
        if fl6 && i == 216 && d.is_empty() {
            continue; // FL 6 writes no empty 216
        }
        if i == 64 {
            (plugin, nenv) = (None, 0);
        }
        let p = plugin.as_deref().unwrap_or_default();
        if i == 218 {
            nenv += 1;
            // the slicer gets Slicex's tail from the channel volume envelope (2nd 218).
            // ponytail: NewStuff's release 6500, not Slicex's own
            if p == b"Slicex" && nenv == 2 {
                for (k, v) in [-1i32, 100, 100, 100, 100, 128, 6500].iter().enumerate() {
                    d[4 + 4 * k..8 + 4 * k].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        if i == 201 {
            plugin = Some(name(&d));
            if let Some(s) = subst(&name(&d), fl6) {
                d = [s, b"\0"].concat();
            }
        }
        let p = plugin.as_deref().unwrap_or_default();
        match i {
            199 => d = if fl6 { b"6.0.4\0" } else { b"7.0.0\0" }.to_vec(),
            196 => d = ctx.sample(&d),
            213 if fl6 && p == b"Sytrus" => match crate::sytrus::convert(&d, 18, 15, crate::sytrus::v18_to_v15) {
                Some(s) => d = s,
                None => ctx.warnings.push("a Sytrus patch didn't convert to FL 6's format".into()),
            },
            213 if subst(p, fl6).is_some() => {
                let r = match p {
                    b"Maximus" => soft_limiter(),
                    b"Slicex" => slicex(&d, ctx),
                    _ => Ok(peq2(&d)),
                };
                d = r.unwrap_or_else(|e| {
                    ctx.warnings.push(format!("{} -> {}: {e}", latin1(p), latin1(subst(p, fl6).unwrap())));
                    Vec::new()
                });
            }
            213 if p == b"Fruity Parametric EQ 2" => d[0] = 1, // FL 7: FL 8's v2, same layout
            212 if fl6 && d.len() >= 36 => {
                // window info: FL 8 sets 0x40 in byte 16 and moved the u32 at 20 to 24
                let (f, v) = (d[16] & !0x40, d[24..28].to_vec());
                d[16..36].copy_from_slice(&[&[f, 0, 0, 0][..], &v, &[0; 8], &v].concat());
                // byte 8: 2 = native Fruity plugin, 0 = VST-hosted
                if VST6.contains(&subst(p, fl6).unwrap_or(p)) {
                    d[8] = 0;
                }
            }
            224 if fl6 => {
                for k in (0..d.len().saturating_sub(19)).step_by(20) {
                    d[k + 14] = 0; // note release: FL 6 has none
                }
            }
            225 => {
                // FL 8 adds param 133 on every track and a 70th track (b7 49, b6 64..71)
                d = d.chunks(12).filter(|r| !(r[5] == 31 && (r[4] == 133 || (r[7] == 49 && r[6] & 0xC0 == 64)))).flatten().copied().collect();
            }
            235 => d.truncate(69),
            233 if fl6 => {
                for k in (0..d.len().saturating_sub(27)).step_by(28) {
                    if d[k + 4..k + 8] == [0, 0, 0, 0x60] {
                        d[k + 7] = 0x50; // row items
                    } else if d[k + 4..k + 6] == [0, 0x50] && d[k + 7] >= 0x50 {
                        // FL 8 free pattern clip (item 0x5000 + p) -> FL 6 row item on pattern p's row
                        let pat = u16_at(&d, k + 6) - 0x5000;
                        d[k + 4..k + 8].copy_from_slice(&[0, 0, 0, 0x50]);
                        d[k + 12] = 0xE7u16.wrapping_sub(pat) as u8;
                        d[k + 14] = 3; // 3 on every FL 6 row item
                        d[k + 20..k + 28].copy_from_slice(&[(-1f32).to_le_bytes(), (-1f32).to_le_bytes()].concat());
                    }
                }
            }
            234 if fl6 => d = automation(d),
            _ => {}
        }
        out.push((i, d));
    }
    if fl6 {
        philter_links(out, &addrs)
    } else {
        out
    }
}

/// Converts an FL 8.0 project to FL `to` ("7.0.0", "6.0.4", "5.0.0" or "4.5.2"). Samples the target lacks are copied
/// into `outdir`.
pub fn from_8_0(b: &[u8], to: &str, outdir: &Path) -> Result<(Vec<u8>, Vec<String>), String> {
    let (h, events) = crate::parse(b)?;
    let ev: Ev = events.iter().map(|e| (e.id, e.data.to_vec())).collect();
    let il = image_line();
    let pf = std::env::var("FLPKIT_PF").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let data = match to {
        "7.0.0" => il.join(r"FL Studio 7\Data"),
        "6.0.4" => il.join(r"FL Studio 6\Data"),
        "5.0.0" => il.join(r"FLStudio5\Data"),
        "4.5.2" => Path::new(&pf).join(r"FLStudio4\Data"),
        _ => return Err(format!("no FL 8 step to {to}")),
    };
    let mut ctx = Ctx { data, outdir: outdir.into(), tempo: 140.0, warnings: Vec::new() };
    let mut ev = from_8(ev, &mut ctx, to != "7.0.0");
    if to == "5.0.0" || to == "4.5.2" {
        ev = crate::fl5::from_6(ev, &mut ctx.warnings);
    }
    if to == "4.5.2" {
        ev = crate::fl4::from_5(ev, &mut ctx.warnings);
    }
    Ok((crate::write(h, &ev), ctx.warnings))
}

#[cfg(test)]
mod tests {
    #[test]
    fn ff_x_points() {
        assert!((super::ff_x(253.0) - 0.2001).abs() < 1e-6 && (super::ff_x(16000.0) - 1.0).abs() < 1e-6);
        assert!(0.2001 < super::ff_x(368.0) && super::ff_x(368.0) < 0.2703);
    }
}
