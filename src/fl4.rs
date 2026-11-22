//! FL 5.0 -> FL 4.5.2 (ported from the Python prototype; rules and evidence in FORMAT.md "Downgrade FL 5.0 → FL 4.5").
//! Rules from the 3 demos FL 4 ships as 4.5.2 saves and FL 5 as 5.0.0 saves (Fuego-FL303, Fuego-Momentum, NewStuff),
//! both versions' other demos, and the user's FL 4.5.2 saves.
use crate::fl11::{i32_at, put_u32, u16_at, u32_at, Ev};
use crate::fl6::{f32_at, name, read_wav, round};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// NewStuff's X-Y Controller window info (u32 0 = mixer track, u32 4 = slot) and Default.fst state ([1][X][min][max])
const XY_212: &str = "0d0000000100000002000000000000002000000088f96400011f00208554390100004003040000007c000000b90000001b010000";
const XY_213: &str =
    "0100000000800000000000000000010000000000008000000000000000000100000000000006000000040000000744656661756c74000000000000000000010000000000000046000000";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|k| u8::from_str_radix(&s[k..k + 2], 16).unwrap()).collect()
}

/// automation clip point: (Δbeat, value, tension, mode)
type Pt = (f64, f64, f64, u32);

/// clip value at beat `b`: segment shape from its end point's tension
fn curve(pts: &[Pt], b: f64) -> f64 {
    let mut x = 0.0;
    for (j, &(dx, v, t, _)) in pts.iter().enumerate() {
        let x0 = x;
        x += dx;
        if b < x && j > 0 {
            let v0 = pts[j - 1].1;
            let f = if x > x0 { ((b - x0) / (x - x0)).max(0.0) } else { 1.0 };
            let a = -t * 1e6f64.ln();
            let g = if a.abs() < 1e-9 { f } else { (a * f).exp_m1() / a.exp_m1() };
            return v0 + (v - v0) * g;
        }
    }
    if b >= x { pts[pts.len() - 1].1 } else { pts[0].1 }
}

struct Clip {
    name: Vec<u8>,
    lo: i32,
    hi: i32,
    pts: Vec<Pt>,
}

