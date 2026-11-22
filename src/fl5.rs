//! FL 6.0 -> FL 5.0 (ported from the Python prototype; rules and evidence in FORMAT.md "Downgrade FL 6.0 → FL 5.0").
//! Rules from FL 5's own 5.0.0 demos, the 5 demos FL 5 ships as 4.x saves and FL 6 as 6.0 saves, and the user's FL 5
//! saves of What I Found.
use crate::fl11::{i32_at, put_u32, u16_at, Ev};
use crate::fl6::{name, preset, round, soft_limiter};
use crate::fl8::image_line;

/// FL 6 only: never in FL 5.0 saves (236 insert start, 235 routing)
const DROP: [u8; 5] = [26, 95, 27, 235, 236];

/// FL 6 mixer param `p` of track `t` -> FL 5 param, or None (FL 5 has no per-track routing)
fn mixer_param(p: u8, t: i32) -> Option<u8> {
    match p {
        0 | 1 => Some(p),
        129..=132 if (1..=64).contains(&t) => Some(p - 65),
        192.. => Some(p - 96),
        _ => None,
    }
}

fn track(tgt: u16) -> i32 {
    (tgt >> 6) as i32 - 128
}

fn div100(d: &mut [u8], o: usize) {
    put_u32(d, o, round(i32_at(d, o) as f64 / 100.0) as i32 as u32);
}

/// FL 5 scales mix, send, volume and pan 0..128 / ±64; FL 6 x100
const SCALED: [u8; 7] = [1, 64, 65, 66, 67, 96, 97];

/// 226 / 227 links: 20-byte records, a mixer target [u8 param][u8 31][u16 128 + track << 6] at 8, renumbered like 225
fn links227(mut d: Vec<u8>) -> Vec<u8> {
    for k in (0..d.len().saturating_sub(19)).step_by(20) {
        d[k + 12..k + 16].fill(0); // FL 6 sets 4 / 8 here, FL 5 always 0
        if d[k + 9] == 31 {
            if let Some(q) = mixer_param(d[k + 8], track(u16_at(&d, k + 10))) {
                d[k + 8] = q;
            }
        }
    }
    d
}

/// 225: FL 6 params per track (0/1 slot on/mix, 64+j route level, 192.. track params) -> FL 5 (64..67 send levels,
/// track params at -96), sorted FL 5's way: per track its slots' 0/1 params, then track params by number
fn mixer225(d: &[u8]) -> Vec<u8> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    for r in d.chunks(12) {
        if r.len() < 12 || r[5] != 31 {
            continue;
        }
        let Some(q) = mixer_param(r[4], track(u16_at(r, 6))) else { continue };
        let mut r = r.to_vec();
        r[4] = q;
        if SCALED.contains(&q) {
            div100(&mut r, 8);
        }
        out.push(r);
    }
    out.sort_by_key(|r| (u16_at(r, 6) >> 6, r[4] > 1, u16_at(r, 6), r[4]));
    out.concat()
}

/// 216: current values of automated params, 12-byte records [u32 0][u16 param][u16 target][i32 value]; mixer params
/// renumbered and scaled like 225, channel volume / pan and main volume /100 too
fn values216(mut d: Vec<u8>) -> Vec<u8> {
    for k in (0..d.len().saturating_sub(11)).step_by(12) {
        let (p, tgt) = (u16_at(&d, k + 4), u16_at(&d, k + 6));
        let mut scale = false;
        if p >> 8 == 31 {
            if let Some(q) = mixer_param(p as u8, track(tgt)) {
                d[k + 4] = q;
                scale = SCALED.contains(&q);
            }
        } else if (tgt < 0x2000 && p <= 1) || (tgt == 0x4000 && p == 0) {
            scale = true;
        }
        if scale {
            div100(&mut d, k + 8);
        }
    }
    d
}

/// 209 / 219 / 229 fields 0 and 1 (pan, volume; automation channels' range): 0..128 in FL 5, x100 in FL 6
fn levels(mut d: Vec<u8>) -> Vec<u8> {
    for k in [0, 4] {
        if d.len() >= k + 4 {
            div100(&mut d, k);
        }
    }
    d
}

