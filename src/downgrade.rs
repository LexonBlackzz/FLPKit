//! Rewrites an FL 25/26 project as an FL 20.x or 12.3 one, one version step at a time (`STEPS`).
//! Every rule here was derived by diffing the same project saved by FL 2025/2026
//! and by FL 20.0/20.6/20.9.1 (samples/), see FORMAT.md "Downgrade".

use crate::{intro::{INTRODUCED, PLUGINS_INTRODUCED}, parse, text, version, write};

fn introduced(id: u8) -> Option<[u16; 4]> {
    INTRODUCED.iter().find(|(i, _)| *i == id).map(|(_, v)| *v)
}

/// MIDI Out v7 -> v6. Controller entries are `[idx][cc][19 bytes]` + `[pascal name][pascal label]`;
/// v7 inserts 4 zero bytes before the strings and names every controller "Controller" where v6
/// leaves the name empty, and labels them "CC" where FL 20 wrote "Volume" / "Pan" for CC 7 / 10.
fn midi_out_to_v6(d: &[u8]) -> Option<Vec<u8>> {
    let mut out = d.get(..322)?.to_vec();
    out[0] = 6;
    let mut o = 322;
    while *d.get(o)? != 0xff {
        let cc = *d.get(o + 1)?;
        out.extend(d.get(o..o + 21)?);
        o += 25;
        let mut s = || {
            let n = *d.get(o)? as usize;
            let t = d.get(o + 1..o + 1 + n)?.to_vec();
            o += 1 + n;
            Some(t)
        };
        let (name, label) = (s()?, s()?);
        let name = if name == b"Controller" { vec![] } else { name };
        // ponytail: only the two CC names seen in FL 20 saves
        let label = match (cc, label.as_slice()) {
            (7, b"CC") => b"Volume".to_vec(),
            (10, b"CC") => b"Pan".to_vec(),
            _ => label,
        };
        for t in [name, label] {
            out.push(t.len() as u8);
            out.extend(t);
        }
    }
    out.extend(&d[o..]);
    Some(out)
}

/// Parameter address `[id u8][type u8][target u16]`, shared by mixer params (225),
/// controller links (216, 227) and pattern automation (223).
/// FL 2026 stores mixer targets as 0x7000 + track<<6 + slot with "Current" = track 501;
/// FL 20 uses 0x2000 + track<<6 + slot with "Current" = 126. Returns false if the target
/// can't exist in FL 20 (mixer track 126..500).
fn addr_to_fl20(a: &mut [u8]) -> bool {
    let target = u16::from_le_bytes([a[2], a[3]]);
    if target >= 0x7000 {
        let (track, slot) = ((target - 0x7000) >> 6, target & 0x3f);
        let track = match track {
            501 => 126,
            t if t < 126 => t,
            _ => return false,
        };
        if a[1] == 0x20 && a[0] >= 126 {
            return false; // route to a mixer track FL 20 doesn't have
        }
        a[2..4].copy_from_slice(&(0x2000 + (track << 6) + slot).to_le_bytes());
        if a[1] == 0x20 {
            // route level to track n: FL 2026 type 0x20 id n, FL 20 type 0x1f id 64 + n
            a[1] = 0x1f;
            a[0] = a[0].wrapping_add(0x40);
        }
    } else if a[1] == 0x80 && matches!(a[0], 0x6e | 0x6f) {
        // channel volume / pan. ponytail: only these two seen; more ids likely moved too.
        a[0] -= 0x6c;
        a[1] = 0;
    }
    true
}

/// FL 20 -> FL 12 address: same layout, but 105 mixer tracks ("Current" 104 instead of 126).
/// False for tracks 104..125 and routes to them (type 0x1f id 64 + n).
fn addr_to_fl12(a: &mut [u8]) -> bool {
    let target = u16::from_le_bytes([a[2], a[3]]);
    if (0x2000..0x2000 + (127 << 6)).contains(&target) {
        let (track, slot) = ((target - 0x2000) >> 6, target & 0x3f);
        let track = match track {
            126 => 104,
            t if t < 104 => t,
            _ => return false,
        };
        if a[1] == 0x1f && (64 + 104..=64 + 126).contains(&a[0]) {
            return false;
        }
        a[2..4].copy_from_slice(&(0x2000 + (track << 6) + slot).to_le_bytes());
    }
    true
}

/// Applies `conv` to every `w`-byte record of `d` (addresses at offsets `at`), dropping
/// records whose address doesn't fit. Returns how many were dropped.
fn convert_records(d: &mut Vec<u8>, w: usize, at: &[usize], conv: fn(&mut [u8]) -> bool) -> usize {
    let before = d.len() / w;
    let mut out = Vec::with_capacity(d.len());
    for r in d.chunks_exact(w) {
        let mut r = r.to_vec();
        if at.iter().all(|&o| conv(&mut r[o..o + 4])) {
            out.extend(r);
        }
    }
    *d = out;
    before - d.len() / w
}

/// `[u32 id][u64 size][data]` records, as used by the Fruity Wrapper state. None if `b`
/// doesn't parse exactly.
fn records(b: &[u8]) -> Option<Vec<(u32, &[u8])>> {
    let (mut o, mut out) = (0, Vec::new());
    while o < b.len() {
        let id = u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?);
        let n = u64::from_le_bytes(b.get(o + 4..o + 12)?.try_into().ok()?) as usize;
        out.push((id, b.get(o + 12..o + 12 + n)?));
        o += 12 + n;
    }
    Some(out)
}

fn put_records(out: &mut Vec<u8>, recs: &[(u32, Vec<u8>)]) {
    for (id, d) in recs {
        out.extend(id.to_le_bytes());
        out.extend((d.len() as u64).to_le_bytes());
        out.extend(d);
    }
}

/// FL 12 effects that were VST DLLs (`Plugins\VST\*.dll`), hosted by the Fruity Wrapper.
/// Name, VST id (as FL 12 saves write it), program name, byte offset of the FL 20+ native
/// state's first i32 parameter (after its u32 version), and per VST parameter
/// (offset, scale): float = (int - offset) / scale.
/// Fitted on demo projects saved both by FL <= 12 (floats) and FL 12.9+ (ints), plus value
/// distributions; FL rounds the float back to its int, so these only need to land in the step.
/// Compressor p5 (type, int / 7) and Delay p4 ((int - 1) / 15) checked against FL 8's own Moholithic Filter.
/// ponytail: single-pair or distribution-only guesses: Compressor p1, Filter p0,
/// Flanger p0/p3, Balance pan, Chorus p6. Bass Boost (id unclear), 7 Band EQ, Center and
/// Phase Inverter (no FL 20+ states seen) stay native, warned.
const D1K: (f32, f32) = (0.0, 1024.0);
const D5K: (f32, f32) = (0.0, 5000.0);
const D10K: (f32, f32) = (0.0, 10000.0);
const D64K: (f32, f32) = (0.0, 65536.0);
type LegacyVst = (&'static str, &'static [u8; 4], &'static str, usize, &'static [(f32, f32)]);
const LEGACY_VSTS: &[LegacyVst] = &[
    ("Fruity Balance", b"laBF", "", 0, &[(-128.0, 256.0), (0.0, 320.0)]),
    ("Fruity Blood Overdrive", b"DOLB", "Reset", 4, &[D10K, D10K, D10K, (0.0, 1.0), D10K, D10K]),
    ("Fruity Chorus", b"CURF", "", 4, &[D1K, D5K, D1K, D5K, D5K, D5K, D1K, D1K, D1K, (0.0, 100.0), D5K, D1K]),
    ("Fruity Compressor", b"CFfU", "Full Reset", 4,
        &[(-600.0, 600.0), (3.9, 296.5), (-300.0, 600.0), (0.0, 4000.0), (0.0, 4000.0), (0.0, 7.0)]),
    ("Fruity Delay", b"leDF", "", 4, &[D1K, D1K, D1K, D1K, (1.0, 15.0), D1K]),
    ("Fruity Fast LP", b"PLFF", "", 4, &[D1K, D1K]),
    ("Fruity Filter", b"tlFF", "", 4, &[D1K, D1K, D1K, D1K, D1K, (0.0, 1.0)]),
    ("Fruity Flanger", b"0AMS", "", 4, &[D1K, D5K, D5K, (0.0, 360.0), D5K, D1K, (0.0, 100.0), D1K, D1K, D5K, D5K, D5K]),
    ("Fruity Free Filter", b"tFFF", "", 4, &[(100.0, 600.0), D1K, D1K, D1K]),
    ("Fruity Mute 2", b"peoP", "Default", 4, &[D1K, D1K]),
    ("Fruity Phaser", b"x413", "", 4,
        &[D5K, (0.0, 1000.0), (0.0, 1000.0), D1K, D1K, (0.0, 22.0), (0.0, 1000.0), D1K, D5K]),
    ("Fruity Reeverb", b"eRfU", "[1] Default", 4,
        &[D64K, D64K, (0.0, 250.0), D64K, D64K, (0.0, 40.0), D64K, D64K, D64K, D64K]),
];

