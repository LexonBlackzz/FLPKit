//! Sytrus state conversions between versions, on the inflated (zlib) payload.

/// Curve `[u32 type][u32 version][u32 n][n x (f64 x, f64 y, f32 tension, u32 mode)]` at `k`: its point count
fn curve_at(a: &[u8], k: usize, ver: u32) -> Option<usize> {
    let u = |o: usize| u32::from_le_bytes(a[o..o + 4].try_into().unwrap());
    if k + 12 > a.len() {
        return None;
    }
    let (t, v, n) = (u(k), u(k + 4), u(k + 8) as usize);
    if !(1..=8).contains(&t) || v != ver || !(1..=4096).contains(&n) || k + 12 + 24 * n > a.len() {
        return None;
    }
    for j in 0..n {
        let o = k + 12 + 24 * j;
        let x = f64::from_le_bytes(a[o..o + 8].try_into().unwrap());
        let y = f64::from_le_bytes(a[o + 8..o + 16].try_into().unwrap());
        let te = f32::from_le_bytes(a[o + 16..o + 20].try_into().unwrap());
        if !(x.is_finite() && y.is_finite() && te.abs() <= 1.0001 && -1e6 < x && x < 1e6) {
            return None;
        }
    }
    Some(n)
}

/// (start, end) of every curve of `ver`
fn curves(a: &[u8], ver: u32) -> Vec<(usize, usize)> {
    let (mut out, mut k) = (Vec::new(), 0);
    while k + 12 < a.len() {
        let n = if a[k + 4..k + 8] == ver.to_le_bytes() { curve_at(a, k, ver) } else { None };
        match n {
            Some(n) => {
                out.push((k, k + 12 + 24 * n));
                k += 12 + 24 * n;
            }
            None => k += 1,
        }
    }
    out
}

/// Plain patch text -> RTF with FL 8's preset template (colors / fonts as FL 8's default patch), latin1, zero-terminated
fn rtf(text: &str) -> Vec<u8> {
    let esc = text.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}");
    let esc = esc.replace("\r\n", "\r").replace('\n', "\r");
    let lines: Vec<&str> = esc.split('\r').collect();
    let mut body = format!("\\cf1\\b\\f0\\fs16 {}\\b0\r\n", lines[0]);
    for l in &lines[1..] {
        body += &format!("\\par \\cf2 {l}\r\n");
    }
    let s = format!(
        "{{\\rtf1\\ansi\\ansicpg1252\\deff0\\deflang1033{{\\fonttbl{{\\f0\\fswiss\\fprq2 Arial;}}}}\r\n\
         {{\\colortbl ;\\red196\\green198\\blue211;\\red131\\green134\\blue154;}}\r\n\
         \\viewkind4\\uc1\\pard {body}\\par }}\r\n"
    );
    let mut out: Vec<u8> = s.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect();
    out.push(0);
    out
}

fn flag(g: &mut [u8], k: usize) {
    if g.get(k) == Some(&3) {
        g[k] = 2;
    }
}

/// Sytrus v23 (FL 12, `[u16 3][u16 23][zlib]`) -> v18 (FL 8, `[u32 18][zlib]`), inflated payloads. Learned from
/// SolidInc's 2 patches saved by FL 8.0 and FL 12.3 (equal after these rules except automated filter cutoffs):
/// - byte 977: v23 inserts one
/// - patch text: v23 [varint n][n UTF-16 chars], v18 [u32 len][RTF, ends "}\r\n\0"]
/// - curves: version 3 -> 2, the mode's top byte (0..2 in v23) cleared
/// - version bytes 3 -> 2: op + 2128 (op 0 right after the text, ops 1..5 20 bytes into their 2164-byte section),
///   36-byte sections at 20, 37-byte at 21, 76-byte at 60; 40-byte sections lose their last 4
pub fn v23_to_v18(b: &[u8]) -> Option<Vec<u8>> {
    let b: Vec<u8> = [&b[..977], &b[978..]].concat();
    let (mut o, mut n, mut sh) = (1136, 0usize, 0);
    loop {
        let x = *b.get(o)?;
        o += 1;
        n |= ((x & 0x7f) as usize) << sh;
        sh += 7;
        if x < 0x80 {
            break;
        }
    }
    let u: Vec<u16> = b.get(o..o + 2 * n)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let r = rtf(&String::from_utf16_lossy(&u));
    o += 2 * n;
    let cv = curves(&b, 3);
    let mut op0 = b.get(o..cv.first()?.0)?.to_vec();
    flag(&mut op0, 2128);
    let mut out = b[..1136].to_vec();
    out.extend((r.len() as u32).to_le_bytes());
    out.extend(&r);
    out.extend(op0);
    for (j, &(s, e)) in cv.iter().enumerate() {
        let mut c = b[s..e].to_vec();
        c[4] = 2;
        for k in (12..c.len()).step_by(24) {
            c[k + 23] = 0;
        }
        out.extend(c);
        let mut g = b[e..cv.get(j + 1).map_or(b.len(), |x| x.0)].to_vec();
        match g.len() {
            2164 => flag(&mut g, 2148),
            40 => g.truncate(36),
            36 => flag(&mut g, 20),
            37 => flag(&mut g, 21),
            76 => flag(&mut g, 60),
            _ => {}
        }
        out.extend(g);
    }
    Some(out)
}

