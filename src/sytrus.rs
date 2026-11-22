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