/// A native FL 20+ legacy effect state as FL 12's Fruity Wrapper (kind 8) hosting its VST,
/// records as in SolidInc's FL 12 save.
fn legacy_vst(plugin: &str, d: &[u8]) -> Vec<u8> {
    let &(name, id, program, at, params) = LEGACY_VSTS.iter().find(|l| l.0 == plugin).unwrap();
    let int = |k: usize| d.get(at + 4 * k..at + 4 + 4 * k).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()));
    // VST2 chunk: magic, 12 bytes, float count, floats, u32 1, 25-byte program name
    let mut chunk = vec![0xf7, 0xff, 0xff, 0xff, 4, 0xfe, 0xff, 0xff, 0xff];
    chunk.resize(21, 0);
    chunk.extend((params.len() as u32).to_le_bytes());
    for (k, &(off, scale)) in params.iter().enumerate() {
        chunk.extend(((int(k) as f32 - off) / scale).clamp(0.0, 1.0).to_le_bytes());
    }
    chunk.extend(1u32.to_le_bytes());
    let mut prog = program.as_bytes().to_vec();
    prog.resize(25, 0);
    chunk.extend(prog);
    let path = format!(r"c:\program files (x86)\image-line\fl studio 12\Plugins\VST\{name}.dll");
    let recs = [
        (1, unhex("ffffffffffffffff0c0000000000000000000000")),
        // byte 9 varies per instance in FL 12 saves (0x80/0x81/0xa0); a0 is the most common
        (2, unhex("003102000085010000a07b0000000000000100000000000000")),
        (30, unhex("01000000010000000000000000000000")),
        (31, unhex("000000000100000000000000")),
        (32, unhex("000000000100000000000000")),
        (50, vec![0; 16]),
        (51, id.to_vec()),
        (54, name.as_bytes().to_vec()),
        (55, path.into_bytes()),
        (53, chunk),
    ];
    let mut out = 8u32.to_le_bytes().to_vec();
    put_records(&mut out, &recs);
    out
}

/// Fruity Wrapper (VST host) state, FL 2026 (kind 12) -> FL 20.x (kind 10). From the same
/// 6 VST3s + 1 VST2 saved by FL 20.6 and 2026 (samples/vsttest*):
/// - record 2 byte 9: flag bit 0 set by 2026 on VST3s, cleared
/// - record 53 (plugin data) VST3: sub-records 1, 3 (2) + a new 4 (parameter table), dropped
/// - record 53 VST2 (starts f7 ff ff ff): byte 4 flag bit 0 set by 2026, cleared
fn wrapper_to_fl20(d: &[u8]) -> Option<Vec<u8>> {
    if d.get(0..4)? != 12u32.to_le_bytes() {
        return None;
    }
    let mut recs: Vec<(u32, Vec<u8>)> = records(&d[4..])?.into_iter().map(|(i, b)| (i, b.to_vec())).collect();
    for (id, b) in &mut recs {
        match id {
            2 if b.len() > 9 => b[9] &= !1,
            53 if b.starts_with(&[1, 0, 0, 0]) => {
                let subs: Vec<(u32, Vec<u8>)> =
                    records(&b[4..])?.into_iter().filter(|(i, _)| *i != 4).map(|(i, s)| (i, s.to_vec())).collect();
                let mut nb = b[..4].to_vec();
                put_records(&mut nb, &subs);
                *b = nb;
            }
            53 if b.starts_with(&[0xf7, 0xff, 0xff, 0xff]) && b.len() > 4 => b[4] &= !1,
            53 => return None,
            _ => {}
        }
    }
    let mut out = 10u32.to_le_bytes().to_vec();
    put_records(&mut out, &recs);
    Some(out)
}

/// Converts an FLP (FL 25 or 26) to the given FL 20.x / 12.3 version string, e.g. "20.6.2.1549".
/// Returns the new file and human-readable warnings about lossy steps.
///
/// Runs [`STEPS`] from the newest down to the target: each step turns its version's format into
/// the one before it. Supporting a new FL release means adding a step at the top for what it
/// changed; supporting an older target, one at the bottom.
///
/// Refuses (unless `force`) projects the target can't load cleanly: VST states that don't
/// convert, plugins newer than the target. A half-loaded project is worse than none: FL happily
/// loads mismatched plugin states and then misbehaves (e.g. distorted audio).
pub fn downgrade(b: &[u8], target: &str, force: bool) -> Result<(Vec<u8>, Vec<String>), String> {
    let (h, events) = parse(b)?;
    let tv = version(target);
    let src_text = events.iter().find(|e| e.id == 199).map(|e| text(e.data)).unwrap_or_default();
    let sv = version(&src_text);
    if tv[0] != 20 && tv[..2] != [12, 3] {
        return Err(format!("target {target}: only FL 20.x and 12.3 targets are supported so far"));
    }
    if sv[0] < 25 {
        return Err(format!("source FL {src_text}: only FL 25/26 projects are supported so far"));
    }
    let mut c = Ctx {
        sv,
        playlist_w: if sv[0] >= 26 { 88 } else { 80 }, // see Project::parse
        ppq: h.ppq as f64,
        bpm: 140.0,
        ..Default::default()
    };
    if !matches!(tv[..2], [20, 6] | [20, 9] | [12, 3]) {
        c.warnings.push(format!("target {target} unverified (rules checked against 20.6.2 and 20.9.1 saves)"));
    }
    let steps: Vec<&Step> = STEPS.iter().filter(|s| tv < s.below).collect();
    let fl12 = tv[0] < 20;
    let mut out: Vec<(u8, Vec<u8>)> = Vec::with_capacity(events.len());
    let mut dropped = std::collections::BTreeMap::<u8, usize>::new();
    let mut natives = std::collections::BTreeSet::new();
    let mut too_new = std::collections::BTreeMap::new();

    'events: for e in &events {
        let (mut id, mut d) = (e.id, e.data.to_vec());
        match id {
            21 => c.chan_type = d.first().copied(),
            236 => c.chan_type = None,
            201 => {
                c.plugin = text(&d);
                let first_seen = PLUGINS_INTRODUCED.iter().find(|(n, _)| *n == c.plugin).map(|(_, v)| *v);
                match c.plugin.as_str() {
                    p if first_seen.is_some_and(|v| v > tv) => {
                        too_new.insert(p.to_string(), first_seen.unwrap());
                    }
                    // FL 12 hosted these as VST DLLs (Plugins\VST\*.dll) in the wrapper
                    "Fruity Bass Boost" | "Fruity 7 Band EQ" | "Fruity Center" | "Fruity Phase Inverter" if fl12 => {
                        natives.insert(format!("{} (a VST in FL 12, not converted: won't load)", c.plugin));
                    }
                    p if fl12 && LEGACY_VSTS.iter().any(|l| l.0 == p) => {}
                    // converted exactly, or byte-identical across versions (FL 20.6 / 12.3 saves)
                    "" | "MIDI Out" | "Fruity Limiter" | "Soundgoodizer" | "Gross Beat" | "Fruity Parametric EQ 2"
                    | "3x Osc" | "Fruity Compressor" | "Fruity Free Filter" | "Fruity Delay 3" | "Fruity Reeverb 2"
                    | "Transient Processor" | "Harmless" | "Wave Candy" | "Fruity Wrapper" | "Sytrus" | "Fruity Love Philter" => {}
                    // 20.7 / 20.8 had in-between layouts (see `to_20_6`)
                    "Maximus" if tv < [20, 7, 0, 0] || tv >= [20, 9, 0, 0] => {}
                    p => {
                        natives.insert(p.to_string());
                    }
                }
            }
            _ => {}
        }
        for s in &steps {
            if !(s.event)(&mut c, &mut id, &mut d)? {
                c.last = e.id;
                continue 'events;
            }
        }
        c.last = e.id;
        match id {
            199 => d = [target.as_bytes(), &[0]].concat(),
            // build number (159) follows the version string's last part
            159 if tv[3] != 0 => d = (tv[3] as u32).to_le_bytes().to_vec(),
            _ => {}
        }
        if introduced(id).map_or(true, |v| v > tv) {
            *dropped.entry(id).or_default() += 1;
            continue;
        }
        out.push((id, d));
    }
    let mut warnings = std::mem::take(&mut c.warnings);
    let mut blockers = Vec::new();
    if c.bad_vsts > 0 {
        blockers.push(format!("{} VST plugin state(s) in an unknown Fruity Wrapper format", c.bad_vsts));
    }
    for (p, v) in &too_new {
        blockers.push(format!("{p}: first seen in FL {}.{}.{}, not available in FL {target}", v[0], v[1], v[2]));
    }
    if !blockers.is_empty() {
        if !force {
            return Err(format!(
                "FL {target} can't load this project cleanly:\n  {}\n(--force converts anyway; those plugins will fail or misbehave)",
                blockers.join("\n  ")
            ));
        }
        warnings.extend(blockers.into_iter().map(|b| format!("forced: {b}")));
    }
    if !natives.is_empty() {
        let list: Vec<&str> = natives.iter().map(|s| s.as_str()).collect();
        warnings.push(format!(
            "native plugin states kept as FL {src_text} wrote them (FL {target} may reset them): {}",
            list.join(", ")
        ));
    }
    if c.legacy > 0 {
        warnings.push(format!(
            "FL 12: {} effect slots hosted as the FL 12 VSTs they were (Compressor, Reeverb, ...; parameters mapped approximately)",
            c.legacy
        ));
    }
    if !dropped.is_empty() {
        let list: Vec<String> = dropped.iter().map(|(id, n)| format!("{id}x{n}")).collect();
        warnings.push(format!("dropped events newer than FL {target}: {}", list.join(" ")));
    }
    let mut h = h;
    c.warnings = warnings;
    for s in &steps {
        if let Some(post) = s.post {
            post(&mut c, &mut out, &mut h);
        }
    }
    Ok((write(h, &out), c.warnings))
}

