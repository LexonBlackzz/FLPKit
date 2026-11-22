//! FL 12.3 -> FL 11.1 (ported from the Python prototype; rules and evidence in FORMAT.md "Downgrade FL 12.3 → FL 11.1").
//! Rules from newstuff / SolidInc saved by FL 11.1 and FL 12.3, FL 11's demos and the user's FL 11 saves.
//! FL 11 keeps FL 12's 24-byte notes, 32-byte playlist items and 105 mixer tracks; like FL 8 it has 8-bit text,
//! 8 effect slots, steps as 91 events and FL 8's mixer track layout.
use std::collections::{HashMap, HashSet};

pub(crate) type Ev = Vec<(u8, Vec<u8>)>;

const DROP: [u8; 3] = [32, 97, 98]; // FL 12 only
/// UTF-16 in FL 12, 8-bit in FL 11
const TEXT: [u8; 16] = [193, 194, 195, 196, 197, 200, 201, 202, 203, 204, 206, 207, 230, 231, 239, 241];
/// no FL 11 counterpart (Wave Candy v9: no rule)
const MISSING: [&str; 2] = ["Transient Processor", "Wave Candy"];

pub(crate) fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
pub(crate) fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
pub(crate) fn i32_at(d: &[u8], o: usize) -> i32 {
    u32_at(d, o) as i32
}
pub(crate) fn put_u32(d: &mut [u8], o: usize, v: u32) {
    d[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
pub(crate) fn trim0(d: &[u8]) -> &[u8] {
    let n = d.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    &d[..n]
}

#[derive(Default)]
struct Ctx {
    /// (track, FL 12 slot) -> FL 11 slot
    slot: HashMap<(u32, u32), u32>,
    /// (track, slot) of Fruity Delay 3s, which become Fruity Delay 2s
    d3: HashSet<(u32, u32)>,
    warnings: Vec<String>,
}

/// UTF-16 text -> 8-bit (characters past U+00FF become '?'), zero-terminated
pub(crate) fn text(d: &[u8]) -> Vec<u8> {
    let u: Vec<u16> = d.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let s = String::from_utf16_lossy(&u);
    let mut out: Vec<u8> = s.trim_end_matches('\0').chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect();
    out.push(0);
    out
}

impl Ctx {
    /// FL 12 slots 8/9 -> the track's free slots below 8 (none free: the effect is dropped); Delay 3 positions
    fn slots(&mut self, ev: &Ev) {
        let mut used: Vec<(u32, Vec<u32>)> = Vec::new();
        let mut name = String::new();
        for (i, d) in ev {
            if *i == 201 {
                name = crate::text(d);
            }
            if *i == 212 && d.len() >= 8 {
                let (t, s) = (u32_at(d, 0), u32_at(d, 4));
                match used.iter_mut().find(|u| u.0 == t) {
                    Some(u) => u.1.push(s),
                    None => used.push((t, vec![s])),
                }
                if name == "Fruity Delay 3" {
                    self.d3.insert((t, s));
                }
            }
        }
        for (t, ss) in &used {
            let mut free = (0..8).filter(|s| !ss.contains(s));
            for &s in ss {
                if s >= 8 {
                    if let Some(n) = free.next() {
                        self.slot.insert((*t, s), n);
                    }
                }
            }
        }
    }

    /// Automation / link address (param, target) FL 12 -> FL 11, or None. Mixer targets 0x2000 + track << 6 + slot:
    /// route-level params 0x1f00 + 64 + track kept, plugin params flagged 0x8000 written bare on mixer effects,
    /// 0x2000 + param on channel generators; slots remapped; Delay 3 knobs: wet (23) -> Delay 2 input (1)
    fn addr(&self, mut p: u16, mut tgt: u16) -> Option<(u16, u16)> {
        if (0x2000..0x4000).contains(&tgt) {
            let t = (tgt >> 6) - 128;
            if t > 104 {
                return None;
            }
            if p >> 8 == 31 && (64..=168).contains(&(p & 255)) {
                if (p & 255) - 64 > 104 {
                    return None;
                }
            } else if p & 0x8000 != 0 {
                p &= 0x7fff;
            }
            let (t, s) = (t as u32, (tgt & 63) as u32);
            if !self.slot.contains_key(&(t, s)) && s >= 8 {
                return None;
            }
            if self.d3.contains(&(t, s)) && p >> 8 != 31 {
                if p != 23 {
                    return None;
                }
                p = 1;
            }
            tgt = 0x2000 + ((t as u16) << 6) + *self.slot.get(&(t, s)).unwrap_or(&s) as u16;
        } else if tgt < 0x2000 && p & 0x8000 != 0 {
            p = 0x2000 | (p & 0x7fff);
        }
        Some((p, tgt))
    }

    /// 216 (12-byte records, address at 4) / 226, 227 (20-byte, address at 8)
    fn links(&self, d: &[u8], size: usize, at: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for r in d.chunks_exact(size) {
            if let Some((p, t)) = self.addr(u16_at(r, at), u16_at(r, at + 2)) {
                let mut r = r.to_vec();
                r[at..at + 2].copy_from_slice(&p.to_le_bytes());
                r[at + 2..at + 4].copy_from_slice(&t.to_le_bytes());
                out.extend(r);
            }
        }
        out
    }

    /// 225 records [u32 0][u8 param][u8 31][u16 0x2000 + track << 6 + slot][i32]: unused send levels (164..168 = 0)
    /// omitted as FL 11 does; slot records (params 0/1) for slots 8/9 remapped or dropped
    fn m225(&self, d: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for r in d.chunks_exact(12) {
            let mut r = r.to_vec();
            if r[5] == 31 && (164..=168).contains(&r[4]) && r[8..12] == [0; 4] {
                continue;
            }
            if r[5] == 31 && r[4] < 2 {
                let a = u16_at(&r, 6);
                let (t, s) = (((a >> 6) - 128) as u32, (a & 63) as u32);
                if s >= 8 {
                    let Some(&n) = self.slot.get(&(t, s)) else { continue };
                    r[6..8].copy_from_slice(&(0x2000 + ((t as u16) << 6) + n as u16).to_le_bytes());
                }
            }
            out.extend(r);
        }
        out
    }

    /// FL 12: [149] [95] [204] 236(8) [98]* [201 212 [203] 155 128 213]* 235 154 147; FL 11: [149] [95] [204] 27
    /// 236(5) [201 212 [203] 213]* 235 154 147, 149 only on colored tracks (FL 12 color - 0x24 per channel, FL 12's
    /// default 78 81 86 = none). 235 bytes 100..103 = 1 on tracks 1..99 (inserts route to the sends).
    fn mixer(&mut self, ev: Ev) -> Ev {
        let Some(first) = ev.iter().position(|e| e.0 == 149 || e.0 == 236) else { return ev };
        let mut out: Ev = ev[..first].to_vec();
        let (mut k, mut t) = (first, -1i32);
        while k < ev.len() && [149, 95, 204, 236].contains(&ev[k].0) {
            let Some(j) = (k..ev.len()).find(|&j| ev[j].0 == 147) else { break };
            t += 1;
            let blk = &ev[k..=j];
            k = j + 1;
            if t > 104 {
                continue;
            }
            let get = |n: u8| blk.iter().find(|e| e.0 == n).map(|e| &e.1);
            let flags = get(236).and_then(|d| d.get(4).copied()).unwrap_or(0x2c);
            let mut head: Ev = Vec::new();
            if let Some(c) = get(149) {
                if c.len() >= 3 && c[..3] != [0x78, 0x81, 0x86] {
                    let mut c2: Vec<u8> = c[..3].iter().map(|&x| x.saturating_sub(0x24)).collect();
                    c2.extend(&c[3..]);
                    head.push((149, c2));
                }
            }
            head.extend(blk.iter().filter(|e| e.0 == 95 || e.0 == 204).cloned());
            head.push((27, vec![flags & 0x1f]));
            head.push((236, vec![0, 0, 0, 0, 1]));
            let (mut body, mut drop, mut prev): (Ev, bool, Option<u8>) = (Vec::new(), false, None);
            let tu = t as u32;
            for (i, d) in blk {
                let (i, mut d) = (*i, d.clone());
                let p = prev.replace(i);
                if [149, 95, 204, 236, 98, 155].contains(&i) || (i == 128 && p == Some(155)) {
                    continue;
                }
                if i == 201 && trim0(&d) == b"Fruity Delay 3" {
                    d = b"Fruity Delay 2\0".to_vec();
                }
                if i == 201 {
                    let name = String::from_utf8_lossy(trim0(&d)).into_owned();
                    drop = MISSING.contains(&name.as_str());
                    if drop {
                        self.warnings.push(format!("FL 11 has no {name} (mixer track {t}): dropped"));
                    }
                }
                if i == 212 && d.len() >= 8 {
                    let s = u32_at(&d, 4);
                    if s >= 8 && !self.slot.contains_key(&(tu, s)) {
                        self.warnings.push(format!("FL 11 mixer track {t} has no free slot: effect dropped"));
                        if body.last().is_some_and(|e| e.0 == 201) {
                            body.pop();
                        }
                        drop = true;
                    }
                    if !drop {
                        put_u32(&mut d, 0, tu);
                        put_u32(&mut d, 4, *self.slot.get(&(tu, s)).unwrap_or(&s));
                    }
                }
                if drop && [201, 212, 203, 213].contains(&i) {
                    if i == 213 {
                        drop = false;
                    }
                    continue;
                }
                if i == 235 && (1..=99).contains(&t) && d.len() >= 104 {
                    d[100..104].fill(1);
                }
                if i == 213 && body.len() >= 2 && body[body.len() - 2].0 == 201 && body[body.len() - 2].1.starts_with(b"Fruity Wrapper") {
                    // FL 11 names wrapped effects (203 = the VST's name record 54)
                    if let Some((_, r)) = records(&d).into_iter().find(|r| r.0 == 54) {
                        let mut n = trim0(&r).to_vec();
                        n.push(0);
                        body.push((203, n));
                    }
                }
                body.push((i, d));
            }
            out.extend(head);
            out.extend(body);
        }
        out.extend(ev[k..].iter().cloned());
        out
    }
}

/// FL 12: 64 21 201 212 203 155 128 [213] ... 143 20 [196]; FL 11: 64 21 [201 212 213] ... 143 20 [196] 192 155 128
/// (samplers have no empty 201/212)
fn channels(ev: Ev) -> Ev {
    let mut out = Vec::with_capacity(ev.len());
    let mut k = 0;
    while k < ev.len() {
        if ev[k].0 != 64 {
            out.push(ev[k].clone());
            k += 1;
            continue;
        }
        let j = (k + 1..ev.len()).find(|&j| [64, 65, 233, 149, 236, 226, 227].contains(&ev[j].0)).unwrap_or(ev.len());
        let mut blk: Ev = Vec::new();
        let mut tail: Vec<(u8, Vec<u8>)> = Vec::new();
        let mut set_tail = |id: u8, d: &Vec<u8>| match tail.iter_mut().find(|e| e.0 == id) {
            Some(e) => e.1 = d.clone(),
            None => tail.push((id, d.clone())),
        };
        for (i, d) in &ev[k..j] {
            let after155 = *i == 128 && blk.last().is_some_and(|e| e.0 == 155);
            if *i == 203 || *i == 155 || after155 {
                set_tail(if *i == 203 { 192 } else { *i }, d);
            }
            if *i == 155 {
                blk.push((155, d.clone()));
                continue;
            }
            if after155 {
                blk.pop();
                continue;
            }
            if *i == 203 {
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
        out.extend(blk);
        for id in [192, 155, 128] {
            if let Some(e) = tail.iter().find(|e| e.0 == id) {
                out.push(e.clone());
            }
        }
        k = j;
    }
    out
}

/// `[u32 id][u64 size][data]` records after a 4-byte kind; a repeated id replaces the earlier one in place
pub(crate) fn records(b: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let (mut o, mut r): (usize, Vec<(u32, Vec<u8>)>) = (4, Vec::new());
    while o + 12 <= b.len() {
        let t = u32_at(b, o);
        let n = u64::from_le_bytes(b[o + 4..o + 12].try_into().unwrap()) as usize;
        let v = b[o + 12..(o + 12 + n).min(b.len())].to_vec();
        match r.iter_mut().find(|e| e.0 == t) {
            Some(e) => e.1 = v,
            None => r.push((t, v)),
        }
        o += 12 + n;
    }
    r
}

/// Fruity Wrapper kind 8 as FL 11 writes it: no 56 (vendor) / 57; record 2's u32 at 9 without FL 2026's top byte;
/// VST2 data header [f7ffffff][u8 flags][i32 -2][u32 size][8 bytes] -> [f7ffffff][u8 flags | 1][u32 size, 0 for
/// params][u32 program]; a VST2's name (54) = its DLL's file name
fn wrapper(d: &[u8]) -> Vec<u8> {
    if d.len() < 4 || d[..4] != [8, 0, 0, 0] {
        return d.to_vec();
    }
    let mut r = records(d);
    r.retain(|e| e.0 != 56 && e.0 != 57);
    if let Some(e) = r.iter_mut().find(|e| e.0 == 2 && e.1.len() >= 13) {
        e.1[12] = 0;
    }
    if let Some(e) = r.iter_mut().find(|e| e.0 == 53) {
        let c = &e.1;
        if c.len() >= 21 && c[..4] == [0xf7, 0xff, 0xff, 0xff] && c[5..9] == [0xfe, 0xff, 0xff, 0xff] {
            let size = if c[4] & 8 != 0 { u32_at(c, 9) } else { 0 };
            let mut n = c[..4].to_vec();
            n.push(c[4] | 1);
            n.extend(size.to_le_bytes());
            n.extend(u32_at(c, 17).to_le_bytes());
            n.extend(&c[21..]);
            e.1 = n;
        }
    }
    let path: String = r.iter().find(|e| e.0 == 55).map(|e| trim0(&e.1).iter().map(|&b| b as char).collect()).unwrap_or_default();
    if path.to_lowercase().ends_with(".dll") {
        let file = path.replace('/', "\\");
        let file = file.rsplit('\\').next().unwrap();
        let stem: Vec<u8> = file.chars().take(file.chars().count() - 4).map(|c| c as u8).collect();
        match r.iter_mut().find(|e| e.0 == 54) {
            Some(e) => e.1 = stem,
            None => r.push((54, stem)),
        }
    }
    let mut out = d[..4].to_vec();
    for (t, v) in &r {
        out.extend(t.to_le_bytes());
        out.extend((v.len() as u64).to_le_bytes());
        out.extend(v);
    }
    out
}

/// Fruity Delay 3 state [u32 1][26 i32, knobs 0..6000] -> Fruity Delay 2 [8 i32: input pan, input vol, dry, feedback,
/// time, stereo offset, feedback mode (0 normal, 2 ping pong), cutoff], knobs 0..128. Delay 3 fields read off FL
/// 2026's 10 presets (index incl. the version): 2 stereo mode, 3 tempo sync, 5 time (1/192 beat = Delay 2's unit),
/// 15 feedback, 17 cutoff, 23 dry, 24 wet. ponytail: linear knob scaling; filter type, modulation, diffusion,
/// saturation dropped
fn delay3(d: &[u8], warnings: &mut Vec<String>) -> Vec<u8> {
    let v: Vec<i32> = (0..27).map(|k| i32_at(d, 4 * k)).collect();
    let k = |x: i32| ((128.0 * x as f64 / 6000.0).round() as i32).clamp(0, 128);
    if v[3] == 0 {
        warnings.push(format!("Delay 3 time not tempo-synced, kept as Delay 2 ticks ({})", v[5]));
    }
    [0, k(v[24]), k(v[23]), k(v[15]), v[5], 0, if v[2] == 2 { 2 } else { 0 }, k(v[17])].iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// FL 12.3 native states -> FL 11.1 (versions from FL 11's demos and the SolidInc pair)
fn state(name: &str, d: &[u8], warnings: &mut Vec<String>) -> Vec<u8> {
    match name {
        // [u16 3][u16 v] -> [u32 v]
        "Maximus" | "Gross Beat" | "Slicex" if d.len() >= 4 && d[..2] == [3, 0] => {
            let mut o = (u16_at(d, 2) as u32).to_le_bytes().to_vec();
            o.extend(&d[4..]);
            o
        }
        // v3 appends a u32
        "Fruity Parametric EQ 2" if d.len() >= 5 && d[0] == 3 => [&[2u8][..], &d[1..d.len() - 4]].concat(),
        // v12 appends a byte (demo pairs)
        "Harmless" if d.len() >= 2 && d[0] == 12 => [&[11u8][..], &d[1..d.len() - 1]].concat(),
        "Fruity Delay 2" if d.len() == 108 && d[..4] == [1, 0, 0, 0] => delay3(d, warnings),
        "Sytrus" if d.len() >= 4 && d[..4] == [3, 0, 0x17, 0] => {
            warnings.push("Sytrus v23 state kept (FL 11 has v22, no rule yet)".into());
            d.to_vec()
        }
        _ => d.to_vec(),
    }
}

fn plugins(ev: &mut Ev, warnings: &mut Vec<String>) {
    let mut name = String::new();
    for (i, d) in ev.iter_mut() {
        if *i == 201 {
            name = String::from_utf8_lossy(trim0(d)).into_owned();
        }
        if *i == 213 && !name.is_empty() {
            *d = if name == "Fruity Wrapper" { wrapper(d) } else { state(&name, d, warnings) };
            name.clear();
        }
    }
}

/// FL 12+ saves step-sequencer steps as length-0 notes (224); FL 8 / 11 keep them as 91 [step][1] after the
/// channel's block, under a 65 (pattern) (FL 8's Moholithic: 65 1, 91 00 01 / 04 01 / 08 01 / 0c 01 after the kick).
/// `size`: note record size; `vel`: its velocity offset, for FL 11's 72 [step][velocity] before each off-default
/// (not 100) velocity
pub(crate) fn steps(ev: Ev, ppq: u32, size: usize, vel: Option<usize>) -> Ev {
    let q = ppq / 4;
    let mut per: Vec<(u16, Vec<(u16, Vec<(u8, u8)>)>)> = Vec::new();
    let (mut out, mut pat): (Ev, Option<u16>) = (Vec::new(), None);
    for (i, d) in ev {
        if i == 65 {
            pat = Some(u16_at(&d, 0));
        }
        if let (224, Some(p)) = (i, pat) {
            let mut keep = Vec::new();
            for r in d.chunks(size) {
                let (pos, ch, ln) = (u32_at(r, 0), u16_at(r, 6), u32_at(r, 8));
                if ln == 0 && pos % q == 0 && pos / q < 256 {
                    let c = match per.iter().position(|e| e.0 == ch) {
                        Some(n) => n,
                        None => {
                            per.push((ch, Vec::new()));
                            per.len() - 1
                        }
                    };
                    let pats = &mut per[c].1;
                    let pi = match pats.iter().position(|e| e.0 == p) {
                        Some(n) => n,
                        None => {
                            pats.push((p, Vec::new()));
                            pats.len() - 1
                        }
                    };
                    pats[pi].1.push(((pos / q) as u8, vel.map_or(100, |v| r[v])));
                } else {
                    keep.extend(r);
                }
            }
            if keep.is_empty() {
                if out.last().is_some_and(|e| e.0 == 65) {
                    out.pop(); // pattern left with no notes
                }
                continue;
            }
            out.push((i, keep));
            continue;
        }
        out.push((i, d));
    }
    if per.is_empty() {
        return out;
    }
    let last = out.iter().rposition(|e| e.0 == 64).unwrap_or(0);
    let end = (last..out.len()).find(|&n| [152, 233, 149].contains(&out[n].0)).unwrap_or(out.len());
    let (mut res, mut ch): (Ev, Option<u16>) = (Vec::with_capacity(out.len()), None);
    for (n, (i, d)) in out.into_iter().enumerate() {
        if i == 64 || n == end {
            if let Some(e) = ch.and_then(|c| per.iter_mut().find(|e| e.0 == c)) {
                e.1.sort_by_key(|p| p.0);
                for (p, st) in &mut e.1 {
                    res.push((65, p.to_le_bytes().to_vec()));
                    st.sort();
                    for &(s, v) in st.iter() {
                        if v != 100 {
                            res.push((72, vec![s, v]));
                        }
                        res.push((91, vec![s, 1]));
                    }
                }
                ch = None;
            }
        }
        if i == 64 {
            ch = Some(u16_at(&d, 0));
        }
        res.push((i, d));
    }
    res
}

/// FL 11 ignores the channel pitch knob (219 i32 at 8, cents) on a stretched sample (215 stretch mode at 108 set)
/// and only applies the stretch pitch (215 i32 at 100, cents): fold the knob into it
fn pitch(ev: &mut Ev) {
    let mut lv: Option<usize> = None;
    for n in 0..ev.len() {
        match ev[n].0 {
            64 => lv = None,
            219 => lv = Some(n),
            215 => {
                let Some(l) = lv else { continue };
                if ev[n].1.len() < 112 || ev[l].1.len() < 12 || i32_at(&ev[n].1, 108) == 0 {
                    continue;
                }
                let k = i32_at(&ev[l].1, 8);
                if k != 0 {
                    let v = i32_at(&ev[n].1, 100) + k;
                    put_u32(&mut ev[n].1, 100, v as u32);
                    put_u32(&mut ev[l].1, 8, 0);
                }
            }
            _ => {}
        }
    }
}

/// Converts an FL 12.3 project (an FL 12.3 save, or `downgrade` output for 12.3) to FL 11.1.
pub fn from_12_3(b: &[u8]) -> Result<(Vec<u8>, Vec<String>), String> {
    let (h, events) = crate::parse(b)?;
    let ev: Ev = events.iter().map(|e| (e.id, e.data.to_vec())).collect();
    let mut c = Ctx::default();
    c.slots(&ev);
    let mut out: Ev = Vec::with_capacity(ev.len());
    for (mut i, mut d) in ev {
        if DROP.contains(&i) {
            continue;
        }
        if i == 199 {
            d = b"11.1.1\0".to_vec();
        }
        if TEXT.contains(&i) {
            d = text(&d);
        }
        if i == 195 {
            i = 198;
        }
        match i {
            // FL 12 appends 13 bytes; i32 at 108 = stretch mode: FL 12's Elastique Pro modes (< 0) fall back to
            // resample in FL 11 -> E3 generic (1)
            215 if d.len() >= 112 => {
                d.truncate(112);
                if i32_at(&d, 108) < 0 {
                    put_u32(&mut d, 108, 1);
                }
            }
            // FL 12 appends 2 bytes; byte 17 f0 in every FL 11 save
            238 if d.len() >= 47 => {
                d.truncate(47);
                if d[17] == 0xff {
                    d[17] = 0xf0;
                }
            }
            225 => d = c.m225(&d),
            216 => d = c.links(&d, 12, 4),
            226 | 227 => d = c.links(&d, 20, 8),
            _ => {}
        }
        out.push((i, d));
    }
    let mut out = c.mixer(channels(out));
    plugins(&mut out, &mut c.warnings);
    let mut out = steps(out, h.ppq as u32, 24, Some(21));
    pitch(&mut out);
    Ok((crate::write(h, &out), c.warnings))
}