fn u32le(a: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(a[o..o + 4].try_into().unwrap())
}
fn f32le(a: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(a[o..o + 4].try_into().unwrap())
}
fn f64le(a: &[u8], o: usize) -> f64 {
    f64::from_le_bytes(a[o..o + 8].try_into().unwrap())
}

/// v18 curve `[u32 t <= 8][u32 2][u32 n][n x (f64 x, f64 y, f32 tension, u32 mode)]` at `k`: its point count
fn curve2_at(a: &[u8], k: usize) -> Option<usize> {
    if k + 12 > a.len() {
        return None;
    }
    let (t, v, n) = (u32le(a, k), u32le(a, k + 4), u32le(a, k + 8) as usize);
    if t > 8 || v != 2 || !(1..=4096).contains(&n) || k + 12 + 24 * n > a.len() {
        return None;
    }
    for j in 0..n {
        let o = k + 12 + 24 * j;
        let (x, y, te, m) = (f64le(a, o), f64le(a, o + 8), f32le(a, o + 16), u32le(a, o + 20));
        if !(x.is_finite() && y.is_finite() && te.abs() <= 1.0001 && m < 0x10000 && -1e6 < x && x < 1e6) {
            return None;
        }
    }
    Some(n)
}

/// curve objects v2 (f64 points) -> v1 (f32 points), as in automation clips (event 234)
fn curves_v1(a: &[u8]) -> Vec<u8> {
    let (mut out, mut o, mut k) = (Vec::new(), 0, 0);
    while k + 12 < a.len() {
        let n = if a[k + 4..k + 8] == [2, 0, 0, 0] { curve2_at(a, k) } else { None };
        let Some(n) = n else {
            k += 1;
            continue;
        };
        let mut head = a[o..k].to_vec();
        let h = head.len();
        // type-1 curves sit in a wrapper [u32 2][u32 0][f32][f32]: its version 2 -> 1 too
        if a[k] == 1 && h >= 16 && head[h - 16..h - 12] == [2, 0, 0, 0] && head[h - 12..h - 8] == [0; 4] {
            head[h - 16] = 1;
        }
        out.extend(head);
        out.extend(&a[k..k + 4]);
        out.extend(1u32.to_le_bytes());
        out.extend(&a[k + 8..k + 12]);
        for j in 0..n {
            let p = k + 12 + 24 * j;
            out.extend((f64le(a, p) as f32).to_le_bytes());
            out.extend((f64le(a, p + 8) as f32).to_le_bytes());
            out.extend(&a[p + 16..p + 24]);
        }
        k += 12 + 24 * n;
        o = k;
    }
    out.extend(&a[o..]);
    out
}

/// per operator: two f32 tables of 256 (FL 8) -> 128 (FL 6) entries, 52 bytes after the operator start (after the
/// preset's RTF text or an 8 x ff run); FL 8 pads the upper halves with zeros
fn tables(a: &[u8]) -> Vec<u8> {
    let z = |s: &[u8]| s.iter().all(|&b| b == 0);
    let (mut out, mut o, mut h) = (Vec::new(), 0, 0);
    while h + 2100 < a.len() {
        let x = h + 52;
        let start = (h >= 8 && a[h - 8..h] == [255; 8]) || (h >= 4 && a[h - 4..h] == [0x7d, 0x0d, 0x0a, 0]);
        if start && z(&a[x + 512..x + 1024]) && z(&a[x + 1536..x + 2048]) {
            out.extend(&a[o..x + 512]);
            out.extend(&a[x + 1024..x + 1536]);
            h = x + 2048;
            o = h;
            continue;
        }
        h += 1;
    }
    out.extend(&a[o..]);
    out
}

/// Sytrus v18 (FL 8) -> v15 (FL 6), inflated payloads; byte-exact on 20 demo pairs. FL 8 inserts 3 bytes (01 00 01)
/// at 974 of the global header
pub fn v18_to_v15(a: &[u8]) -> Option<Vec<u8>> {
    if a.get(974..977)? != [1, 0, 1] {
        return None;
    }
    Some(tables(&curves_v1(&[&a[..974], &a[977..]].concat())))
}