/// State shared by the steps while they walk the events.
#[derive(Default)]
struct Ctx {
    sv: [u16; 4],
    /// source playlist record size (233)
    playlist_w: usize,
    /// source name of the current plugin (201), also after a step renamed it
    plugin: String,
    /// event 21 of the current channel, None in the mixer
    chan_type: Option<u8>,
    /// previous source event id
    last: u8,
    warnings: Vec<String>,
    bad_vsts: usize,
    /// FL 12 legacy effects hosted as VSTs
    legacy: usize,
    /// FL 25+ per-clip gain / fades, for the 20.9 step's post pass
    clip_fx: Vec<ClipFx>,
    /// 233 events seen
    arrangements: usize,
    /// project PPQ (header) and tempo (156, before the playlist)
    ppq: f64,
    bpm: f64,
}

/// Rewrites one event (id and data) from the step's format to the one before it; Ok(false)
/// drops the event.
type EventRule = fn(&mut Ctx, &mut u8, &mut Vec<u8>) -> Result<bool, String>;
/// Runs on the whole converted event list, after all event rules.
type PostRule = fn(&mut Ctx, &mut Vec<(u8, Vec<u8>)>, &mut crate::Header);

struct Step {
    /// applies to targets older than this
    below: [u16; 4],
    event: EventRule,
    post: Option<PostRule>,
}

/// Newest first; each function's name is the format it produces.
const STEPS: &[Step] = &[
    Step { below: [21, 0, 0, 0], event: to_20_9, post: Some(per_clip_ports) },
    Step { below: [20, 9, 0, 0], event: to_20_8, post: None },
    Step { below: [20, 8, 0, 0], event: to_20_7, post: None },
    Step { below: [20, 7, 0, 0], event: to_20_6, post: None },
    Step { below: [20, 6, 0, 0], event: to_20_5, post: None },
    Step { below: [20, 1, 1, 0], event: to_20_1, post: None },
    Step { below: [20, 0, 0, 0], event: to_12_3, post: Some(|c, out, _| to_fl12_layout(out, &mut c.warnings)) },
];

/// FL 25+ per-clip gain and fades, which FL 20 lacks, as channel copies and volume automation.
fn per_clip_ports(c: &mut Ctx, out: &mut Vec<(u8, Vec<u8>)>, h: &mut crate::Header) {
    stretch_times(c, out);
    per_clip_gain(out, h, &c.clip_fx, &mut c.warnings);
    per_clip_fades(out, h, &c.clip_fx, &mut c.warnings);
}

/// Sampler stretch with Mul (215 at +104, 10000 x log2) but no time (+96): FL 2026 applies the
/// Mul alone, FL 12 doesn't until the mode is re-picked, which fills in the time: the sample's
/// length x Mul in 96-PPQ ticks (Avicii's reverse vocal 02: 2.0468 s x 2 at 128 BPM = 838,
/// user-toggled FL 12.3 and 20.6 saves). Reads the length from the sample's WAV header.
fn stretch_times(c: &mut Ctx, out: &mut [(u8, Vec<u8>)]) {
    let (mut i, mut skipped) = (0, Vec::new());
    while i < out.len() {
        if out[i].0 != 64 {
            i += 1;
            continue;
        }
        let end = (i + 1..out.len()).find(|&k| matches!(out[k].0, 64 | 99 | 233 | 236)).unwrap_or(out.len());
        let p215 = (i..end).find(|&k| out[k].0 == 215 && out[k].1.len() >= 112);
        let path = (i..end).find(|&k| out[k].0 == 196).map(|k| text(&out[k].1));
        if let (Some(k), Some(path)) = (p215, path) {
            let d = &out[k].1;
            let (time, mul) = (u32::from_le_bytes(d[96..100].try_into().unwrap()), i32::from_le_bytes(d[104..108].try_into().unwrap()));
            if time == 0 && mul != 0 {
                match wav_seconds(&path) {
                    Some(sec) => {
                        let t = sec * 2f64.powf(mul as f64 / 10000.0) * c.bpm / 60.0 * 96.0;
                        out[k].1[96..100].copy_from_slice(&(t.round() as u32).to_le_bytes());
                    }
                    None => skipped.push(path),
                }
            }
        }
        i = end;
    }
    if !skipped.is_empty() {
        c.warnings.push(format!(
            "stretch time not set (sample not a readable WAV), FL 12 may play these unstretched until the stretch mode is re-picked: {}",
            skipped.join(", ")
        ));
    }
}

/// Length of a PCM WAV file in seconds, from its RIFF header.
fn wav_seconds(path: &str) -> Option<f64> {
    let b = std::fs::read(path).ok()?;
    if b.get(..4)? != b"RIFF" || b.get(8..12)? != b"WAVE" {
        return None;
    }
    let (mut o, mut rate_align) = (12, None);
    while o + 8 <= b.len() {
        let n = u32::from_le_bytes(b[o + 4..o + 8].try_into().ok()?) as usize;
        match &b[o..o + 4] {
            b"fmt " => {
                let f = b.get(o + 8..o + 24)?;
                rate_align = Some((u32::from_le_bytes(f[4..8].try_into().ok()?), u16::from_le_bytes([f[12], f[13]])));
            }
            b"data" => {
                let (rate, align) = rate_align?;
                return (rate > 0 && align > 0).then(|| (n / align as usize) as f64 / rate as f64);
            }
            _ => {}
        }
        o += 8 + n + (n & 1);
    }
    None
}