/// FL 5 automation clips -> FL 4.5 (no clips there): each linked clip becomes a Fruity X-Y Controller in a free mixer
/// slot whose X knob is automated by pattern automation (223); the clip's 227 links take the X-Y output as their
/// source. Each clip placement becomes one pattern, placed where the clip was.
/// ponytail: sampled every 6 ticks (1/16 beat at PPQ 96) + at points; clip LFOs and unlinked clips ignored
fn clips(ev: Ev, warnings: &mut Vec<String>) -> Ev {
    const STEP: i64 = 6;
    let (mut ch, mut clips, mut mix, mut track) = (None, BTreeMap::<u16, Clip>::new(), false, 0u32);
    let (mut has_fx, mut routed) = (HashSet::new(), HashSet::new());
    for (i, d) in &ev {
        mix |= *i == 233;
        if !mix {
            if *i == 64 {
                ch = Some(u16_at(d, 0));
            }
            let Some(c) = ch else { continue };
            if *i == 21 && d[0] == 5 {
                clips.insert(c, Clip { name: b"Automation".to_vec(), lo: 0, hi: 128, pts: Vec::new() });
            }
            let Some(cl) = clips.get_mut(&c) else { continue };
            match i {
                192 => cl.name = d.clone(),
                219 => (cl.lo, cl.hi) = (i32_at(d, 0), i32_at(d, 4)),
                234 => {
                    cl.pts = (0..u32_at(d, 17) as usize)
                        .map(|k| {
                            let o = 21 + 16 * k;
                            (f32_at(d, o) as f64, f32_at(d, o + 4) as f64, f32_at(d, o + 8) as f64, u32_at(d, o + 12))
                        })
                        .collect()
                }
                22 => {
                    routed.insert(d[0] as u32);
                }
                _ => {}
            }
        } else {
            if *i == 201 {
                has_fx.insert(track);
            }
            if *i == 147 {
                let t = u32_at(d, 0);
                if (1..65).contains(&t) {
                    routed.insert(t);
                }
                track += 1;
            }
        }
    }
    let linked: Vec<u16> = ev
        .iter()
        .filter(|(i, d)| *i == 227 && d[1] == 0)
        .map(|(_, d)| u16_at(d, 2))
        .filter(|c| clips.get(c).is_some_and(|cl| !cl.pts.is_empty()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if linked.is_empty() {
        return ev;
    }
    let free: Vec<u32> = (1..=64).rev().filter(|t| !has_fx.contains(t) && !routed.contains(t)).collect();
    let mut slots = HashMap::new();
    for (k, &c) in linked.iter().enumerate() {
        match free.get(k / 8) {
            Some(&t) => {
                slots.insert(c, (t, (k % 8) as u32));
            }
            None => warnings.push("not enough free mixer tracks for every automation clip's X-Y Controller".into()),
        }
    }
    let linked: Vec<u16> = linked.into_iter().filter(|c| slots.contains_key(c)).collect();
    let addr = |c: u16| (0x2000 + (slots[&c].0 << 6) + slots[&c].1) as u16;
    let xval = |c: u16, v: f64| {
        let cl = &clips[&c];
        round(65536.0 * (cl.lo as f64 + v * (cl.hi - cl.lo) as f64) / 128.0).clamp(0, 65536) as i32
    };

    let mut nxt = ev.iter().filter(|e| e.0 == 65).map(|e| u16_at(&e.1, 0)).max().unwrap_or(0) + 1;
    let (mut placements, mut newpats, mut items) = (HashMap::new(), Vec::<(u16, u16, Vec<u8>)>::new(), Vec::new());
    for (i, d) in &ev {
        if *i != 233 {
            continue;
        }
        for r in d.chunks_exact(28) {
            let (pos, base, c, ln) = (u32_at(r, 0), u16_at(r, 4), u16_at(r, 6), u32_at(r, 8) as i64);
            if base != 0x5000 || !slots.contains_key(&c) {
                continue;
            }
            let s = (f32_at(r, 20) as f64).max(0.0);
            let key = (c, round(s * 1e4), ln);
            let p = *placements.entry(key).or_insert_with(|| {
                let pts = &clips[&c].pts;
                let mut xs: BTreeSet<i64> = (0..ln).step_by(STEP as usize).collect();
                let mut x = 0.0;
                for p in pts {
                    x += p.0;
                    if 0.0 <= (x - s) * 96.0 && (x - s) * 96.0 < ln as f64 {
                        xs.insert(round((x - s) * 96.0));
                    }
                }
                let (mut recs, mut last) = (Vec::new(), None);
                let rec = |t: i64, v: i32| [&(t as u32).to_le_bytes()[..], &0u16.to_le_bytes(), &addr(c).to_le_bytes(), &v.to_le_bytes()].concat();
                for t in xs {
                    let v = xval(c, curve(pts, s + t as f64 / 96.0));
                    if Some(v) != last {
                        recs.extend(rec(t, v));
                        last = Some(v);
                    }
                }
                recs.extend(rec(ln - 1, last.unwrap_or(0))); // pattern length = clip length
                newpats.push((nxt, c, recs));
                nxt += 1;
                nxt - 1
            });
            let mut it = pos.to_le_bytes().to_vec();
            it.extend(0u16.to_le_bytes());
            it.extend(0x5000u16.to_le_bytes());
            it.extend((ln as u32).to_le_bytes());
            it.extend(0x78E7u16.wrapping_sub(p).to_le_bytes());
            it.extend(3u16.to_le_bytes());
            it.extend(0x80806440u32.to_le_bytes());
            it.extend((-1i32).to_le_bytes());
            it.extend((-1i32).to_le_bytes());
            items.extend(it);
        }
    }
    let mut first: HashMap<u16, i32> = HashMap::new();
    for &c in &linked {
        if !newpats.iter().any(|p| p.1 == c) {
            first.insert(c, xval(c, clips[&c].pts[0].1));
        }
    }
    for (_, c, r) in &newpats {
        first.entry(*c).or_insert(i32_at(r, 8));
    }

    let ids: Vec<u8> = ev.iter().map(|e| e.0).collect();
    let pat_end = [226, 227, 64].iter().filter_map(|k| ids.iter().position(|i| i == k)).min();
    let name_at = ids.iter().position(|&i| i == 233).unwrap_or(ids.len());
    let name_at = (0..name_at).rev().find(|&j| ids[j] == 151 || j == 0).map(|j| j + 1);
    let color = ev.iter().find(|e| e.0 == 150).map_or(13156792u32.to_le_bytes().to_vec(), |e| e.1.clone());
    let xy212 = hex(XY_212);
    let (mut out, mut track, mut mix): (Ev, u32, bool) = (Vec::new(), 0, false);
    for (j, (i, mut d)) in ev.into_iter().enumerate() {
        if Some(j) == pat_end {
            for (p, _, r) in &newpats {
                out.push((65, p.to_le_bytes().to_vec()));
                out.push((223, r.clone()));
            }
        }
        if Some(j) == name_at {
            for (p, c, _) in &newpats {
                out.push((65, p.to_le_bytes().to_vec()));
                out.push((193, clips[c].name.clone()));
                out.push((150, color.clone()));
                out.push((151, vec![0; 4]));
            }
        }
        if i == 233 {
            mix = true;
            d.extend(&items);
            let mut recs: Vec<&[u8]> = d.chunks(28).collect();
            recs.sort_by_key(|r| u32_at(r, 0));
            d = recs.concat();
        }
        if i == 227 {
            let c = u16_at(&d, 2);
            if d[1] == 0 && slots.contains_key(&c) {
                let a = addr(c).to_le_bytes();
                d[0..8].copy_from_slice(&[0, 0, a[0], a[1], 0, 0, a[0], a[1]]);
            }
        }
        if mix && i == 149 {
            for &c in &linked {
                let (t, s) = slots[&c];
                if t == track {
                    let mut st = hex(XY_213);
                    put_u32(&mut st, 4, first[&c] as u32);
                    out.push((201, b"Fruity X-Y Controller\0".to_vec()));
                    out.push((212, [&t.to_le_bytes()[..], &s.to_le_bytes(), &xy212[8..]].concat()));
                    out.push((213, st));
                }
            }
        }
        if mix && i == 147 {
            track += 1;
        }
        out.push((i, d));
    }
    out
}

/// 233 channel items (u16 0x5000 at 4, channel at 6): automation clips (type 5) dropped; audio clips' trims at 20 / 24
/// go from FL 5's f32 ms (-1 = untrimmed) to FL 4's u32 start / end frames at 44.1 kHz
fn playlist(d: &[u8], types: &HashMap<u16, u8>) -> Vec<u8> {
    let mut out = Vec::new();
    for r in d.chunks(28) {
        let mut r = r.to_vec();
        if r.len() == 28 && u16_at(&r, 4) == 0x5000 {
            if types.get(&u16_at(&r, 6)) == Some(&5) {
                continue;
            }
            let (s, e) = (f32_at(&r, 20) as f64, f32_at(&r, 24) as f64);
            let (a, b) = if s < 0.0 { (0xffffffff, 0x7fffffff) } else { (round(s * 44.1) as u32, (round(e * 44.1) - 1).max(0) as u32) };
            put_u32(&mut r, 20, a);
            put_u32(&mut r, 24, b);
        }
        out.extend(r);
    }
    out
}

/// Fruity slicer v13 -> v6: header [u32 6][i32 bars][i32 bpm][i32 0][i32 0]; slices drop their i32 -1 / f32 -1; same
/// 13-byte trailer, last byte 0. ponytail: bars / bpm rounded; v13 pitch / stretch / mode dropped
fn slicer(d: &[u8]) -> Vec<u8> {
    if !d.starts_with(&[13, 0, 0, 0]) {
        return d.to_vec();
    }
    let (beats, bpm) = (f32_at(d, 4) as f64, f32_at(d, 8) as f64);
    let mut out: Vec<u8> = [6, round(beats / 4.0).max(1) as i32, round(bpm) as i32, 0, 0].iter().flat_map(|v: &i32| v.to_le_bytes()).collect();
    let mut o = 32;
    out.extend(&d[o..o + 1 + d[o] as usize]);
    o += 1 + d[o] as usize;
    let n = u32_at(d, o);
    out.extend(&d[o..o + 4]);
    o += 4;
    for _ in 0..n {
        let l = 1 + d[o] as usize;
        out.extend(&d[o..o + l]);
        o += l;
        out.extend(&d[o..o + 4]);
        out.push(0);
        o += 13;
    }
    out.extend(&d[o..o + 12]);
    out.push(0);
    out
}

/// FL 5 only (206 / 207 song genre / author)
const DROP: [u8; 3] = [206, 207, 234];

pub(crate) fn from_5(ev: Ev, warnings: &mut Vec<String>) -> Ev {
    let ev = clips(ev, warnings);
    let (mut types, mut rate, mut steps, mut ch) = (HashMap::new(), HashMap::new(), HashMap::new(), None);
    for (i, d) in &ev {
        if *i == 64 {
            ch = Some(u16_at(d, 0));
        }
        let Some(c) = ch else { continue };
        match i {
            21 => {
                types.insert(c, d[0]);
            }
            // FL 5 stretch time: 215 u32 at 96, 192 per beat; FL 4 stretches to steps (event 140).
            // ponytail: Mul / stretch pitch / mode dropped
            215 if d.len() >= 100 => {
                steps.insert(c, round(u32_at(d, 96) as f64 / 48.0) as u32);
            }
            196 if types.get(&c) == Some(&4) => {
                if let Ok(w) = read_wav(std::path::Path::new(&crate::fl6::latin1(&name(d)))) {
                    rate.insert(c, w.rate);
                }
            }
            _ => {}
        }
    }
    let (mut out, mut plugin, mut ch4): (Ev, Vec<u8>, Option<u16>) = (Vec::new(), Vec::new(), None);
    let mut has226 = ev.iter().any(|e| e.0 == 226);
    for (i, mut d) in ev {
        if DROP.contains(&i) {
            continue;
        }
        if !has226 && (i == 227 || i == 64) {
            // every FL 4 / 5 save has this record before its 227 list; FL 4 fails without
            out.push((226, hex("ff000000ff0000000400ff0f0000000080000000")));
            has226 = true;
        }
        match i {
            199 => d = b"4.5.2\0".to_vec(),
            // FL 4 has no automation clips (channel type 5): keep the channel as an empty sampler
            21 if d[0] == 5 => d = vec![0],
            233 => d = playlist(&d, &types),
            64 => plugin.clear(),
            201 => plugin = name(&d).to_ascii_lowercase(),
            _ => {}
        }
        if i == 226 || i == 227 {
            // FL 4 writes u32 128 at 16. ponytail: field not decoded, FL 4's constant
            for k in (0..d.len().saturating_sub(19)).step_by(20) {
                put_u32(&mut d, k + 16, 128);
            }
        }
        if i == 213 && plugin == b"sytrus" && d.starts_with(&[7, 0, 0, 0]) {
            match crate::sytrus::convert(&d, 7, 6, crate::sytrus::v7_to_v6) {
                Some(s) => d = s,
                None => warnings.push("a Sytrus patch didn't convert to FL 4's format".into()),
            }
        }
        if i == 213 && plugin == b"fruity slicer" {
            d = slicer(&d);
        }
        if i == 215 {
            d.truncate(96); // FL 4.5.2 saves 96 bytes
        }
        if i == 143 && ch4.is_some_and(|c| types.get(&c) == Some(&4)) {
            // FL 4 writes audio clips' sample rate here
            out.push((153, rate.get(&ch4.unwrap()).copied().unwrap_or(44100).to_le_bytes().to_vec()));
        }
        if i == 64 {
            ch4 = Some(u16_at(&d, 0));
        }
        if i == 143 {
            // channel flags: FL 4 only bit 0; FL 5's bit 3 is FL 4's event 19 right after
            let f = u32_at(&d, 0);
            out.push((143, (f & 1).to_le_bytes().to_vec()));
            if f & 8 != 0 {
                out.push((19, vec![1]));
            }
            continue;
        }
        if i == 135 && out.last().is_some_and(|e| e.0 == 196) {
            out.insert(out.len() - 1, (i, d)); // FL 4 writes 135 before the sample path
            continue;
        }
        if i == 22 && out.last().is_some_and(|e| e.0 == 70) {
            out.push((140, ch4.and_then(|c| steps.get(&c)).copied().unwrap_or(0).to_le_bytes().to_vec()));
        }
        out.push((i, d));
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn curve_shape() {
        let pts = [(0.0, 0.0, 0.0, 0), (4.0, 1.0, 0.0, 0), (4.0, 0.5, 0.5, 0)];
        assert!(super::curve(&pts, 2.0) == 0.5 && super::curve(&pts, 9.0) == 0.5 && super::curve(&pts, -1.0) == 0.0);
        let v = super::curve(&pts, 6.0);
        assert!(0.5 < v && v < 1.0);
    }
}
