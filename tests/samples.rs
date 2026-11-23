//! Converts every samples/*.flp and diffs note on/off per track against FL's own .mid export.
//! Needs the samples/ folder (FL projects + FL's own exports, not in the repo); every test skips without it.

type Ev = (u64, u8, u8, u8);

/// Note tracks only: (name, sorted note events, other channel events in file order).
fn smf_notes(b: &[u8]) -> Vec<(String, Vec<Ev>, Vec<Ev>)> {
    fn vlq(b: &[u8], i: &mut usize) -> u64 {
        let mut v = 0;
        loop {
            let c = b[*i];
            *i += 1;
            v = (v << 7) | (c & 0x7f) as u64;
            if c & 0x80 == 0 {
                return v;
            }
        }
    }
    let ntracks = u16::from_be_bytes([b[10], b[11]]);
    let mut i = 14;
    let mut out = Vec::new();
    for _ in 0..ntracks {
        assert_eq!(&b[i..i + 4], b"MTrk");
        let end = i + 8 + u32::from_be_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        i += 8;
        let (mut tick, mut rs, mut name, mut notes, mut ctl) = (0, 0u8, String::new(), Vec::new(), Vec::new());
        while i < end {
            tick += vlq(b, &mut i);
            match b[i] {
                0xff => {
                    let ty = b[i + 1];
                    i += 2;
                    let l = vlq(b, &mut i) as usize;
                    if ty == 3 {
                        name = String::from_utf8_lossy(&b[i..i + l]).into();
                    }
                    i += l;
                }
                0xf0 | 0xf7 => {
                    i += 1;
                    let l = vlq(b, &mut i) as usize;
                    i += l;
                }
                s => {
                    if s & 0x80 != 0 {
                        rs = s;
                        i += 1;
                    }
                    let n = if matches!(rs >> 4, 0xc | 0xd) { 1 } else { 2 };
                    let (k, v) = (b[i], if n == 2 { b[i + 1] } else { 0 });
                    i += n;
                    match rs >> 4 {
                        9 if v == 0 => notes.push((tick, 0x80 | rs & 15, k, 64)),
                        8 | 9 => notes.push((tick, rs, k, v)),
                        _ => ctl.push((tick, rs, k, v)),
                    }
                }
            }
        }
        notes.sort();
        if !notes.is_empty() {
            out.push((name, notes, ctl));
        }
    }
    out
}

#[test]
fn matches_fl_exports() {
    let mut checked = 0;
    let Ok(dir) = std::fs::read_dir("samples") else { return };
    for entry in dir {
        let path = entry.unwrap().path();
        if path.extension().map_or(true, |e| e != "flp") {
            continue;
        }
        let Ok(fl) = std::fs::read(path.with_extension("mid")) else { continue };
        let p = flparser::Project::parse(&std::fs::read(&path).unwrap()).unwrap();
        let mut ours = smf_notes(&p.to_smf());
        // FL8's exporter ends every note 1 tick early; FL11+ export the same data at full
        // length, so we keep full length and only adjust the comparison.
        if p.version.starts_with("8.") {
            for (_, ev, _) in &mut ours {
                ev.iter_mut().filter(|e| e.1 & 0xf0 == 0x80).for_each(|e| e.0 -= 1);
                ev.sort();
            }
        }
        let theirs = smf_notes(&fl);
        assert_eq!(ours.len(), theirs.len(), "{path:?}: note track count");
        // CC/pitch-bend reset blocks: we emulate the FL 20.0 and 20.6+ exporters; older ones differ.
        let ver: Vec<u32> = p.version.split('.').map(|s| s.parse().unwrap_or(0)).collect();
        let check_ctl = ver[0] >= 20;
        for ((on, oe, oc), (tn, te, tc)) in ours.iter().zip(&theirs) {
            assert_eq!(on, tn, "{path:?}: track name");
            assert_eq!(oe.len(), te.len(), "{path:?} {on}: event count");
            if let Some((a, b)) = oe.iter().zip(te).find(|(a, b)| a != b) {
                panic!("{path:?} {on}: first diff ours {a:?} vs FL {b:?}");
            }
            if check_ctl {
                // masterpitch's tick-0 knob value is FL's live knob state at export time (0, 127
                // and 116 in three exports of the same file), so there only pitch bends count.
                let live_knobs = path.to_string_lossy().contains("masterpitch");
                let same = |a: &&Ev, b: &&Ev| a == b || (live_knobs && a.1 >> 4 == 0xb && b.1 >> 4 == 0xb && a.0 == 0);
                if let Some((i, (a, b))) = oc.iter().zip(tc).enumerate().find(|(_, (a, b))| !same(a, b)) {
                    panic!("{path:?} {on}: CC #{i} ours {a:?} vs FL {b:?}");
                }
                assert_eq!(oc.len(), tc.len(), "{path:?} {on}: CC count");
            }
        }
        checked += 1;
    }
    assert!(checked > 0, "no flp/mid pairs in samples/");
    eprintln!("{checked} projects match FL's export");
}