/// FL 25 / 26 -> FL 20.9 (SolidInc, Avicii, vsttest, cctest saved by both).
fn to_20_9(c: &mut Ctx, id: &mut u8, d: &mut Vec<u8>) -> Result<bool, String> {
    match *id {
        // 25.2.3+ header block: 172, then an id-0 byte and an app-name text (192,
        // otherwise unused since FL12). None exist in FL 20.
        0 if c.last == 172 => return Ok(false),
        192 if c.sv >= [12, 0, 0, 0] => return Ok(false),
        // per-channel: 104 (u16) + 50 replaced 22 (u8) in 24.2.99
        104 => {
            *id = 22;
            *d = vec![d[0]];
        }
        // MIDI Out state v7 = v6 except in the controller list at 322 (cctest, FL 2026 vs 20.6)
        213 if c.plugin == "MIDI Out" && d.first() == Some(&7) => match midi_out_to_v6(d) {
            Some(n) => *d = n,
            None => c.warnings.push("MIDI Out state not converted: unexpected controller list".into()),
        },
        213 if c.plugin == "Fruity Wrapper" => match wrapper_to_fl20(d) {
            Some(n) => *d = n,
            None => c.bad_vsts += 1,
        },
        // Parametric EQ 2: append-only. v3 = 309 B (<= 20.7), v6 = 350 B (20.8+), v8 = 354 B
        // (21+); every SolidInc instance truncates to FL's own 20.6 / 20.9 bytes. Left at v8,
        // FL 20.6 froze and played noise (user-tested).
        // u32 at 292: 0/1 up to 20.99, 2/3 from 24.1 (Avicii: 3, froze FL 20.6). Corpus values
        // only; meaning unknown.
        213 if c.plugin == "Fruity Parametric EQ 2" && d.len() == 354 && d[0] >= 7 => {
            d[0] = 6;
            d.truncate(350);
            d[292] &= 1;
        }
        // append-only states, u32 version at byte 0 (demo corpus + SolidInc saves)
        213 if c.plugin == "3x Osc" && d.first() == Some(&14) && d.len() == 460 => {
            d[0] = 13;
            d.truncate(93);
        }
        213 if c.plugin == "Fruity Compressor" && d.first() == Some(&3) && d.len() == 36 => {
            d[0] = 2;
            d.truncate(32);
        }
        // Gross Beat v9 = v8 + one trailing byte. (2026 also held one more envelope point,
        // with its counts updated, which a v8 reader handles.) Left at v9, FL 20 resets it.
        // ponytail: one sample (vsttest); verify with customized Gross Beat presets.
        213 if c.plugin == "Gross Beat" && d.get(2) == Some(&9) => {
            d[2] = 8;
            d.pop();
        }
        233 => {
            let w = c.playlist_w;
            if d.len() % w != 0 {
                return Err(format!("playlist size {} not a multiple of {w}", d.len()));
            }
            let mut p = Vec::with_capacity(d.len() / w * 32);
            for (i, r) in d.chunks_exact(w).enumerate() {
                // start/end 0/len is kept: FL 20 also writes it, meaning "cut at len", while
                // -1/-1 shows the whole pattern (rewriting it un-trimmed Avicii's clips)
                p.extend(&r[..32]);
                let f = |o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                c.clip_fx.push((c.arrangements, i, f(52), f(36), f(44)));
            }
            cut_overlaps(c, &mut p, d);
            *d = p;
            c.arrangements += 1;
        }
        156 if d.len() >= 4 => c.bpm = u32::from_le_bytes(d[..4].try_into().unwrap()) as f64 / 1000.0,
        // insert routing, one byte per target track: FL 2026 104 (1 when empty), FL 20 127
        235 => d.resize(127, 0),
        // fields appended over time (sizes from the demo corpus): truncate to the target's.
        // A u32 field at 96 became an f32 in 24.2.99 (192 -> 192.0).
        215 => {
            if d.len() >= 100 {
                let f = f32::from_le_bytes(d[96..100].try_into().unwrap());
                d[96..100].copy_from_slice(&(f.round().max(0.0) as u32).to_le_bytes());
            }
            // stretch mode at 108: FL 2026's -2 (Stretch Pro) isn't applied by FL 20 on load (it
            // shows the settings, plays unstretched) until the mode is toggled, which saves -1
            // ("Stretch"): Avicii's reverse vocals, user-toggled FL 20.6 save. Stretch Pro's
            // formant (f64 at 160, 0.5 + cents / 2400) has no FL 20 equivalent: dropped.
            if d.len() >= 112 && d[108..112] == (-2i32).to_le_bytes() {
                d[108..112].copy_from_slice(&(-1i32).to_le_bytes());
            }
            d.truncate(168);
        }
        // plugin slot info: u32 at 0 is the mixer track. FL 2026's "Current" track is 501,
        // FL 20's is 126; FL 20.6 aborts the load on 501 (Avicii's Wave Candy, user-bisected)
        212 => {
            if d.len() >= 4 && d[..4] == 501u32.to_le_bytes() {
                d[..4].copy_from_slice(&126u32.to_le_bytes());
            }
            // plugin window size (i32 w, h at 44): 0 = the plugin's default, as FL 11..20 save never-resized
            // windows. Newer FL's sizes (or a replaced plugin's) made older versions tile small UIs (user)
            if d.len() >= 52 {
                d[44..52].fill(0);
            }
        }
        238 => d.truncate(66),
        216 if d.is_empty() => return Ok(false), // FL 20 doesn't write the empty one
        225 | 216 | 223 => {
            let n = convert_records(d, 12, &[4], addr_to_fl20);
            if n > 0 {
                c.warnings.push(format!("event {id}: dropped {n} records for mixer tracks 126+"));
            }
        }
        // controller link: source address at 0, target address at 8
        227 if convert_records(d, 20, &[0, 8], addr_to_fl20) > 0 => {
            c.warnings.push("dropped a controller link to mixer track 126+".into());
            return Ok(false);
        }
        // FL 20.9 writes 3 / 1 on every mixer track; FL 2026 writes 0
        165 if *d == [0; 4] => *d = 3u32.to_le_bytes().to_vec(),
        166 if *d == [0; 4] => *d = 1u32.to_le_bytes().to_vec(),
        _ => {}
    }
    Ok(true)
}

/// FL 2025+ never overlaps clips on one playlist track: a later clip cuts the earlier one off
/// at its start, unless a fade spans the overlap (the earlier's fade-out or the later's fade-in:
/// then they crossfade). FL 20 plays both, so cut the earlier one's length (and scale its end
/// offset) where FL 2026 does. From Avicii's 12 overlaps vs FL 2026's playlist (user screenshot).
/// ponytail: audio/automation clips only; pattern clips overlapping on a track not checked.
fn cut_overlaps(c: &mut Ctx, p: &mut [u8], src: &[u8]) {
    let w = c.playlist_w;
    let f = |r: &[u8], o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
    let u = |r: &[u8], o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
    let ms = |ticks: u32| ticks as f64 / c.ppq / c.bpm * 60000.0;
    // channel clips: (track, pos, record index)
    let mut clips: Vec<(u16, u32, usize)> = src
        .chunks_exact(w)
        .enumerate()
        .filter(|(_, r)| u16::from_le_bytes([r[6], r[7]]) < BASE)
        .map(|(i, r)| (u16::from_le_bytes([r[12], r[13]]), u(r, 0), i))
        .collect();
    clips.sort();
    let mut cut = 0;
    for pair in clips.windows(2) {
        let ((ta, pa, ia), (tb, pb, ib)) = (pair[0], pair[1]);
        let (a, b) = (&src[ia * w..ia * w + w], &src[ib * w..ib * w + w]);
        let len = u(a, 8);
        if ta != tb || pb <= pa || pb >= pa + len {
            continue;
        }
        let overlap = ms(pa + len - pb) - 1.0;
        if f(a, 44) as f64 >= overlap || f(b, 36) as f64 >= overlap {
            continue; // crossfade
        }
        let r = &mut p[ia * 32..ia * 32 + 32];
        let new = pb - pa;
        r[8..12].copy_from_slice(&new.to_le_bytes());
        let (start, end) = (f(r, 24), f(r, 28));
        if start >= 0.0 && end > start {
            let e = start + (end - start) * new as f32 / len as f32;
            r[28..32].copy_from_slice(&e.to_le_bytes());
        }
        cut += 1;
    }
    if cut > 0 {
        c.warnings.push(format!("{cut} overlapping clips cut where the next clip on their track starts, as FL 2025+ plays them"));
    }
}

/// FL 20.9 -> 20.8.
fn to_20_8(c: &mut Ctx, id: &mut u8, d: &mut Vec<u8>) -> Result<bool, String> {
    match *id {
        // Fruity Limiter v7 (20.9+) = v6 + one trailing byte
        213 if c.plugin == "Fruity Limiter" && d.first() == Some(&7) => {
            d[0] = 6;
            d.pop();
        }
        213 if c.plugin == "3x Osc" && d.first() == Some(&13) && d.len() == 93 => {
            d[0] = 12;
            d.truncate(92);
        }
        213 if c.plugin == "Fruity Free Filter" && d.first() == Some(&2) && d.len() == 21 => {
            d[0] = 1;
            d.truncate(20);
        }
        215 => d.truncate(158),
        _ => {}
    }
    Ok(true)
}