/// Delay Bank (FL 6) -> Fruity Delay 2 with the song's own first Delay 2 state, dry 0 (Delay Bank sits on a send).
/// ponytail: not a translation of Delay Bank's taps
fn delay2(ev: &Ev) -> Vec<u8> {
    let mut plugin = Vec::new();
    let mut st = [0i32, 128, 128, 117, 192, 0, 2, 0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
    for (i, d) in ev {
        if *i == 201 {
            plugin = name(d);
        }
        if *i == 213 && plugin == b"Fruity Delay 2" {
            st = d.clone();
            break;
        }
    }
    if st.len() >= 32 {
        put_u32(&mut st, 28, 0);
    }
    st
}

/// Multiband Compressor (FL 6) -> Fruity Compressor: FL 5's "FastMaster" preset where it stands in for Maximus, else
/// "Light". ponytail: presets, not a translation of the bands
fn compressor(d: &[u8]) -> Result<Vec<u8>, String> {
    let soft = soft_limiter().ok();
    let f = if soft.as_deref() == Some(d) { "FastMaster.fst" } else { "Light.fst" };
    preset(&image_line().join(r"FLStudio5\Data\Patches\Plugin presets\Effects\Fruity Compressor").join(f))
}

fn subst(p: &[u8]) -> Option<&'static [u8]> {
    match p {
        b"Fruity Multiband Compressor" => Some(b"Fruity Compressor"),
        b"Fruity Delay Bank" => Some(b"Fruity Delay 2"),
        _ => None,
    }
}

/// FL 6 events -> FL 5. FL 6 track: 149 204 27 236 [201 212 213]* 235 154 147; FL 5: [201 212 213]* 149 204 154 147
pub(crate) fn from_6(ev: Ev, warnings: &mut Vec<String>) -> Ev {
    let first = ev.iter().position(|e| e.0 == 149).unwrap_or(ev.len());
    let mut out: Ev = ev[..first]
        .iter()
        .filter(|e| !DROP.contains(&e.0))
        .map(|(i, d)| {
            let d = d.clone();
            let d = match i {
                199 => b"5.0.0\0".to_vec(),
                226 | 227 => links227(d),
                209 | 219 | 229 => levels(d),
                216 => values216(d),
                _ => d,
            };
            (*i, d)
        })
        .collect();
    // main volume: FL 5 always writes it (128 in its demos), FL 6 never
    if !out.iter().any(|e| e.0 == 12) {
        if let Some(k) = out.iter().position(|e| e.0 == 11).or_else(|| out.iter().position(|e| e.0 == 199)) {
            out.insert(k + 1, (12, vec![128]));
        }
    }
    let (mut head, mut slots, mut t): (Ev, Ev, u32) = (Vec::new(), Vec::new(), 0);
    let (mut plugin, mut route): (Vec<u8>, Option<Vec<u8>>) = (Vec::new(), None);
    for (i, d) in &ev[first..] {
        let (i, mut d) = (*i, d.clone());
        if i == 235 {
            route = Some(d.clone()); // FL 6 routing: byte j = 1 if the track feeds track j
        }
        if DROP.contains(&i) {
            continue;
        }
        if i == 201 {
            plugin = name(&d);
            if let Some(s) = subst(&plugin) {
                d = [s, b"\0"].concat();
            }
        }
        if [201, 212, 203, 213].contains(&i) {
            // 203 = slot custom name, inside the slot
            if let Some(s) = subst(&plugin) {
                if i == 213 {
                    d = if plugin == b"Fruity Delay Bank" {
                        delay2(&ev)
                    } else {
                        compressor(&d).unwrap_or_else(|e| {
                            warnings.push(format!("Multiband Compressor -> Fruity Compressor: {e}"));
                            Vec::new()
                        })
                    };
                }
                if i == 212 && s == b"Fruity Compressor" {
                    d[8] = 0; // VST-hosted in FL 5
                }
            }
            if i == 201 && slots.last().is_some_and(|e| e.0 == 212) {
                slots.push((213, Vec::new())); // FL 5 writes empty states
            }
            slots.push((i, d));
            continue;
        }
        if [149, 204, 154, 147].contains(&i) {
            if i == 147 {
                // FL 5 output target: 0 master, j insert j (one only); the master has 65
                let j = (1..65).find(|&j| route.as_ref().is_some_and(|r| r.get(j).is_some_and(|&b| b != 0))).unwrap_or(0);
                d = (if t == 0 { 65 } else { j as i32 }).to_le_bytes().to_vec();
                route = None;
            }
            head.push((i, d));
            if i == 147 {
                if slots.last().is_some_and(|e| e.0 == 212) {
                    slots.push((213, Vec::new()));
                }
                out.append(&mut slots);
                out.append(&mut head);
                t += 1;
            }
            continue;
        }
        let d = match i {
            225 => mixer225(&d),
            227 => links227(d),
            _ => d,
        };
        out.push((i, d));
    }
    let mut n = Vec::new();
    for (i, d) in out.iter_mut() {
        match i {
            201 => n = name(d),
            64 => n.clear(),
            213 if n == b"Sytrus" && d.starts_with(&[15, 0, 0, 0]) => match crate::sytrus::convert(d, 15, 7, crate::sytrus::v15_to_v7) {
                Some(s) => *d = s,
                None => warnings.push("a Sytrus patch didn't convert to FL 5's format".into()),
            },
            _ => {}
        }
    }
    out
}