/// (start, end) of each v1 curve `[u32 type][u32 1][u32 n][n x 16 bytes]`
fn curves1(a: &[u8]) -> Vec<(usize, usize)> {
    let (mut out, mut k) = (Vec::new(), 0);
    while k + 12 < a.len() {
        if a[k + 4..k + 8] == [1, 0, 0, 0] {
            let (t, n) = (u32le(a, k), u32le(a, k + 8) as usize);
            if (1..=8).contains(&t)
                && (1..=4096).contains(&n)
                && k + 12 + 16 * n <= a.len()
                && (0..n).all(|j| f32le(a, k + 12 + 16 * j).abs() < 1e6 && f32le(a, k + 20 + 16 * j).abs() <= 1.0001)
            {
                out.push((k, k + 12 + 16 * n));
                k += 12 + 16 * n;
                continue;
            }
        }
        k += 1;
    }
    out
}

/// operator block (fields from `h`): v15 adds a u32 at h+48 (operator noise; v7 keeps it near the end), 2 tables of
/// 128 f32 (v7: 24), and reshapes the 12 bytes from end-28
fn op15(g: &[u8], h: usize) -> Vec<u8> {
    let e = g.len();
    let p = &g[e - 28..e - 16];
    [&g[..h + 48], &g[h + 52..h + 148], &g[h + 564..h + 660], &g[h + 1076..e - 28], &p[..4], &g[h + 48..h + 52], &[0; 4], &p[8..11], &g[e - 16..]]
        .concat()
}

/// Sytrus v15 (FL 6) -> v7 (FL 5), inflated payloads. From the one patch both ship (Moholithic Filter) and the
/// user's FL 5 save. ponytail: harmonic tables cut to FL 5's 24 entries; cut points in zero runs are guesses
pub fn v15_to_v7(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 1137 || u32le(b, 1129) != 2 {
        return None;
    }
    let mut out = Vec::new();
    let mut o = 0;
    for (s, e) in [(966, 972), (1043, 1081), (1095, 1099), (1112, 1116), (1129, 1133)] {
        out.extend(&b[o..s]);
        o = e;
    }
    let rtf_end = 1137 + u32le(b, 1133) as usize;
    out.extend(b.get(o..rtf_end)?);
    let cv = curves1(b);
    out.extend(op15(b.get(rtf_end..cv.first()?.0)?, 0));
    for (j, &(s, e)) in cv.iter().enumerate() {
        out.extend(&b[s..e]);
        let g = &b[e..cv.get(j + 1).map_or(b.len(), |x| x.0)];
        match g.len() {
            1140 if g[4..20] == [255; 16] => out.extend(op15(g, 20)),
            108 => out.extend(&g[..101]),
            37 => out.extend([&g[..20], &g[21..]].concat()),
            _ => out.extend(g),
        }
    }
    Some(out)
}

fn flag1(g: &mut [u8], k: usize) {
    if let Some(b) = g.get_mut(k).filter(|b| **b == 1) {
        *b = 0;
    }
}

/// Sytrus v7 (FL 5) -> v6 (FL 4.5), inflated payloads. From "Synth string - fat texture" saved by FL 4.5.2 and 4.9.0.
/// ponytail: the 1 -> 0 flag bytes are 1 in v7 and 0 in v6 on that pair; meaning not decoded
pub fn v7_to_v6(b: &[u8]) -> Option<Vec<u8>> {
    let cv = curves1(b);
    let r = 1081 + u32le(b, 1077) as usize;
    let mut head = b.get(..cv.first()?.0)?.to_vec();
    flag1(&mut head, r + 271);
    head.remove(967); // v7 adds a byte at 967
    let mut out = head;
    for (j, &(s, e)) in cv.iter().enumerate() {
        let n = u32le(b, s + 8) as usize;
        out.extend(&b[s..s + 4]);
        out.extend([0; 4]); // curve version 1 -> 0, points 16 -> 12 bytes
        out.extend(&b[s + 8..s + 12]);
        for k in 0..n {
            out.extend(&b[s + 12 + 16 * k..s + 24 + 16 * k]);
        }
        let mut g = b[e..cv.get(j + 1).map_or(b.len(), |x| x.0)].to_vec();
        match g.len() {
            307 => flag1(&mut g, 291),
            36 => flag1(&mut g, 20),
            76 => flag1(&mut g, 60),
            _ => {}
        }
        out.extend(g);
    }
    Some(out)
}

/// `[u32 from][zlib]` state -> `[u32 to][zlib]` through `f`, or None when the version or layout doesn't match
pub(crate) fn convert(d: &[u8], from: u32, to: u32, f: fn(&[u8]) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    use miniz_oxide::{deflate::compress_to_vec_zlib, inflate::decompress_to_vec_zlib};
    if d.len() < 4 || u32le(d, 0) != from {
        return None;
    }
    let p = f(&decompress_to_vec_zlib(&d[4..]).ok()?)?;
    Some([&to.to_le_bytes()[..], &compress_to_vec_zlib(&p, 6)].concat())
}