/// FL 20.8 -> 20.7.
fn to_20_7(c: &mut Ctx, id: &mut u8, d: &mut Vec<u8>) -> Result<bool, String> {
    // Parametric EQ 2 v6 -> v3 (see `to_20_9`); byte 305 is always 0 before 20.8
    if *id == 213 && c.plugin == "Fruity Parametric EQ 2" && d.len() == 350 && d[0] == 6 {
        d[0] = 3;
        d.truncate(309);
        d[305] = 0;
    }
    Ok(true)
}

/// FL 20.7 -> 20.6.
fn to_20_6(c: &mut Ctx, id: &mut u8, d: &mut Vec<u8>) -> Result<bool, String> {
    // Maximus: version at byte 2. 0x0f (20.8+) = 0x0a (<= 20.6) + 33 bytes inserted at 507
    // (SolidInc 20.6 vs 2026). 20.7/20.8 had in-between layouts: kept, with a warning.
    if *id == 213 && c.plugin == "Maximus" && d.len() == 1375 && d.get(2) == Some(&0x0f) {
        d[2] = 0x0a;
        d.drain(507..540);
    }
    Ok(true)
}

/// FL 20.6 -> 20.5 (and 20.0: nothing seen between them).
fn to_20_5(c: &mut Ctx, id: &mut u8, d: &mut Vec<u8>) -> Result<bool, String> {
    match *id {
        // MIDI Out v6 (20.6+) = v5 (20.0) + the "On reset" mask (u32) at the end
        213 if c.plugin == "MIDI Out" && d.first() == Some(&6) => {
            d.truncate(d.len() - 4);
            d[0] = 5;
        }
        // Love Philter v12 / Sytrus v25 (20.6) appended one byte to v11 / v24 (20.0)
        213 => zlib_state(c, d, &[("Fruity Love Philter", 12), ("Sytrus", 25)], |_, s| {
            s.pop();
        }),
        215 => d.truncate(157),
        _ => {}
    }
    Ok(true)
}

/// FL 20.1 -> 20.0.
fn to_20_1(_: &mut Ctx, id: &mut u8, d: &mut Vec<u8>) -> Result<bool, String> {
    if *id == 238 {
        d.truncate(62);
    }
    Ok(true)
}

/// FL 20.0 -> 12.3 (newstuff / SolidInc saved by FL 12.3.0.72; FORMAT.md "Downgrade to FL 12.3").
fn to_12_3(c: &mut Ctx, id: &mut u8, d: &mut Vec<u8>) -> Result<bool, String> {
    let legacy = LEGACY_VSTS.iter().any(|l| l.0 == c.plugin);
    match *id {
        // FL 12 hosted these as VST DLLs (Plugins\VST\*.dll) in the wrapper
        201 if legacy => {
            *d = utf16z("Fruity Wrapper");
            c.legacy += 1;
        }
        213 if legacy => *d = legacy_vst(&c.plugin, d),
        // MIDI Out v5 (20.0) = v4 (12.3)
        213 if c.plugin == "MIDI Out" && d.first() == Some(&5) => d[0] = 4,
        // FL 12.3's wrapper (kind 8) has the same records as FL 20's (kind 10)
        213 if c.plugin == "Fruity Wrapper" && d.starts_with(&10u32.to_le_bytes()) => d[0] = 8,
        // Reeverb 2 v17 (20.0+) inserted a u32 at 56 into v16 (12.3: 58 bytes)
        213 if c.plugin == "Fruity Reeverb 2" && d.len() == 66 && d[0] == 17 => {
            *d = [&d[..56], &d[60..62]].concat();
            d[0] = 16;
        }
        // Love Philter v11 (20.0) inserted a byte at 54; Sytrus v24 appended one. Kept as is,
        // FL 12 misread them and automation drove the wrong knobs (user-tested).
        213 => zlib_state(c, d, &[("Fruity Love Philter", 11), ("Sytrus", 24)], |lp, s| {
            if lp {
                s.remove(54);
            } else {
                s.pop();
            }
        }),
        // FL 12: 8 bytes, and byte 4 bit 0x20 is always set (FL 20 clears it); 20.9+ set 0x80 in byte 3
        236 if d.len() >= 8 => {
            d.truncate(8);
            d[3] &= 0x7f;
            d[4] |= 0x20;
        }
        196 => {
            if let Some(rest) = text(d).strip_prefix("%FLStudioFactoryData%\\Data\\") {
                *d = utf16z(&format!("%FLStudioData%\\{rest}"));
            }
        }
        215 => d.truncate(125),
        238 => d.truncate(49),
        // insert routing: 105 targets (FL 20: 127)
        235 => d.truncate(105),
        212 => {
            if d.len() >= 4 && d[..4] == 126u32.to_le_bytes() {
                // mixer track: FL 20's "Current" 126 is FL 12's 104
                d[..4].copy_from_slice(&104u32.to_le_bytes());
            } else if d.len() > 16 && d[16] == 0x50 && c.chan_type.is_some_and(|t| t != 2) {
                // FL 12 sets bit 0x04 of byte 16 on channels without a plugin (sampler, audio
                // clip, layer, automation: type != 2); FL 20+ clears it (SolidInc FL 12 vs 20)
                d[16] |= 4;
            }
        }
        225 | 216 | 223 => {
            let n = convert_records(d, 12, &[4], addr_to_fl12);
            if n > 0 {
                c.warnings.push(format!("event {id}: dropped {n} records for mixer tracks 104+"));
            }
        }
        227 if convert_records(d, 20, &[0, 8], addr_to_fl12) > 0 => {
            c.warnings.push("dropped a controller link to mixer track 104+".into());
            return Ok(false);
        }
        _ => {}
    }
    Ok(true)
}

/// `[u16 3][u16 version][zlib]` plugin states (Love Philter, Sytrus): if `c.plugin` is one of
/// `from` at its version, decompresses, applies `f` (true for Love Philter), lowers the version.
fn zlib_state(c: &mut Ctx, d: &mut Vec<u8>, from: &[(&str, u16)], f: fn(bool, &mut Vec<u8>)) {
    let Some(&(name, v)) = from.iter().find(|p| p.0 == c.plugin) else { return };
    if d.len() <= 4 || d[..2] != [3, 0] || u16::from_le_bytes([d[2], d[3]]) != v {
        return;
    }
    match miniz_oxide::inflate::decompress_to_vec_zlib(&d[4..]) {
        Ok(mut s) => {
            f(name == "Fruity Love Philter", &mut s);
            *d = [&[3, 0][..], &(v - 1).to_le_bytes(), &miniz_oxide::deflate::compress_to_vec_zlib(&s, 6)].concat();
        }
        Err(_) => c.warnings.push(format!("{name}: state not zlib, kept")),
    }
}

/// Levels adjustment volume (229 at +4) vs dB, typed into FL 2026 (`samples/adjustments(FL2026).flp`);
/// the max is FL 20.6's hint (x12.00). Same field and range in FL 20.6.
const LEVELS_VOL: [(i32, f32); 6] = [(6705, -12.0), (9575, -6.0), (12800, 0.0), (16240, 6.0), (19798, 12.0), (25600, 21.584)];
/// Below the table: amplitude ~ raw^2.136, the slope at its low end.
const LEVELS_LOW_K: f32 = 2.136;

fn levels_db(raw: i32) -> f32 {
    let t = &LEVELS_VOL;
    if raw <= t[0].0 {
        return t[0].1 + 20.0 * LEVELS_LOW_K * (raw.max(1) as f32 / t[0].0 as f32).log10();
    }
    let i = t.iter().position(|p| p.0 >= raw).unwrap_or(t.len() - 1).max(1);
    let ((x0, y0), (x1, y1)) = (t[i - 1], t[i]);
    y0 + (y1 - y0) * (raw - x0) as f32 / (x1 - x0) as f32
}