#[test]
fn writer_round_trips() {
    let Ok(dir) = std::fs::read_dir("samples") else { return };
    for entry in dir {
        let path = entry.unwrap().path();
        if path.extension().map_or(true, |e| e != "flp") {
            continue;
        }
        let b = std::fs::read(&path).unwrap();
        let (h, ev) = flparser::parse(&b).unwrap();
        let ev: Vec<(u8, Vec<u8>)> = ev.iter().map(|e| (e.id, e.data.to_vec())).collect();
        assert!(flparser::write(h, &ev) == b, "{path:?} doesn't round-trip");
    }
}

/// Downgraded FL 2025/2026 saves must match FL's own 20.6 / 20.9.1 save of the same song
/// event-for-event; payloads may differ only where FL itself varies (colors, window
/// positions, registration, sample paths, native plugin states, default mixer params).
#[test]
fn downgrade_matches_fl20_saves() {
    const MAY_DIFFER: &[u8] = &[28, 128, 133, 150, 196, 200, 212, 213, 219, 225, 226, 236, 237, 238];
    // plugin states that must come out byte-identical to FL's own 20.x save
    const EXACT_PLUGINS: &[&str] = &["Fruity Parametric EQ 2", "Fruity Limiter", "MIDI Out", "3x Osc", "Fruity Compressor", "Fruity Free Filter", "Maximus"];
    let mut checked = 0;
    for song in ["newstuffFromFL8MIDI", "SolidInc-What I found"] {
        for (target, real) in [("20.9.1.2826", "FL20.9.1"), ("20.6.2.1549", "FL20.6")] {
            for src in ["FL2025", "FL2026"] {
                let Ok(b) = std::fs::read(format!("samples/{song}({src}Export).flp")) else { continue };
                let Ok(r) = std::fs::read(format!("samples/{song}({real}Export).flp")) else { continue };
                let (ours, _) = flparser::downgrade::downgrade(&b, target, false).unwrap();
                let (_, a) = flparser::parse(&ours).unwrap();
                let (_, e) = flparser::parse(&r).unwrap();
                let ids = |v: &[flparser::Event]| v.iter().map(|e| e.id).collect::<Vec<_>>();
                let ctx = format!("{song} {src} -> {target}");
                // SolidInc's FL2025 save has one 158, 26, 162 run where FL 20 wrote only 158
                // (FL's own saves differ there, not a conversion step).
                let mut a = a;
                if src == "FL2025" && song.starts_with("SolidInc") {
                    let k = a.iter().zip(&e).position(|(x, y)| x.id != y.id).unwrap();
                    assert_eq!([a[k - 1].id, a[k].id, a[k + 1].id], [158, 26, 162], "{ctx}");
                    a.drain(k..k + 2);
                }
                assert_eq!(ids(&a), ids(&e), "{ctx}: event sequence");
                let mut plugin = String::new();
                for (x, y) in a.iter().zip(&e) {
                    if x.id == 201 {
                        plugin = flparser::text(x.data);
                    }
                    if x.id == 213 && EXACT_PLUGINS.contains(&plugin.as_str()) {
                        assert!(x.data == y.data, "{ctx}: {plugin} state at {:#x}", y.offset);
                    }
                    // FL2026 itself re-mapped one channel's 215[108] (5 -> 6) that FL2025 kept
                    let masked = |d: &[u8]| {
                        let mut d = d.to_vec();
                        if x.id == 215 && src == "FL2026" && d.len() > 108 {
                            d[108] = 0;
                        }
                        d
                    };
                    // FL 2026 re-saves untrimmed clips as 0/len, FL 20 wrote -1/-1: same clip
                    let clips = |d: &[u8]| {
                        let mut d = d.to_vec();
                        for r in d.chunks_exact_mut(32) {
                            if r[24..28] == [0; 4] && r[28..32] == r[8..12] {
                                r[24..32].fill(0xff);
                            }
                        }
                        d
                    };
                    if x.id == 233 {
                        // overlapping clips are cut where the next clip on their track starts (as
                        // FL 2025+ plays them; FL 20 kept the overlap): allow only exactly that
                        let mut ours = x.data.to_vec();
                        let starts: Vec<(&[u8], &[u8])> = x.data.chunks_exact(32).map(|r| (&r[12..14], &r[0..4])).collect();
                        for (r, f) in ours.chunks_exact_mut(32).zip(y.data.chunks_exact(32)) {
                            let u = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
                            let end = (u(&r[0..4]) + u(&r[8..12])).to_le_bytes();
                            if u(&r[8..12]) < u(&f[8..12]) && starts.iter().any(|(t, p)| *t == &r[12..14] && *p == end) {
                                r[8..12].copy_from_slice(&f[8..12]);
                                r[28..32].copy_from_slice(&f[28..32]);
                            }
                        }
                        assert!(clips(&ours) == clips(y.data), "{ctx}: playlist differs");
                        continue;
                    }
                    if masked(x.data) != masked(y.data) && !MAY_DIFFER.contains(&x.id) {
                        panic!("{ctx}: event {} at {:#x} differs", x.id, y.offset);
                    }
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 0 || !std::path::Path::new("samples").exists());
    eprintln!("{checked} downgrades match FL 20 saves");
}

/// Downgraded to FL 12.3, the event sequence must equal FL 12's own save, except that we keep
/// 238/239 for named playlist tracks past the last used one (FL 12 drops them). Payloads may
/// also differ in sample paths FL 12 made relative to its install (196), FL 12-only mixer flags
/// (236) and untrimmed clips' lengths / -1 encoding (233).
#[test]
fn downgrade_matches_fl12_saves() {
    const MAY_DIFFER: &[u8] = &[28, 128, 133, 150, 196, 200, 201, 212, 213, 215, 219, 225, 226, 233, 236, 237, 238];
    for song in ["newstuffFromFL8MIDI", "SolidInc-What I found"] {
        let (Ok(b), Ok(r)) = (std::fs::read(format!("samples/{song}(FL2026Export).flp")), std::fs::read(format!("samples/{song}(FL12Export).flp"))) else { continue };
        let (ours, _) = flparser::downgrade::downgrade(&b, "12.3.0.72", true).unwrap();
        let (_, a) = flparser::parse(&ours).unwrap();
        let (_, e) = flparser::parse(&r).unwrap();
        let tracks = e.iter().filter(|x| x.id == 238).count() as u32;
        let a: Vec<_> = a
            .iter()
            .enumerate()
            .filter(|(i, x)| {
                let extra = |y: &flparser::Event| y.id == 238 && u32::from_le_bytes(y.data[..4].try_into().unwrap()) > tracks;
                !(extra(x) || (x.id == 239 && *i > 0 && extra(&a[i - 1])))
            })
            .map(|(_, x)| x)
            .collect();
        let ids = |v: &[&flparser::Event]| v.iter().map(|x| x.id).collect::<Vec<_>>();
        assert_eq!(ids(&a), ids(&e.iter().collect::<Vec<_>>()), "{song}: event sequence");
        for (x, y) in a.iter().zip(&e) {
            assert!(x.data == y.data || MAY_DIFFER.contains(&x.id), "{song}: event {} at {:#x} differs", x.id, y.offset);
            // zlib plugin states (Sytrus, Love Philter): [u16 3][u16 version] must be FL 12's
            if x.id == 213 && y.data.starts_with(&[3, 0]) {
                assert_eq!(x.data[..4], y.data[..4], "{song}: plugin state version at {:#x}", y.offset);
            }
            // native legacy effects -> FL 12 VST wrapper: same records, VST2 chunk floats close
            if x.id == 213 && y.data.windows(19).any(|w| w == br"Plugins\VST\Fruity ") {
                assert_eq!(x.data.len(), y.data.len(), "{song}: wrapper at {:#x}", y.offset);
                let c = y.data.windows(4).position(|w| w == [0xf7, 0xff, 0xff, 0xff]).unwrap();
                // record 2 byte 9 (at 57) varies per instance in FL 12's own saves
                let (mut xs, mut ys) = (x.data[..c + 25].to_vec(), y.data[..c + 25].to_vec());
                (xs[57], ys[57]) = (0, 0);
                assert_eq!(xs, ys, "{song}: wrapper at {:#x}", y.offset);
                let n = u32::from_le_bytes(y.data[c + 21..c + 25].try_into().unwrap()) as usize;
                let f = |d: &[u8], k: usize| f32::from_le_bytes(d[c + 25 + 4 * k..c + 29 + 4 * k].try_into().unwrap());
                for k in 0..n {
                    assert!((f(&x.data, k) - f(&y.data, k)).abs() < 0.02, "{song}: wrapper at {:#x} param {k}", y.offset);
                }
                assert_eq!(x.data[c + 25 + 4 * n..], y.data[c + 25 + 4 * n..]);
            }
        }
    }
}

#[test]
fn downgrade_refuses_vsts_and_newer_plugins() {
    for (f, expect) in [
        ("samples/cctest(FL2026).flp", "Emphasizer"),
    ] {
        let Ok(b) = std::fs::read(f) else { continue };
        let err = flparser::downgrade::downgrade(&b, "20.6.2.1549", false).unwrap_err();
        assert!(err.contains(expect), "{f}: {err}");
        assert!(flparser::downgrade::downgrade(&b, "20.6.2.1549", true).is_ok());
    }
}

/// Per-clip gain/fades add channels, automation clips and playlist records: every record must
/// stay 32 bytes and point at an existing channel, and the song must end where the source does.
#[test]
fn per_clip_ports_keep_playlist_valid() {
    let Ok(b) = std::fs::read("samples/persamplevariations(FL2026Export).flp") else { return };
    let (ours, _) = flparser::downgrade::downgrade(&b, "20.6.2.1549", true).unwrap();
    let (h, ev) = flparser::parse(&ours).unwrap();
    assert_eq!(ev.iter().filter(|e| e.id == 64).count(), h.channels as usize);
    let end = |d: &[u8], w: usize| {
        d.chunks_exact(w).map(|r| u32::from_le_bytes(r[0..4].try_into().unwrap()) + u32::from_le_bytes(r[8..12].try_into().unwrap())).max()
    };
    let pl = ev.iter().find(|e| e.id == 233).unwrap().data;
    assert_eq!(pl.len() % 32, 0);
    // FL 20 keeps records sorted by position; out of order, it skipped drawing them
    let pos: Vec<u32> = pl.chunks_exact(32).map(|r| u32::from_le_bytes(r[0..4].try_into().unwrap())).collect();
    assert!(pos.windows(2).all(|w| w[0] <= w[1]), "playlist not sorted by position");
    for r in pl.chunks_exact(32) {
        let item = u16::from_le_bytes([r[6], r[7]]);
        assert!(item > 0x5000 || item < h.channels, "clip points at missing channel {item}");
    }
    let (_, src) = flparser::parse(&b).unwrap();
    assert_eq!(end(pl, 32), end(src.iter().find(|e| e.id == 233).unwrap().data, 88));
    // 8 faded clips -> 6 distinct fades, each an automation channel linked to a volume
    assert_eq!(ev.iter().filter(|e| e.id == 227).count(), 6);
}

/// MIDI Out v7 with a controller list converts to what FL 20.6 saved for the same project, except
/// bytes 38 / 42, which the user changed there (0 in every FL save before that, 2026 included).
#[test]
fn midi_out_controllers_match_fl20() {
    let (Ok(b), Ok(real)) = (std::fs::read("samples/cctest(FL2026).flp"), std::fs::read("samples/cctest(FL20.6).flp")) else { return };
    let (ours, _) = flparser::downgrade::downgrade(&b, "20.6.2.1549", true).unwrap();
    let states = |b: &[u8]| {
        let (_, ev) = flparser::parse(b).unwrap();
        let mut plugin = String::new();
        let mut out = Vec::new();
        for e in ev {
            match e.id {
                201 => plugin = flparser::text(e.data),
                213 if plugin == "MIDI Out" => {
                    let mut d = e.data.to_vec();
                    (d[38], d[42]) = (0, 0);
                    out.push(d);
                }
                _ => {}
            }
        }
        out
    };
    assert_eq!(states(&ours), states(&real));
}

/// VST (Fruity Wrapper) and Fruity Limiter states converted from FL 2026 must equal what
/// FL 20.6 wrote for the same plugins, except inside the plugin's own data (record 53),
/// which plugins re-save slightly differently each time (Kickstart 2, Absynth 6) or where
/// FL 20.6 wrote uninitialized memory (dblue Crusher).
#[test]
fn downgraded_plugin_states_match_fl20() {
    fn states(b: &[u8]) -> Vec<(String, Vec<u8>)> {
        let (_, ev) = flparser::parse(b).unwrap();
        let mut plugin = String::new();
        let mut out = Vec::new();
        for e in ev {
            match e.id {
                201 => plugin = flparser::text(e.data),
                213 => out.push((plugin.clone(), e.data.to_vec())),
                _ => {}
            }
        }
        out
    }
    // wrapper records as (id, bytes), with record 53 reduced to its sub-record ids
    fn shape(d: &[u8]) -> Vec<(u32, Vec<u8>)> {
        let (mut o, mut out) = (4, vec![(0, d[..4].to_vec())]);
        while o < d.len() {
            let id = u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
            let n = u64::from_le_bytes(d[o + 4..o + 12].try_into().unwrap()) as usize;
            let body = &d[o + 12..o + 12 + n];
            out.push((id, if id == 53 { body[..5.min(n)].to_vec() } else { body.to_vec() }));
            o += 12 + n;
        }
        out
    }
    let mut exact = 0;
    for v in ["", " v2"] {
        let Ok(b) = std::fs::read(format!("samples/vsttest w automation{v} (FL2026Export).flp")) else { continue };
        let Ok(real) = std::fs::read(format!("samples/vsttest w automation{v} (FL20.6Export).flp")) else { continue };
        let (ours, _) = flparser::downgrade::downgrade(&b, "20.6.2.1549", true).unwrap();
        for ((p, x), (q, y)) in states(&ours).iter().zip(states(&real)) {
            assert_eq!(p, &q);
            match p.as_str() {
                "Fruity Wrapper" => {
                    assert_eq!(shape(x), shape(&y), "vsttest{v} wrapper layout");
                    exact += (x == &y) as usize;
                }
                "Fruity Limiter" | "Soundgoodizer" => assert_eq!(x, &y, "vsttest{v} {p}"),
                // same layout as FL 20.6's; content keeps 2026's one extra envelope point
                "Gross Beat" => assert_eq!((x[..3].to_vec(), x.len()), (y[..3].to_vec(), y.len() + 24)),
                _ => {}
            }
        }
    }
    assert!(exact >= 4 || !std::path::Path::new("samples").exists(), "expected Nexus, Pro-L 2, Pro-C 2, SVStage byte-identical, got {exact}");
}

#[test]
fn smf_pieces_concatenate_to_to_smf() {
    let Ok(b) = std::fs::read("samples/newstuffFromFL8MIDI(FL2026Export).flp") else { return };
    let p = flparser::Project::parse(&b).unwrap();
    let pieces = p.smf_pieces();
    assert!(pieces.len() > 3);
    assert_eq!(pieces.concat(), p.to_smf());
    assert_eq!(&pieces[0][..4], b"MThd");
}

#[test]
fn channel_colors() {
    let Ok(b) = std::fs::read("samples/SolidInc-What I found(FL2026Export).flp") else { return };
    let p = flparser::Project::parse(&b).unwrap();
    let cs: std::collections::HashSet<_> = p.channels.iter().filter_map(|c| c.color).collect();
    for c in p.channels.iter().take(8) { eprintln!("{:24} {:?}", c.name, c.color); }
    assert!(cs.len() > 1, "expected several distinct channel colors, got {cs:?}");
}

#[test]
fn progress_reports_reach_the_end() {
    // a project with MIDI Out channels: FL 21+ exports nothing else
    let Ok(b) = std::fs::read("samples/newstuffFromFL8MIDI(FL2026Export).flp") else { return };
    let mut last = (0, 0);
    let p = flparser::Project::parse_progress(&b, &mut |d, t| last = (d, t)).unwrap();
    assert_eq!(last, (b.len() as u64, b.len() as u64));
    let mut w = (0, 1, 0);
    let pieces = p.smf_pieces_progress(&mut |t, e, n| w = (t, e, n));
    assert_eq!(pieces.concat(), p.to_smf());
    assert!(w.0 == w.1 && w.2 > 0, "{w:?}");
}

/// FL 12.3 -> 11.1 against FL 11's own save of the same song: same event sequence, and every event equal
/// except ids whose content FL changes on each save (window positions, project info, mixer state).
#[test]
fn fl11_matches_fl11_save() {
    const MAY_DIFFER: &[u8] = &[133, 198, 212, 213, 225, 237];
    let (Ok(b), Ok(real)) = (
        std::fs::read("samples/newstuffFromFL8MIDI(FL12Export).flp"),
        std::fs::read("samples/newstuffFromFL8MIDI(FL11Export).flp"),
    ) else { return };
    let (ours, _) = flparser::fl11::from_12_3(&b).unwrap();
    let ((_, a), (_, e)) = (flparser::parse(&ours).unwrap(), flparser::parse(&real).unwrap());
    let ids = |v: &[flparser::Event]| v.iter().map(|x| x.id).collect::<Vec<_>>();
    assert_eq!(ids(&a), ids(&e));
    for (x, y) in a.iter().zip(&e) {
        assert!(MAY_DIFFER.contains(&x.id) || x.data == y.data, "event {} differs", x.id);
    }
}

/// FL 12.3 -> 8.0: Moholithic (FL 2026 export) keeps its step-sequencer kick as FL 8 step events (91), every
/// event FL 8 lacks is gone, and FL 8's tempo pair (93 fine + 66 BPM) replaces 156.
#[test]
fn fl8_steps_and_tempo() {
    let Ok(b) = std::fs::read("samples/ScottFisher-Moholithic Filter(FL2026Export).flp") else { return };
    let (d12, _) = flparser::downgrade::downgrade(&b, "12.3.0.72", true).unwrap();
    let dir = std::env::temp_dir().join("flpkit-fl8-test samples");
    let (d8, _) = flparser::fl8::from_12_3(&d12, &dir).unwrap();
    let (_, ev) = flparser::parse(&d8).unwrap();
    assert!(ev.iter().any(|e| e.id == 91), "no step events");
    assert!(ev.iter().any(|e| e.id == 66) && !ev.iter().any(|e| e.id == 156));
    assert!(!ev.iter().any(|e| [28, 29, 31, 32, 97, 98, 202, 237, 238, 239].contains(&e.id)));
    assert_eq!(flparser::text(ev.iter().find(|e| e.id == 199).unwrap().data), "8.0.0");
}

/// FL 8.0 -> 7.0 / 6.0 / 5.0 / 4.5.2 / FruityLoops 3.5.4 on What I Found (FL 8's NewStuff.flp): version strings, Sytrus in each target's
/// state version, and for FL 4.5 no automation clips left (channel type 5, event 234) but X-Y Controllers instead.
#[test]
fn fl8_to_older() {
    let Ok(b) = std::fs::read("samples/SolidInc-What I found(FL8Export).flp") else { return };
    let dir = std::env::temp_dir().join("flpkit-older-test samples");
    for (to, sytrus) in [("7.0.0", 18u32), ("6.0.4", 15), ("5.0.0", 7), ("4.5.2", 6), ("3.5.4", 6)] {
        let (d, _) = flparser::fl6::from_8_0(&b, to, &dir).unwrap();
        let (_, ev) = flparser::parse(&d).unwrap();
        assert_eq!(flparser::text(ev.iter().find(|e| e.id == 199).unwrap().data), to);
        let mut plugin = String::new();
        for e in &ev {
            if e.id == 201 {
                plugin = flparser::text(e.data);
            }
            if e.id == 213 && plugin == "Sytrus" {
                assert_eq!(u32::from_le_bytes(e.data[..4].try_into().unwrap()), sytrus, "Sytrus state for {to}");
            }
        }
        if to == "4.5.2" {
            assert!(!ev.iter().any(|e| e.id == 234 || (e.id == 21 && e.data[0] == 5)));
            assert!(ev.iter().any(|e| e.id == 201 && flparser::text(e.data) == "Fruity X-Y Controller"));
        }
        if to == "3.5.4" {
            // playlist as 129 (bar, pattern) events; 21 mixer tracks (one 147 each)
            assert!(!ev.iter().any(|e| e.id == 233) && ev.iter().any(|e| e.id == 129));
            assert_eq!(ev.iter().filter(|e| e.id == 147).count(), 21);
        }
    }
}