fn levels_raw(db: f32) -> i32 {
    let t = &LEVELS_VOL;
    if db <= t[0].1 {
        return (t[0].0 as f32 * 10f32.powf((db - t[0].1) / (20.0 * LEVELS_LOW_K))).round() as i32;
    }
    let i = t.iter().position(|p| p.1 >= db).unwrap_or(t.len() - 1).max(1);
    let ((x0, y0), (x1, y1)) = (t[i - 1], t[i]);
    (x0 as f32 + (x1 - x0) as f32 * ((db - y0) / (y1 - y0)).min(1.0)).round() as i32
}

/// FL 12 has 105 mixer tracks and 199 playlist tracks (FL 20: 127, 500):
/// - mixer tracks 104..125 go (each runs to its 147, after its 236); "Current" (126) becomes 104;
/// - playlist clip track fields count down from the last track: FL 20's 499 is FL 12's 198;
/// - FL 12 writes 238 (+ its 239 name) for at least 33 tracks, or up to the last one used.
fn to_fl12_layout(out: &mut Vec<(u8, Vec<u8>)>, warnings: &mut Vec<String>) {
    let tracks: Vec<usize> = (0..out.len()).filter(|&i| out[i].0 == 236).collect();
    if tracks.len() == 127 {
        let end = |k: usize| (tracks[k]..out.len()).find(|&i| out[i].0 == 147).unwrap();
        let (a, b) = (end(103) + 1, end(125) + 1);
        out.drain(a..b);
    } else {
        warnings.push(format!("FL 12: expected 127 mixer tracks, found {}; mixer left as is", tracks.len()));
    }
    let (mut dropped, mut last_used) = (0, 0u32);
    for e in out.iter_mut().filter(|e| e.0 == 233) {
        let mut p = Vec::with_capacity(e.1.len());
        for r in e.1.chunks_exact(32) {
            let t = u16::from_le_bytes([r[12], r[13]]);
            if t < 301 {
                dropped += 1;
                continue;
            }
            let mut r = r.to_vec();
            r[12..14].copy_from_slice(&(t - 301).to_le_bytes());
            last_used = last_used.max(199 - (t - 301) as u32);
            p.extend(r);
        }
        e.1 = p;
    }
    if dropped > 0 {
        warnings.push(format!("FL 12: dropped {dropped} clips on playlist tracks 200+"));
    }
    let keep = last_used.max(33);
    let mut i = 0;
    while i < out.len() {
        if out[i].0 == 238 {
            let idx = u32::from_le_bytes(out[i].1[..4].try_into().unwrap());
            let named = out.get(i + 1).is_some_and(|e| e.0 == 239);
            if idx > 199 || (idx > keep && !named) {
                out.drain(i..i + 1 + named as usize);
                continue;
            }
        }
        i += 1;
    }
}

/// Channel blocks in `out`: each runs from its 64 to the next 64, stopping early at pattern
/// names/colours (65 ...) that can sit between them; the last one ends before the arrangement (99).
/// Returns the blocks and where new channels go.
fn channel_blocks(out: &[(u8, Vec<u8>)], channels: u16) -> Option<(Vec<std::ops::Range<usize>>, usize)> {
    let starts: Vec<usize> = (0..out.len()).filter(|&i| out[i].0 == 64).collect();
    let last = *starts.last()?;
    // the arrangement (99, FL 20+) or the playlist (233, FL 12) follows the channels
    let end = (last..out.len()).find(|&i| matches!(out[i].0, 99 | 233))?;
    if starts.len() != channels as usize {
        return None;
    }
    let blocks: Vec<_> = (0..starts.len())
        .map(|c| {
            let stop = starts.get(c + 1).copied().unwrap_or(end);
            starts[c]..(starts[c]..stop).find(|&i| out[i].0 == 65).unwrap_or(stop)
        })
        .collect();
    let end = blocks.last()?.end;
    Some((blocks, end))
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
}

/// A copy of a channel block as channel `idx`, renamed with `name(old name)`.
fn copy_channel(out: &[(u8, Vec<u8>)], block: std::ops::Range<usize>, idx: u16, name: impl Fn(&str) -> String) -> Vec<(u8, Vec<u8>)> {
    let mut b = out[block].to_vec();
    b[0].1 = idx.to_le_bytes().to_vec();
    for (id, d) in &mut b {
        if *id == 203 {
            *d = utf16z(&name(&crate::text(d)));
        }
    }
    b
}

/// One playlist clip's FL 2025+ extras: (arrangement = nth 233 event, record, gain,
/// fade-in ms, fade-out ms).
type ClipFx = (usize, usize, f32, f32, f32);

fn nth_event(out: &[(u8, Vec<u8>)], id: u8, n: usize) -> usize {
    (0..out.len()).filter(|&i| out[i].0 == id).nth(n).unwrap()
}

/// FL 11+ pattern base: playlist items above it are patterns, below are channels.
const BASE: u16 = 0x5000;

fn channel_types(out: &[(u8, Vec<u8>)], blocks: &[std::ops::Range<usize>]) -> Vec<Option<u8>> {
    blocks.iter().map(|b| out[b.clone()].iter().find(|e| e.0 == 21).map(|e| e.1[0])).collect()
}

/// FL 2025+ playlist clips carry their own gain (f32 at +52 of the record, linear). FL 20 has
/// none: each audio/sampler channel used with a gain != 1 gets a copy, appended after the last
/// channel, with the gain added to its Levels adjustment volume (229), and its clips are
/// pointed at it. Clips with the same channel and gain share one copy.
fn per_clip_gain(out: &mut Vec<(u8, Vec<u8>)>, h: &mut crate::Header, clips: &[ClipFx], warnings: &mut Vec<String>) {
    let Some((blocks, end)) = channel_blocks(out, h.channels) else {
        warnings.push("per-clip gain/fades not ported: unexpected channel layout".into());
        return;
    };
    let types = channel_types(out, &blocks);
    let mut copies: Vec<(usize, u32, u16)> = Vec::new(); // (channel, gain bits, new index)
    let mut new_blocks = Vec::new();
    let mut clamped = 0;
    for &(arr, rec, gain, _, _) in clips {
        let ev = nth_event(out, 233, arr);
        let at = rec * 32 + 6;
        let item = u16::from_le_bytes([out[ev].1[at], out[ev].1[at + 1]]);
        let chan = item as usize;
        // samplers (0) and audio clips (4) only: automation clips (5) store 0 there
        if item > BASE || chan >= blocks.len() || !matches!(types[chan], Some(0 | 4)) || gain == 1.0 {
            continue;
        }
        let idx = match copies.iter().find(|c| c.0 == chan && c.1 == gain.to_bits()) {
            Some(c) => c.2,
            None => {
                let idx = h.channels + copies.len() as u16;
                let db = 20.0 * gain.log10();
                let mut b = copy_channel(out, blocks[chan].clone(), idx, |n| format!("{n} ({db:+.1} dB)"));
                // Levels adjustment (229): i32 pan, vol (12800 = 100%, max 25600), ?, mod x, mod y
                for (id, d) in &mut b {
                    if *id == 229 && d.len() >= 8 {
                        let target = levels_db(i32::from_le_bytes(d[4..8].try_into().unwrap())) + db;
                        clamped += (target > LEVELS_VOL.last().unwrap().1) as usize;
                        d[4..8].copy_from_slice(&levels_raw(target).to_le_bytes());
                    }
                }
                new_blocks.extend(b);
                copies.push((chan, gain.to_bits(), idx));
                idx
            }
        };
        out[ev].1[at..at + 2].copy_from_slice(&idx.to_le_bytes());
    }
    if !copies.is_empty() {
        h.channels += copies.len() as u16;
        out.splice(end..end, new_blocks);
        warnings.push(format!("per-clip gain: added {} channel copies", copies.len()));
    }
    if clamped > 0 {
        warnings.push(format!("per-clip gain: {clamped} channel copies needed more than the max Levels adjustment volume"));
    }
}

/// An FL 20.9.1 automation clip channel (SolidInc save), minus its index (64), name (203) and
/// the point count + points of 234.
const AUTO_TEMPLATE: &[(u8, &str)] = &[
    (21, "05"),
    (201, "0000"),
    (212, "0000000000000000ffffffff0000000050000000000000000000000000000000000000005a000000ae0000000000000000000000"),
    (203, ""),
    (155, "00000000"),
    (128, "9f5f7c00"),
    (0, "01"),
    (209, "0000000000190000000000000400000090000000"),
    (138, "80008000"),
    (139, "00000100"),
    (89, "0000"),
    (97, "8000"),
    (69, "8000"),
    (86, "0001"),
    (71, "0004"),
    (83, "0000"),
    (74, "0000"),
    (75, "0000"),
    (76, "0000"),
    (85, "0008"),
    (131, "00008000"),
    (70, "0000"),
    (22, "00"),
    (219, "000000000032000000000000000100000000000000000000"),
    (229, "0000000000320000000000000000000000000000"),
    (221, "01000000f401000000"),
    (215, "ffffffff0000000001000000ffffffff3c0000000000803f0000803f0000803f0000803f0000803f0000000001000000ffffffff000400003000000000000100a7050000000000000001000000000000000000000000000000000000010000000000000000000000000000000000000002000000feffffffffffffff000000000000000000000000000000000000f03f0000000000000000ffffffff00000000000000000000e03f"),
    (132, "06000600"),
    (144, "00000000"),
    (145, "01000000"),
    (234, "010000004000000000040000000300000001000000ffffffffffffffffffffffffffffffff800000008000000000000000800000000500000003000000010000000000000000000000000000000000f03f00000000000000000100000000000000fffffffffffffffffffffffffbb2000000000000000000000000000000000000"),
    (32, "00"),
    (228, "64000000000000000000000000000000"),
    (228, "3c000000000000000000000000000000"),
    (218, "000000000000000064000000204e0000204e00003075000032000000204e00000000000064000000204e000000000000b680000000000000000000000000000000000000"),
    (218, "040000000000000064000000204e0000204e00003075000032000000204e00000000000064000000204e000000000000b68000000000000000000000000000009bffffff"),
    (218, "000000000000000064000000204e0000204e00003075000032000000204e00000000000064000000204e000000000000b680000000000000000000000000000000000000"),
    (218, "000000000000000064000000204e0000204e00003075000032000000204e00000000000064000000204e000000000000b680000000000000000000000000000000000000"),
    (218, "000000000000000064000000204e0000204e00003075000032000000204e00000000000064000000204e000000000000b680000000000000000000000000000000000000"),
    (143, "03000000"),
    (20, "00"),
];

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// Automation point (tension, mode) for a segment: flat, and the fade shape. Mode 1 = Double curve
/// (ponytail: from FL's historical point-mode order, unconfirmed; `automationclipshowcase`
/// has modes 0x03, 0x05, 0x0b unlabelled).
const FLAT: (f32, u8) = (0.0, 0);
const FADE: (f32, u8) = (0.1, 1);

/// (start tick, end tick, fade-in ms, fade-out ms, record index)
type Span = (f64, f64, f32, f32, usize);

/// FL 2025+ clip fades (ms) become one volume automation clip per channel: it holds the channel's
/// volume and ramps 0 -> volume -> 0 linearly around each faded clip. Clips of one channel that
/// overlap (crossfades) can't share that curve, so each overlapping one moves to a copy of the
/// channel (a "lane"). Runs after `per_clip_gain`, so clips already point at their gain copies.
const MIN_FADE_MS: f32 = 25.0;

fn per_clip_fades(out: &mut Vec<(u8, Vec<u8>)>, h: &mut crate::Header, clips: &[ClipFx], warnings: &mut Vec<String>) {
    let fades: std::collections::HashMap<(usize, usize), (f32, f32)> =
        clips.iter().filter(|c| c.3 > 1.0 || c.4 > 1.0).map(|c| ((c.0, c.1), (c.3, c.4))).collect();
    if fades.is_empty() {
        return;
    }
    let Some((blocks, end)) = channel_blocks(out, h.channels) else { return };
    let types = channel_types(out, &blocks);
    let bpm = out.iter().find(|e| e.0 == 156).map_or(140.0, |e| u32::from_le_bytes(e.1[..4].try_into().unwrap()) as f64 / 1000.0);
    let ppq = h.ppq as f64;
    let ms_ticks = |ms: f32| ms as f64 / 60000.0 * bpm * ppq; // ponytail: ignores tempo changes
    let rec = |out: &[(u8, Vec<u8>)], arr: usize, r: usize| {
        let d = &out[nth_event(out, 233, arr)].1[r * 32..r * 32 + 32];
        let u32at = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) as f64;
        (u16::from_le_bytes([d[6], d[7]]), u32at(0), u32at(8))
    };
    // channel volume already automated (227 link to [00 00][chan]): leave those alone
    let automated: std::collections::HashSet<u16> = out
        .iter()
        .filter(|e| e.0 == 227 && e.1.len() >= 12 && e.1[8..10] == [0, 0])
        .map(|e| u16::from_le_bytes([e.1[10], e.1[11]]))
        .collect();

    // every clip (faded or not) of each channel that has a faded clip, per arrangement
    let mut by_chan: std::collections::BTreeMap<(usize, u16), Vec<Span>> = Default::default();
    let mut skipped = 0;
    let mut keys: Vec<_> = fades.keys().copied().collect();
    keys.sort();
    for (arr, r) in keys {
        let (item, ..) = rec(out, arr, r);
        if item > BASE || item as usize >= blocks.len() || !matches!(types[item as usize], Some(0 | 4)) {
            continue;
        }
        if automated.contains(&item) {
            skipped += 1;
            continue;
        }
        if by_chan.contains_key(&(arr, item)) {
            continue;
        }
        let n = out[nth_event(out, 233, arr)].1.len() / 32;
        let list = (0..n)
            .filter(|&k| rec(out, arr, k).0 == item)
            .map(|k| {
                let (_, pos, len) = rec(out, arr, k);
                let (fi, fo) = fades.get(&(arr, k)).copied().unwrap_or((0.0, 0.0));
                (pos, pos + len, fi, fo, k)
            })
            .collect();
        by_chan.insert((arr, item), list);
    }

    let all_records: Vec<[u8; 32]> = out.iter().filter(|e| e.0 == 233).flat_map(|e| e.1.chunks_exact(32).map(|r| r.try_into().unwrap()).collect::<Vec<_>>()).collect();
    // playlist track field counts down from the top track: new tracks go below the lowest used
    let mut free_track = all_records.iter().map(|r| u16::from_le_bytes([r[12], r[13]])).min().unwrap_or(500);
    let size215 = out.iter().find(|e| e.0 == 215).map(|e| e.1.len());

    let (mut new_blocks, mut links, mut lanes_added) = (Vec::new(), Vec::new(), 0);
    let (mut next, mut placed) = (h.channels, 0);
    let mut pending: Vec<(usize, f64, f64, u16, u16)> = Vec::new(); // (arrangement, pos, len, automation channel, target)
    // automation channels by (target channel, clip length, fade-in, fade-out in ticks)
    let mut autos: std::collections::HashMap<(u16, u64, u64, u64), u16> = Default::default();
    for ((arr, chan), mut list) in by_chan {
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        // lane 0 is the channel itself, lane k > 0 a copy for clips overlapping earlier ones
        let mut lanes: Vec<(u16, f64, Vec<Span>)> = Vec::new();
        for clip in list {
            let lane = match lanes.iter().position(|l| l.1 <= clip.0) {
                Some(i) => i,
                None => {
                    let idx = if lanes.is_empty() {
                        chan
                    } else {
                        let n = lanes.len();
                        new_blocks.extend(copy_channel(out, blocks[chan as usize].clone(), next, |name| format!("{name} (crossfade {n})")));
                        lanes_added += 1;
                        next += 1;
                        next - 1
                    };
                    lanes.push((idx, 0.0, Vec::new()));
                    lanes.len() - 1
                }
            };
            lanes[lane].1 = clip.1;
            if lanes[lane].0 != chan {
                let ev = nth_event(out, 233, arr);
                out[ev].1[clip.4 * 32 + 6..clip.4 * 32 + 8].copy_from_slice(&lanes[lane].0.to_le_bytes());
            }
            lanes[lane].2.push(clip);
        }
        let block = &out[blocks[chan as usize].clone()];
        let name = block.iter().find(|e| e.0 == 203).map(|e| crate::text(&e.1)).unwrap_or_default();
        let vol = block.iter().find(|e| e.0 == 219).map_or(10000.0, |e| u32::from_le_bytes(e.1[4..8].try_into().unwrap()) as f64) / 12800.0;
        for (lane, (target, _, spans)) in lanes.into_iter().enumerate() {
            for (s, e, fi, fo, _) in spans {
                // fades up to 25 ms are declicks (two thirds of Avicii's 1969); FL's sampler
                // declicks itself, and an automation clip each made FL 12 choke (user-tested)
                let (fi, fo) = (if fi > MIN_FADE_MS { fi } else { 0.0 }, if fo > MIN_FADE_MS { fo } else { 0.0 });
                if fi == 0.0 && fo == 0.0 {
                    continue;
                }
                let len = (e - s).round();
                let fi = ms_ticks(fi).min(len).round();
                let fo = ms_ticks(fo).min(len - fi).round();
                let key = (target, len as u64, fi as u64, fo as u64);
                let auto = match autos.get(&key) {
                    Some(&a) => a,
                    None => {
                        let auto = next;
                        next += 1;
                        autos.insert(key, auto);
                        // (tick, value) within the clip; equal ticks make a step. The curve ends
                        // back at full volume so later unfaded clips of the channel aren't muted.
                        // FL 2026's fade looks like FL 20's Double curve at 10% tension (user)
                        let mut pts = vec![(0.0, if fi > 0.0 { 0.0 } else { vol }, FLAT)];
                        if fi > 0.0 {
                            pts.push((fi, vol, FADE));
                        }
                        if fo > 0.0 {
                            pts.extend([(len - fo, vol, FLAT), (len, 0.0, FADE), (len, vol, FLAT)]);
                        } else {
                            pts.push((len, vol, FLAT));
                        }
                        let suffix = if lane > 0 { format!(" (crossfade {lane})") } else { String::new() };
                        new_blocks.push((64, auto.to_le_bytes().to_vec()));
                        for &(id, hex) in AUTO_TEMPLATE {
                            let mut d = unhex(hex);
                            match id {
                                203 => d = utf16z(&format!("{name}{suffix} - fade")),
                                215 => {
                                    if let Some(n) = size215 {
                                        d.resize(n, 0);
                                    }
                                }
                                // header (17 bytes), point count, points (f64 delta beats, f64 value,
                                // f32 tension, u32 flags), then the template's clip settings
                                234 => {
                                    let mut p = Vec::new();
                                    let mut prev = 0.0;
                                    for (i, &(t, v, (tension, mode))) in pts.iter().enumerate() {
                                        p.extend(((t - prev) / ppq).to_le_bytes());
                                        p.extend(v.to_le_bytes());
                                        p.extend(tension.to_le_bytes());
                                        // flags: mode, 0, 0, tension sign (02 = none, 01 = +, ff = -;
                                        // the first point has 00), as FL 20.6 re-saves them
                                        let sign = if i == 0 { 0 } else if tension > 0.0 { 1 } else if tension < 0.0 { 0xff } else { 2 };
                                        p.extend([mode, 0, 0, sign]);
                                        prev = t;
                                    }
                                    d = [&d[..17], &(pts.len() as u32).to_le_bytes()[..], &p, &d[17..]].concat();
                                }
                                _ => {}
                            }
                            new_blocks.push((id, d));
                        }
                        // link (as FL 20.6 writes it): automation channel -> volume (param 0) of `target`
                        let (a, t) = (auto.to_le_bytes(), target.to_le_bytes());
                        links.push((227u8, [&[0, 0][..], &a, &[0, 0], &a, &[0, 0], &t, &[8, 0, 0, 0, 0xd5, 1, 0, 0]].concat()));
                        auto
                    }
                };
                pending.push((arr, s, len, auto, target));
            }
        }
    }
    // pack the fade clips onto as few new tracks as possible (first track they don't overlap on)
    pending.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap());
    // A clip's last point (back to full volume at its end) isn't applied by FL: the channel stayed at 0
    // after a fade-out (Avicii's Progressive Kick from bar 65, every target). One tick longer, it is,
    // unless another fade clip of the same channel starts on that tick.
    let starts: std::collections::HashSet<(usize, u16, u64)> = pending.iter().map(|p| (p.0, p.4, p.1 as u64)).collect();
    let pending: Vec<_> = pending
        .iter()
        .map(|&(arr, s, len, auto, target)| {
            let free = !starts.contains(&(arr, target, (s + len) as u64));
            (arr, s, if free { len + 1.0 } else { len }, auto)
        })
        .collect();
    let mut tracks: Vec<(usize, u16, f64)> = Vec::new(); // (arrangement, track, busy until)
    for (arr, s, len, auto) in pending {
        let track = match tracks.iter_mut().find(|t| t.0 == arr && t.2 <= s) {
            Some(t) => {
                t.2 = s + len;
                t.1
            }
            None => {
                free_track = free_track.saturating_sub(1);
                tracks.push((arr, free_track, s + len));
                free_track
            }
        };
        // playlist record (32 bytes): pos, base, item, len, track, then FL 20.6's flags for an
        // automation clip, start/end offsets in beats
        let mut r = Vec::with_capacity(32);
        r.extend((s as u32).to_le_bytes());
        r.extend(BASE.to_le_bytes());
        r.extend(auto.to_le_bytes());
        r.extend((len as u32).to_le_bytes());
        r.extend(track.to_le_bytes());
        r.extend([0, 0, 0x78, 0, 0, 0, 0x40, 0x64, 0x80, 0x80]);
        r.extend(0f32.to_le_bytes());
        r.extend(((len / ppq) as f32).to_le_bytes());
        let ev = nth_event(out, 233, arr);
        out[ev].1.extend(r);
        placed += 1;
    }
    // FL 20 keeps playlist records sorted by position (all 34 demo/sample playlists); appended
    // out of order, FL 20.6 only drew them after scrolling (user-tested)
    for e in out.iter_mut().filter(|e| e.0 == 233) {
        let mut recs: Vec<&[u8]> = e.1.chunks_exact(32).collect();
        recs.sort_by_key(|r| u32::from_le_bytes(r[0..4].try_into().unwrap()));
        e.1 = recs.concat();
    }
    h.channels = next;
    out.splice(end..end, new_blocks);
    let first = out.iter().position(|e| e.0 == 64).unwrap();
    out.splice(first..first, links);
    warnings.push(format!(
        "per-clip fades: {placed} faded clips, {} volume automation clips, {lanes_added} crossfade channel copies",
        autos.len()
    ));
    if skipped > 0 {
        warnings.push(format!("per-clip fades: {skipped} clips skipped, their channel volume is already automated"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_vol_round_trips() {
        for (raw, db) in LEVELS_VOL {
            assert_eq!(levels_raw(db), raw);
            assert!((levels_db(raw) - db).abs() < 1e-3);
        }
        assert_eq!(levels_raw(100.0), 25600); // capped at the knob's max
        assert!((levels_db(levels_raw(-30.0)) + 30.0).abs() < 0.01);
        assert_eq!(levels_raw(-3.0), (9575 + 12800) / 2 + 1);
    }

    #[test]
    fn mixer_addr_matches_fl20() {
        // pairs taken from SolidInc FL2026 vs FL20.9.1 (events 216 / 225)
        for (new, old) in [
            ([0x09, 0x80, 0x80, 0x71], [0x09, 0x80, 0x80, 0x21]),
            ([0x66, 0x20, 0x00, 0x73], [0xa6, 0x1f, 0x00, 0x23]),
            ([0x6e, 0x80, 0x06, 0x00], [0x02, 0x00, 0x06, 0x00]),
            ([0x00, 0x00, 0x00, 0x40], [0x00, 0x00, 0x00, 0x40]),
            ([0x00, 0x1f, 0x49, 0xed], [0x00, 0x1f, 0x89, 0x3f]), // Current: 501 -> 126
        ] {
            let mut a = new;
            assert!(addr_to_fl20(&mut a));
            assert_eq!(a, old);
        }
        assert!(!addr_to_fl20(&mut [0, 0x1f, 0x00, 0x90])); // track 128
        // FL 12: Current 126 -> 104, track 104+ and routes to it dropped, other ids kept
        let mut a = [0x00, 0x1f, 0x89, 0x3f];
        assert!(addr_to_fl12(&mut a));
        assert_eq!(a, [0x00, 0x1f, 0x09, 0x3a]);
        assert!(!addr_to_fl12(&mut [0, 0x1f, 0x00, 0x3a])); // track 104
        assert!(!addr_to_fl12(&mut [0xa8, 0x1f, 0x00, 0x20])); // route to track 104
        assert!(addr_to_fl12(&mut [0xc0, 0x1f, 0x00, 0x20])); // id 192: not a route
    }
}
