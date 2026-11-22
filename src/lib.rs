//! FLP container reader: `FLhd` header + `FLdt` event stream.
//! Event payload size is decided by ID range:
//! 0..64 = 1 byte, 64..128 = 2, 128..192 = 4, 192..=255 = varint length + blob.

#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub format: i16,
    pub channels: u16,
    pub ppq: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct Event<'a> {
    pub offset: usize,
    pub id: u8,
    pub data: &'a [u8],
}

impl Event<'_> {
    /// Little-endian value for fixed-size events (ids < 192).
    pub fn value(&self) -> u32 {
        self.data.iter().rev().fold(0, |v, &b| (v << 8) | b as u32)
    }
}

fn u16_at(b: &[u8], i: usize) -> Result<u16, String> {
    b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or("truncated".into())
}

fn u32_at(b: &[u8], i: usize) -> Result<u32, String> {
    b.get(i..i + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or("truncated".into())
}

/// Reads a file, or stdin for "-" (e.g. `xz -dc big.flp.xz | flpdump -`).
pub fn read_input(path: &str) -> std::io::Result<Vec<u8>> {
    if path == "-" {
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut std::io::stdin().lock(), &mut b)?;
        Ok(b)
    } else {
        std::fs::read(path)
    }
}

pub fn parse(b: &[u8]) -> Result<(Header, Vec<Event<'_>>), String> {
    if b.get(0..4) != Some(b"FLhd") {
        return Err("not an FLP (missing FLhd)".into());
    }
    let hlen = u32_at(b, 4)? as usize;
    let header = Header {
        format: u16_at(b, 8)? as i16,
        channels: u16_at(b, 10)?,
        ppq: u16_at(b, 12)?,
    };
    let mut i = 8 + hlen;
    if b.get(i..i + 4) != Some(b"FLdt") {
        return Err(format!("missing FLdt at {i:#x}"));
    }
    let dlen = u32_at(b, i + 4)? as usize;
    i += 8;
    let end = (i + dlen).min(b.len());

    let mut events = Vec::new();
    let mut ver = [0u16; 4]; // from FLVersion (199), always the first event
    while i < end {
        let offset = i;
        let id = b[i];
        i += 1;
        let len = match id {
            // Breaks the size rule from 25.2.3 on (every 25.2.3+ demo file desyncs
            // with 4 and walks cleanly with 3). ponytail: per-version table if more appear.
            172 if ver >= [25, 2, 3, 0] => 3,
            0..=63 => 1,
            64..=127 => 2,
            128..=191 => 4,
            _ => {
                let (mut len, mut shift) = (0usize, 0);
                loop {
                    let byte = *b.get(i).ok_or("truncated varint")?;
                    i += 1;
                    len |= ((byte & 0x7f) as usize) << shift;
                    if byte & 0x80 == 0 {
                        break;
                    }
                    shift += 7;
                }
                len
            }
        };
        let data = b.get(i..i + len).ok_or(format!("event {id} at {offset:#x} overruns file"))?;
        i += len;
        if id == 199 {
            ver = version(&text(data));
        }
        events.push(Event { offset, id, data });
    }
    Ok((header, events))
}

/// "20.9.1.2826" -> [20, 9, 1, 2826]; missing parts are 0.
pub fn version(s: &str) -> [u16; 4] {
    let mut v = [0u16; 4];
    for (slot, part) in v.iter_mut().zip(s.split('.')) {
        *slot = part.trim_end_matches('\0').parse().unwrap_or(0);
    }
    v
}

/// Serializes an event stream back into an FLP (inverse of `parse`).
pub fn write(h: Header, events: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (id, d) in events {
        body.push(*id);
        if *id >= 192 {
            let mut n = d.len();
            loop {
                let b = (n & 0x7f) as u8;
                n >>= 7;
                body.push(if n > 0 { b | 0x80 } else { b });
                if n == 0 {
                    break;
                }
            }
        }
        body.extend(d);
    }
    let mut out = b"FLhd".to_vec();
    out.extend(6u32.to_le_bytes());
    out.extend(h.format.to_le_bytes());
    out.extend(h.channels.to_le_bytes());
    out.extend(h.ppq.to_le_bytes());
    out.extend(b"FLdt");
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

mod intro;
pub mod downgrade;
pub mod fl11;
pub mod fl4;
pub mod fl5;
pub mod fl6;
pub mod fl8;
pub mod sytrus;

/// Best-effort text decode: UTF-16LE (FL 11.5+?) or 8-bit (older).
pub fn text(data: &[u8]) -> String {
    let looks_utf16 = data.len() >= 2 && data.len() % 2 == 0 && data[1] == 0;
    let s = if looks_utf16 {
        let u: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else {
        data.iter().map(|&c| c as char).collect()
    };
    s.trim_end_matches('\0').to_string()
}

// ---------------------------------------------------------------- project model

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap};

#[derive(Debug, Clone, Copy)]
pub struct Note {
    pub pos: u32,
    pub len: u32,
    pub chan: u16,
    pub key: u8,
    pub vel: u8,
    pub rel: u8,
    pub fine: u8, // 0..240, 120 = none
    pub color: u8, // note color group 0..15 (FL's "MIDI channel" note property)
}

impl Note {
    /// MIDI key as FL's MIDI Out plays it: fine pitch transposes by whole semitones.
    /// ponytail: 10 cents/unit inferred from fine=0 -> -12 semitones only.
    pub fn midi_key(&self) -> u8 {
        (self.key as i32 + (self.fine as i32 - 120) / 10).clamp(0, 127) as u8
    }
}

/// Pattern controller event (223): already sampled by FL, one record per value change.
#[derive(Debug, Clone, Copy)]
pub struct Ctrl {
    pub pos: u32,
    pub param: u16, // 0x0004 = channel pitch (cents); 0x8001 + k = MIDI Out controller k
    pub chan: u16,
    pub val: i32,
}

#[derive(Debug, Clone, Default)]
pub struct Pattern {
    pub notes: Vec<Note>, // sorted by (chan, pos) (stable: file order within a tick)
    pub ctrl: Vec<Ctrl>,  // sorted by pos (stable: file order within a tick)
    /// (channel, start, end) of each channel's run in `notes`
    pub chans: Vec<(u16, usize, usize)>,
}

#[derive(Debug, Clone, Copy)]
pub struct Clip {
    pub pos: u32,
    pub len: u32,
    pub pattern: u16,
    /// pattern tick the clip starts at (left trim); 0 when untrimmed
    pub start: u32,
    /// pattern ticks the clip plays: `len`, or unbounded for an FL 12 untrimmed clip
    /// nothing follows on its track
    pub play: u32,
    /// playlist track as stored (FL 12: counts down from 198; 0 for FL8)
    pub track: u16,
}

/// Playlist clip of an automation channel linked to any other target (event 227).
#[derive(Debug, Clone)]
pub struct AutoClip {
    pub pos: u32,
    pub len: u32,
    pub start: f64,           // clip-local beat at `pos` (left trim)
    pub pts: Vec<(f64, f64, f64, u8)>, // (beat, value 0..1, tension and mode of the segment ending here)
    pub param: u16,           // 0 volume, 4 pitch, 0x8001 + k MIDI Out controller k, ... (as in 223)
    pub target: u16,          // rack channel, or 0x4000 = project
    pub src: u16,             // the automation channel
}
// AutoClip points are raw 0..1; Project::auto_value adds the channel's LFO and Min/Max.

/// An automation clip's LFO (lfo_everything FL 12 + its FL export, Hypernova "Main Vol #4").
#[derive(Debug, Clone, Copy, Default)]
pub struct Lfo {
    pub period: f64,  // beats, tempo-synced: speed 65536 = 1/16 beat, 45819 ("4:0") = 1 beat, exponential
    pub tension: f64, // -1..1: + squares the wave, - makes it spiky (Hypernova 1)
    pub skew: f64,    // -1..1: rise time (1 + skew) / 2 of the period (-1 falling saw, 1 rising saw)
    pub pw: f64,      // -1..1: holds the peak (+) or trough (-) for |pw| of the period
    pub amt: f64,     // -1..1
    pub multiply: bool,
}

impl Lfo {
    /// -1..1 at clip tick `t` (from the clip's origin, `ppq` ticks a beat), with tension, skew and pw
    /// (or nested values of them). A triangle: 0 at the period start rising, + peak, 0 at half, - trough.
    /// FL computes the phase in f32 (lfo_everything: from 16 periods on, the max-speed LFO's two equal
    /// values differ by one 16-bit step, as here). Static knobs match FL's export to +-1 CC.
    pub fn at(&self, t: f64, ppq: f64, tension: f64, skew: f64, pw: f64) -> f64 {
        let ph = ((t as f32 * (1.0 / (ppq * self.period)) as f32).fract() as f64).rem_euclid(1.0);
        let r = ((1.0 + skew) / 2.0).clamp(1e-9, 1.0 - 1e-9);
        let (peak, trough) = (r / 2.0, 0.5 + (1.0 - r) / 2.0);
        let base = |b: f64| {
            if b < peak { b / peak } else if b < trough { 1.0 - 2.0 * (b - peak) / (trough - peak) } else { -1.0 + (b - trough) / (1.0 - trough) }
        };
        // pw: the cycle squeezed into 1 - |pw|, held at its peak (pw > 0) or trough for the rest
        let (l, hold) = (1.0 - pw.abs(), if pw > 0.0 { peak } else { trough });
        let x = if ph < l * hold { base(ph / l) } else if ph < l * hold + pw.abs() { pw.signum() } else { base((ph - pw.abs()) / l) };
        // tension: 1 - g(1 - |x|), g the clip curve with a = 30 t |t| (lfo_everythingnested)
        if tension == 0.0 { return x }
        let a = 30.0 * tension * tension.abs();
        (1.0 - (a * (1.0 - x.abs())).exp_m1() / a.exp_m1()).copysign(x)
    }
}

impl AutoClip {
    /// Value at clip-local `beat`. A segment's curve is set by its end point's tension t:
    /// (e^(a f) - 1) / (e^a - 1) with a = -t ln 10^6 (Hypernova's master pitch, t = -0.08).
    /// ponytail: tension sign checked on one falling segment only
    pub fn value_at(&self, beat: f64) -> f64 {
        // On a jump (FL stores it as a segment ~5e-8 beats long) FL takes the average: Hypernova
        // volume writes 50 between 0 and 100, on the tick whose next tick is the jump.
        const JUMP: f64 = 1e-6;
        let j = self.pts.partition_point(|p| p.0 < beat - JUMP);
        if let (Some(a), Some(b)) = (self.pts.get(j), self.pts.get(j + 1)) {
            if b.0 - a.0 < JUMP && (a.0 - beat).abs() < JUMP {
                return (a.1 + b.1) / 2.0;
            }
        }
        let i = self.pts.partition_point(|p| p.0 <= beat);
        match (i.checked_sub(1).map(|j| self.pts[j]), self.pts.get(i)) {
            (Some((x0, y0, _, _)), Some(&(x1, y1, t, mode))) => {
                let f = (beat - x0) / (x1 - x0);
                let g = if t == 0.0 { f } else if mode == 12 {
                    // point mode 12 ("double curve"): an S of two mirrored halves, each x r / (1 - x + x r)
                    // with r = ((1 + t) / (1 - t))^1.7 (Hypernova: t -0.36, -0.505, -0.715 -> r 0.29, 0.15, 0.042)
                    let r = ((1.0 + t) / (1.0 - t)).powf(1.7);
                    let h = |x: f64| x * r / (1.0 - x + x * r);
                    if f < 0.5 { h(2.0 * f) / 2.0 } else { 1.0 - h(2.0 - 2.0 * f) / 2.0 }
                } else {
                    // mode 0 ("single curve"); other modes unseen
                    let a = -t * 1e6f64.ln();
                    (a * f).exp_m1() / a.exp_m1() // not mirrored on rising segments (that breaks Hypernova's RISERS pitch)
                };
                y0 + (y1 - y0) * g
            }
            (Some((_, y, _, _)), None) | (None, Some(&(_, y, _, _))) => y,
            (None, None) => 0.0,
        }
    }
}

impl Clip {
    /// Song tick of pattern tick `p`, or None if `p` is outside the clip's window
    /// (FL plays only events starting inside it).
    pub fn at(&self, p: u32) -> Option<u64> {
        (p >= self.start && p - self.start < self.play).then(|| self.pos as u64 + (p - self.start) as u64)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Channel {
    pub name: String,
    pub plugin: String,
    pub midi_ch: Option<u8>,
    pub pan: u32,       // 0..12800, 6400 = center (event 219)
    pub volume: u32,    // 0..12800 (event 219)
    pub pitch: i32,     // cents (event 219)
    pub pitch_range: u8, // channel pitch knob range in semitones (byte 112 of 215; FL default 2)
    pub knobs: Vec<(u16, u8)>, // MIDI Out controllers FL sends: (index k of param 0x8001 + k, CC)
    pub knob_init: Vec<u8>, // their knob values in the state (u32 slots at 34 + 4k; Gangnam FL 20.0)
    pub knob_range: Vec<(u8, u8)>, // their Min/Max CC values (Ascension: Damper 63..64)
    pub color: Option<[u8; 3]>, // RGB (event 128, stored as 0x00BBGGRR)
    pub color_map: bool, // MIDI Out: note color group = MIDI channel
    pub program: Option<u8>, // MIDI Out program, sent after each reset block (213 byte 30 = program + 1, 0 = none)
    pub echo: (f64, u32, u32), // echo delay (209): feed 0..1, echoes, time (0 = all on the note)
    pub lfo: Option<Lfo>,      // automation channel LFO (234 tail)
    pub lfo_on: bool,
}

impl Channel {
    fn cc(v: u32) -> u8 {
        (v as u64 * 128 / 12800).min(127) as u8
    }
}

// ponytail: pitch-bend range 12 is what FL exports for every channel seen so far; source unknown.
const BEND_RANGE_CENTS: i32 = 1200;
fn bend(cents: i32) -> u16 {
    (8192 + cents as i64 * 8192 / BEND_RANGE_CENTS as i64).clamp(0, 16383) as u16
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Msg {
    NoteOn { key: u8, vel: u8 },
    NoteOff { key: u8, vel: u8 }, // vel = release
    Cc { cc: u8, val: u8 },
    PitchBend(u16),
    Program(u8),
}

impl Msg {
    pub fn bytes(self, ch: u8) -> [u8; 3] {
        let ch = ch & 15;
        match self {
            Msg::NoteOn { key, vel } => [0x90 | ch, key & 0x7f, vel.min(127)],
            Msg::NoteOff { key, vel } => [0x80 | ch, key & 0x7f, vel.min(127)],
            Msg::Cc { cc, val } => [0xb0 | ch, cc & 0x7f, val.min(127)],
            Msg::PitchBend(v) => [0xe0 | ch, (v & 0x7f) as u8, (v >> 7) as u8],
            Msg::Program(p) => [0xc0 | ch, p & 0x7f, 0], // 2 bytes: see `len`
        }
    }

    pub fn len(self) -> usize {
        if matches!(self, Msg::Program(_)) { 2 } else { 3 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidiEvent {
    pub tick: u64,
    pub chan: u16, // rack channel; MIDI channel is Project::channels[chan].midi_ch
    pub msg: Msg,
    pub color: u8, // the note's color group (0 for controllers): its MIDI channel when the channel maps colors
}

impl Channel {
    /// CC value of clip value `v` for knob `i`: floor(min + v (max - min + 1)) in its Min..Max
    /// (0..127: floor(v * 128); Ascension's Damper 63..64 writes 63 below v = 0.5, 64 above).
    fn knob_cc(&self, i: Option<usize>, v: f64) -> u8 {
        let (lo, hi) = i.and_then(|i| self.knob_range.get(i).copied()).unwrap_or((0, 127));
        (lo as f64 + v * (hi - lo + 1) as f64).floor().clamp(lo as f64, hi as f64) as u8
    }
}

impl Project {
    /// MIDI channel FL's MIDI Out sends `ev` on.
    pub fn midi_channel(&self, ev: &MidiEvent) -> u8 {
        let c = self.channels.get(ev.chan as usize);
        let note = matches!(ev.msg, Msg::NoteOn { .. } | Msg::NoteOff { .. });
        match c {
            // FL 12 counts the color up from the MIDI Out channel, clamped (Hypernova "Arts 11x":
            // ch 3 + color 13 = 15); FL 20+ uses the color alone (ts6 ch 15, collab ch 12).
            // ponytail: one FL 12 sample, and its channel also has echo delay on
            Some(c) if c.color_map && note => match self.version.split('.').next().and_then(|v| v.parse::<u32>().ok()) {
                Some(..=12) => (c.midi_ch.unwrap_or(0) + ev.color).min(15),
                _ => ev.color,
            },
            _ => c.and_then(|c| c.midi_ch).unwrap_or(0),
        }
    }
}

#[derive(Debug, Default)]
pub struct Project {
    pub version: String,
    pub ppq: u16,
    pub tempo: f64,
    pub master_pitch: i32, // cents (event 80); FL's MIDI Out adds it to every channel's pitch bend
    pub time_sig: (u8, u8),
    pub channels: Vec<Channel>,
    pub patterns: HashMap<u16, Pattern>,
    pub clips: Vec<Clip>, // pattern clips only
    pub tempo_clips: Vec<AutoClip>, // values in BPM (as FL's beat counts them, see tempo_points)
    pub auto_clips: Vec<AutoClip>,
    pub tempo_legacy: bool, // tempo points stored as (BPM - 60) / 120 (Nuker 3): FL writes every tick
    pub warnings: Vec<String>,
    pub cut_remainder: bool, // FL's export option "Cut remainder": the tail ends 1 tick after the song
    pub zero_vel_as_one: bool, // write velocity-0 note-ons (read as note-offs by players) as velocity 1
    auto_cache: std::sync::OnceLock<HashMap<u16, Vec<MidiEvent>>>, // playlist automation per channel
}

fn rd32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
fn rd16(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(d[o..o + 2].try_into().unwrap())
}

/// MIDI Out state v7 (FL2026): controller list at 322, entries
/// [index u8][cc u8][23 bytes][pascal name][pascal label], 0xff terminates.
fn midi_out_knobs(d: &[u8]) -> Option<Vec<(u16, u8, (u8, u8))>> {
    // controller list at 322: [idx][cc][19 bytes] (+ 4 bytes in v7) [pascal name][pascal label], 0xff ends
    let extra = match rd32(d, 0) {
        4..=6 => 0, // v4: Hypernova (FL 12.3)
        7 => 4,
        _ => return None, // ponytail: v2/v3 (FL 8-11) not decoded (no samples with knobs)
    };
    let (mut o, mut knobs) = (322, Vec::new());
    while *d.get(o)? != 0xff {
        // entry byte 4 bit 0x80: not sent (Hypernova: only 91, 93, 74, 120 of 14 reach FL's export)
        if d.get(o + 4)? & 0x80 == 0 {
            // entry +9 u32 min, +13 u32 max (Ascension FL 12: Damper 63..64, Cutoff 0..127)
            let (lo, hi) = (rd32(d.get(..o + 17)?, o + 9), rd32(d.get(..o + 17)?, o + 13));
            let range = if lo <= hi && hi <= 127 { (lo as u8, hi as u8) } else { (0, 127) };
            knobs.push((*d.get(o)? as u16, *d.get(o + 1)?, range));
        }
        o += 2 + 19 + extra;
        o += 1 + *d.get(o)? as usize;
        o += 1 + *d.get(o)? as usize;
    }
    Some(knobs)
}

impl Project {
    pub fn parse(b: &[u8]) -> Result<Project, String> {
        Self::parse_progress(b, &mut |_, _| {})
    }

    /// `parse`, calling `progress(bytes done, bytes total)` as it goes (once per FLP event;
    /// pattern note events are the slow ones), and `(total, total)` at the end.
    pub fn parse_progress(b: &[u8], progress: &mut dyn FnMut(u64, u64)) -> Result<Project, String> {
        let (h, events) = parse(b)?;
        let total = b.len() as u64;
        let mut p = Project { ppq: h.ppq, tempo: 140.0, time_sig: (4, 4), ..Default::default() };
        let (mut major, mut pat, mut chan) = (0u32, 0u16, None::<usize>);
        let mut playlists = 0;
        let (mut auto, mut tempo_src, mut chan_clips) = (HashMap::new(), Vec::new(), Vec::new());
        let mut links: Vec<(u16, u16, u16)> = Vec::new(); // (automation channel, param, target)
        let mut starts = std::collections::HashSet::new(); // (track, pos) of every clip
        let mut cur_pat = 0u16;

        for e in &events {
            let d = e.data;
            progress(e.offset as u64, total);
            match e.id {
                199 => {
                    p.version = text(d);
                    major = p.version.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0);
                }
                66 => p.tempo = e.value() as f64,           // FL8
                156 => p.tempo = e.value() as f64 / 1000.0, // FL11+
                17 => p.time_sig.0 = e.value() as u8,
                18 => p.time_sig.1 = e.value() as u8,
                65 => pat = e.value() as u16,
                80 => p.master_pitch = e.value() as u16 as i16 as i32,
                67 => cur_pat = e.value() as u16, // the pattern selected in FL
                64 => {
                    let i = e.value() as usize;
                    if p.channels.len() <= i {
                        p.channels.resize(i + 1, Channel { pan: 6400, volume: 10000, pitch_range: 2, ..Default::default() });
                    }
                    chan = Some(i);
                }
                // Channel fields: first value after channel select wins, so later
                // mixer-effect plugin events don't overwrite the generator.
                128 | 201 | 192 | 203 | 209 | 213 | 215 | 219 if chan.is_some() => {
                    let c = &mut p.channels[chan.unwrap()];
                    match e.id {
                        219 if d.len() >= 12 => (c.pan, c.volume, c.pitch) = (rd32(d, 0), rd32(d, 4), rd32(d, 8) as i32),
                        215 if d.len() > 112 => c.pitch_range = d[112],
                        // feed i32 /25600, pan, pitch, echoes, time (Hypernova "Arts 11x": 6700, 10, 0)
                        209 if d.len() >= 20 => c.echo = (rd32(d, 0) as f64 / 25600.0, rd32(d, 12), rd32(d, 16)),
                        128 if c.color.is_none() && d.len() >= 3 => c.color = Some([d[0], d[1], d[2]]),
                        201 if c.plugin.is_empty() => c.plugin = text(d),
                        192 | 203 if c.name.is_empty() => c.name = text(d),
                        // MIDI Out state: u32 version, u32 MIDI channel (FL8 v2 .. FL2026 v7)
                        213 if c.plugin == "MIDI Out" && c.midi_ch.is_none() && d.len() >= 8 => {
                            c.midi_ch = Some(rd32(d, 4) as u8);
                            // "Map note color to MIDI channel" (Nuker 3, state v4; 0 in all v2..v7 samples)
                            c.color_map = d.get(29).is_some_and(|&x| x != 0);
                            // byte 30: program + 1, 0 = none (Gangnam FL 20.0: 0x52 -> program 81; ts6/Nuker 3: 1 -> 0)
                            c.program = d.get(30).and_then(|&x| x.checked_sub(1));
                            let k = midi_out_knobs(d).unwrap_or_default();
                            c.knobs = k.iter().map(|k| (k.0, k.1)).collect();
                            c.knob_range = k.iter().map(|k| k.2).collect();
                            c.knob_init = c.knobs.iter().map(|k| d.get(34 + 4 * k.0 as usize).map_or(0, |&x| x.min(127))).collect();
                        }
                        _ => {}
                    }
                }
                224 => {
                    let w = if major < 11 { 20 } else { 24 };
                    let notes = &mut p.patterns.entry(pat).or_default().notes;
                    // FL11+ u16 flags at +4: 0x0008 = slide note (bends the sounding note; FL's
                    // MIDI export leaves it out)
                    for r in d.chunks_exact(w).filter(|r| w == 20 || rd16(r, 4) & 8 == 0) {
                        // FL8: key u8 then fine,rel,..; FL11+: key u16, group u16, fine, ?, rel, ..
                        let (fine, rel, vel, color) = if w == 20 { (r[13], r[14], r[17], r[15]) } else { (r[16], r[18], r[21], r[19]) };
                        notes.push(Note { pos: rd32(r, 0), chan: rd16(r, 6), len: rd32(r, 8), key: r[12], vel, rel, fine, color: color & 15 });
                    }
                }
                // ponytail: 12-byte layout verified on FL2026 only
                223 => {
                    let ctrl = &mut p.patterns.entry(pat).or_default().ctrl;
                    for r in d.chunks_exact(12) {
                        ctrl.push(Ctrl { pos: rd32(r, 0), param: rd16(r, 4), chan: rd16(r, 6), val: rd32(r, 8) as i32 });
                    }
                }
                // automation clip points: u32 1, u32 64, u8 0, u32 4, u32 3, u32 count at 17,
                // then 24-byte points [f64 Δbeats][f64 value][f32 tension][u32 flags]
                234 if chan.is_some() && d.len() >= 21 => {
                    let n = rd32(d, 17) as usize;
                    let f = |o: usize| f64::from_le_bytes(d[o..o + 8].try_into().unwrap());
                    if d.len() >= 21 + 24 * n {
                        let mut beat = 0.0;
                        let tension = |k: usize| f32::from_le_bytes(d[37 + 24 * k..41 + 24 * k].try_into().unwrap()) as f64;
                        let pts: Vec<_> = (0..n).map(|k| { beat += f(21 + 24 * k); (beat, f(29 + 24 * k), tension(k), d[41 + 24 * k]) }).collect();
                        auto.insert(chan.unwrap() as u16, pts);
                        // then [u32 1][16 ff][6 u32][u32 m at +44][m x 24][u32][16 bytes][u32 speed]
                        // [i32 tension][i32 skew][i32 pw], knobs 0..128 (lfo_everything FL 12)
                        let o = 21 + 24 * n;
                        let r = o + 48 + 24 * d.get(o + 44..o + 48).map_or(0, |_| rd32(d, o + 44) as usize);
                        if d.len() >= r + 36 {
                            // LFO on: u32 -1 at r + 4, 0 off (Hypernova: on 27/35/44, off 45 with AMT set;
                            // event 212 byte 20 is 5 on both)
                            p.channels[chan.unwrap()].lfo_on = rd32(d, r + 4) == u32::MAX;
                            // knobs 0..128 (-1 = untouched); AMT 64 = 0: when_e's 64s add nothing in FL's export
                            let k = |o: usize| (rd32(d, o) as i32).max(0) as f64 / 128.0;
                            p.channels[chan.unwrap()].lfo = Some(Lfo {
                                period: 2f64.powf(4.0 * (65536.0 - rd32(d, r + 20) as f64) / (65536.0 - 45819.0)) / 16.0,
                                tension: k(r + 24),
                                skew: k(r + 28),
                                pw: k(r + 32),
                                amt: ((rd32(d, 4) as i32 as f64 - 64.0) / 64.0).clamp(-1.0, 1.0),
                                multiply: d[8] == 0,
                            });
                        }
                    }
                }
                // controller link: source address at 0 (automation channel), target at 8.
                // ponytail: tempo target [05 00][0x4000] seen in FL 12.3 only
                227 if d.len() >= 12 && d[8] == 5 && rd16(d, 10) == 0x4000 => tempo_src.push(rd16(d, 2)),
                227 if d.len() >= 12 => links.push((rd16(d, 2), rd16(d, 8), rd16(d, 10))),
                233 => {
                    chan = None; // channel section is over
                    playlists += 1;
                    if playlists > 1 {
                        p.warnings.push("multiple arrangements; only the first is used".into());
                        continue;
                    }
                    // ponytail: widths verified on 8.0, 11.1, 12.3, 20.x, 25.1, 26.1 only
                    let w = match major { 0..=10 => 28, 11..=24 => 32, 25 => 80, _ => 88 };
                    if d.len() % w != 0 {
                        p.warnings.push(format!("playlist size {} not a multiple of {w}", d.len()));
                    }
                    for r in d.chunks_exact(w) {
                        let (pos, len) = (rd32(r, 0), rd32(r, 8));
                        let mut untrimmed = false;
                        if w != 28 && rd16(r, 18) & 0x2000 == 0 {
                            starts.insert((rd16(r, 12), pos));
                        }
                        let (pattern, start, unhandled) = if w == 28 && rd16(r, 4) == 0x5000 && rd16(r, 6) > 0x5000 {
                            // FL8 free pattern clip: item = 0x5000 + pattern, i32 -1 trims (What I Found: 99 of them)
                            let st = rd32(r, 20);
                            (Some(rd16(r, 6) - 0x5000), 0, st != u32::MAX && f32::from_le_bytes(r[20..24].try_into().unwrap()) != -1.0)
                        } else if w == 28 && rd16(r, 4) == 0x5000 {
                            // FL8 channel item (audio / automation clip): item = channel, below the base
                            (None, 0, false)
                        } else if w == 28 {
                            // FL8 row item: playlist row == pattern, stored reversed from 0xE7 (empirical)
                            let st = f32::from_le_bytes(r[20..24].try_into().unwrap());
                            let en = f32::from_le_bytes(r[24..28].try_into().unwrap());
                            let t = |x: f32| !(x.is_nan() || x == -1.0);
                            (0xE7u16.checked_sub(r[12] as u16), 0, t(st) || t(en))
                        } else {
                            let (base, item) = (rd16(r, 4), rd16(r, 6));
                            // pattern clips: start/end offsets in pattern ticks; untrimmed is i32 -1
                            // (FL11..2025), f32 -1.0 (also seen in FL12) or 0/len (FL2026). The end
                            // needs no handling: the clip's length already cuts there.
                            let unset = |x: u32| x == u32::MAX || x == 0xBF80_0000;
                            let st = rd32(r, 24);
                            untrimmed = unset(rd32(r, 28));
                            let st = if unset(st) { 0 } else { st };
                            // u16 flags at +18: 0x2000 = clip muted (FL skips it on export)
                            let muted = rd16(r, 18) & 0x2000 != 0;
                            (item.checked_sub(base).filter(|_| item > base && !muted), st, false)
                        };
                        if unhandled {
                            p.warnings.push(format!("trimmed clip at {pos} not handled; using full clip"));
                        }
                        if let Some(pattern) = pattern {
                            // play = MAX marks an untrimmed clip for the block pass below
                            let track = if w == 28 { 0 } else { rd16(r, 12) };
                            p.clips.push(Clip { pos, len, pattern, start, play: if untrimmed { u32::MAX } else { len }, track });
                        } else if rd16(r, 6) < rd16(r, 4) && (w == 28 || rd16(r, 18) & 0x2000 == 0) {
                            // channel clip (automation/audio): start offset is f32 beats, -1 untrimmed
                            // (FL8: at 20; ponytail: FL8 mute flag not located)
                            let o = if w == 28 { 20 } else { 24 };
                            let st = f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                            chan_clips.push((pos, len, rd16(r, 6), if st > 0.0 { st as f64 } else { 0.0 }));
                        }
                    }
                }
                _ => {}
            }
        }
        // Empty playlist: FL plays the selected pattern (pattern mode), so export that, bar-rounded.
        // ponytail: one pass of the pattern; FL's own export of pattern mode not checked
        if p.clips.is_empty() {
            if let Some(pt) = p.patterns.get(&cur_pat) {
                let bar = (p.ppq as u32 * 4 * p.time_sig.0 as u32 / p.time_sig.1.max(1) as u32).max(1);
                let end = pt.notes.iter().map(|n| n.pos + n.len).max().unwrap_or(0);
                let len = end.div_ceil(bar).max(1) * bar;
                p.clips.push(Clip { pos: 0, len, pattern: cur_pat, start: 0, play: len, track: 0 });
                p.warnings.push(format!("empty playlist: playing the selected pattern {cur_pat} once"));
            }
        }
        // An automation channel's levels (219) are its clip's Min/Max knobs: pan = min, volume = max,
        // out = (min + v (max - min)) / 12800. Tempo = 10 + 512 out (FL 12's 10..522). Hypernova
        // 1250/12800: 0.270563 = 185, 10/33 = 200; Nuker 3 1250/4250: 0.75 = 150, 3.85 = 522.
        let out = |ch: u16, v: f64| {
            let c = &p.channels[ch as usize];
            (c.pan as f64 + v * (c.volume as f64 - c.pan as f64)) / 12800.0
        };
        // FL writes such tempo clips every tick when the points run past 1 (Nuker 3, collab), else
        // on 1/128 BPM steps (Hypernova). ponytail: told apart only by that (event 145 of the tempo
        // channel is 0 / 1 / 2 there, unconfirmed)
        p.tempo_legacy = tempo_src.iter().filter_map(|c| auto.get(c)).flatten().any(|p: &(f64, f64, f64, u8)| p.1 > 1.0);
        for &(pos, len, ch, start) in &chan_clips {
            if let (true, Some(pts)) = (tempo_src.contains(&ch), auto.get(&ch)) {
                let pts = pts.iter().map(|&(b, v, t, m)| (b, 10.0 + 512.0 * out(ch, v), t, m)).collect();
                p.tempo_clips.push(AutoClip { pos, len, start, pts, param: 5, target: 0x4000, src: ch });
            }
        }
        p.tempo_clips.sort_by_key(|c| c.pos);
        for &(pos, len, ch, start) in &chan_clips {
            for &(_, param, target) in links.iter().filter(|l| l.0 == ch) {
                if let Some(pts) = auto.get(&ch) {
                    p.auto_clips.push(AutoClip { pos, len, start, pts: pts.clone(), param, target, src: ch });
                }
            }
        }
        p.auto_clips.sort_by_key(|c| c.pos);
        // A pattern plays once at a time. A clip starting while an earlier clip of the same
        // pattern still plays (any playlist track, same position included) cuts it off; clips
        // of different patterns overlap freely (Nuker 3 FL 12, ts6 FL 20.0, collab FL 2025).
        let mut order: Vec<usize> = (0..p.clips.len()).collect();
        order.sort_by_key(|&i| (p.clips[i].pattern, p.clips[i].pos)); // stable: record order on ties
        for w in order.windows(2) {
            let (x, y) = (p.clips[w[0]], p.clips[w[1]]);
            if x.pattern == y.pattern && y.pos < x.pos + x.len {
                let c = &mut p.clips[w[0]];
                (c.len, c.play) = (y.pos - x.pos, y.pos - x.pos);
            }
        }
        // FL 12 plays an untrimmed clip past its end (rest of the pattern) unless another
        // clip starts right there on the same playlist track (Nuker 3, FL 12.3; FL 2026
        // cuts at the end, cctest). ponytail: overrun unbounded; Nuker 3's go 38 ticks at most.
        for c in p.clips.iter_mut().filter(|c| c.play == u32::MAX) {
            let free = major <= 12 && !starts.contains(&(c.track, c.pos + c.len));
            c.play = if free { u32::MAX - c.start } else { c.len };
        }
        // Echo delay with time 0 stacks 1 + echoes copies on the note (FL's "overmerge"), copy
        // k at velocity round(vel feed^(k/2)), velocity 0 included (Hypernova "Arts 11x").
        let echo = |c: u16| p.channels.get(c as usize).map(|c| c.echo).filter(|e| e.0 > 0.0 && e.1 > 0);
        if p.patterns.values().flat_map(|pt| &pt.notes).any(|n| echo(n.chan).is_some_and(|e| e.2 > 0)) {
            // ponytail: delayed echoes not written; unit of the time field unknown
            p.warnings.push("echo delay with time > 0: echoes not exported".into());
        }
        for pt in p.patterns.values_mut() {
            if pt.notes.iter().any(|n| echo(n.chan).is_some_and(|e| e.2 == 0)) {
                pt.notes = pt.notes.iter().flat_map(|&n| {
                    let (feed, k) = echo(n.chan).filter(|e| e.2 == 0).map_or((0.0, 0), |e| (e.0, e.1));
                    (0..=k).map(move |k| Note { vel: (n.vel as f64 * feed.powf(k as f64 / 2.0)).round() as u8, ..n })
                }).collect();
            }
            pt.notes.sort_by_key(|n| (n.chan, n.pos));
            pt.chans = pt.notes.chunk_by(|a, b| a.chan == b.chan).scan(0, |at, run| {
                *at += run.len();
                Some((run[0].chan, *at - run.len(), *at))
            }).collect();
            pt.ctrl.sort_by_key(|c| c.pos);
        }
        p.clips.sort_by_key(|c| c.pos);
        progress(total, total);
        Ok(p)
    }

    /// MIDI message for a pattern controller event, if it maps to one.
    pub fn ctrl_msg(&self, c: &Ctrl) -> Option<Msg> {
        match c.param {
            // ponytail: master pitch added to automated pitch as to the static one (unchecked)
            0x0004 => Some(Msg::PitchBend(bend(c.val + self.master_pitch))),
            p if p > 0x8000 => {
                let cc = self.channels.get(c.chan as usize)?.knobs.iter().find(|k| k.0 == p - 0x8001)?.1;
                Some(Msg::Cc { cc, val: c.val.clamp(0, 127) as u8 })
            }
            _ => None,
        }
    }

    /// Time-ordered stream of notes and controller changes; memory is bounded by
    /// sounding notes and clip count, not song size.
    pub fn events(&self) -> Events<'_> {
        self.events_of(None)
    }

    /// `events()` of one rack channel only; walks just that channel's notes, so each
    /// channel (MIDI track) can be produced on its own, in parallel.
    pub fn channel_events(&self, chan: u16) -> Events<'_> {
        self.events_of(Some(chan))
    }

    /// Ticks FL writes a playlist automation clip on, with the value written: FL holds the
    /// value in steps (1/65536 of the 0..1 range, channel pitch 1/16384, tempo 1/128 BPM), takes the value at the next
    /// tick, and writes whenever its step changes, and on the clip's first tick. Hypernova: 99.7%
    /// of a knob ramp's, 99.9% of a master pitch curve's, 98.8% of the tempo's ticks match FL.
    /// Output 0..1 of automation clip `c` at clip tick `k` (FL takes the value at k + 1): its curve,
    /// then its channel's LFO. Clips on that automation
    /// channel itself drive its Max (param 0, "Output max level") and LFO level (0x0604), holding
    /// their last value past their end (Hypernova "Main Vol #4").
    pub fn auto_value(&self, c: &AutoClip, k: u64) -> f64 {
        let nest: Vec<&AutoClip> = self.auto_clips.iter().filter(|m| m.target == c.src).collect();
        self.auto_value_in(c, k, &nest)
    }

    fn auto_value_in(&self, c: &AutoClip, k: u64, nest: &[&AutoClip]) -> f64 {
        let ch = self.channel(c.src);
        let beat = c.start + (k + 1) as f64 / self.ppq as f64;
        let v = c.value_at(beat);
        let tick = c.pos as u64 + k;
        let nested = |param: u16| {
            let m = nest.iter().filter(|m| m.param == param && m.pos as u64 <= tick).last()?;
            Some(self.auto_value(m, (tick - m.pos as u64).min(m.len.max(1) as u64 - 1)))
        };
        // Min/Max = the channel's levels (219 pan/volume), Max overridden by a nested clip. Not for
        // channel pitch: Hypernova "RISERS! - Channel pitch" (4800/8000) bends over the full range.
        let (lo, hi) = match c.param {
            4 => (0.0, 1.0),
            _ => (ch.pan as f64 / 12800.0, ch.volume as f64 / 12800.0),
        };
        let hi = nested(0).unwrap_or(hi);
        let mut out = lo + v * (hi - lo);
        if let Some(l) = ch.lfo.filter(|_| ch.lfo_on) {
            // nested knobs are bipolar: 2v - 1. ponytail: nested skew and pw shift FL's phase as they
            // move (lfo_everythingnested), not modeled
            let knob = |p: u16, x: f64| nested(p).map_or(x, |a| 2.0 * a - 1.0);
            let amt = knob(0x0604, l.amt);
            let x = l.at(beat * self.ppq as f64, self.ppq as f64, knob(0x0601, l.tension), knob(0x0602, l.skew), knob(0x0603, l.pw));
            // depth scales with the range (lfo_everythingnested: Max 0.5, value 1 -> 0.5 +- 0.25)
            let d = amt * (hi - lo) * x;
            out = if l.multiply { out * (1.0 + d) } else { out + d / 2.0 };
        }
        out.clamp(0.0, 1.0)
    }

    fn clip_writes(&self, c: &AutoClip, steps: f64) -> Vec<(u64, f64)> {
        let mut out = Vec::new();
        let mut last = None;
        let nest: Vec<&AutoClip> = self.auto_clips.iter().filter(|m| m.target == c.src).collect();
        for k in 0..c.len as u64 {
            // tempo clips hold BPM already (Min/Max applied at parse)
            let v = if c.target == 0x4000 && c.param == 5 { c.value_at(c.start + (k + 1) as f64 / self.ppq as f64) } else { self.auto_value_in(c, k, &nest) };
            let q = (v * steps).round();
            if last != Some(q) {
                out.push((c.pos as u64 + k, q / steps));
                last = Some(q);
            }
        }
        out
    }

    /// Controller events of the playlist automation clips, per rack channel, time-ordered.
    /// Volume -> CC7, pan -> CC10, MIDI Out controller k -> its CC (value: floor(v * 128)),
    /// channel pitch (+/- the channel's pitch range) and master pitch (+/- 1200 cents) -> pitch bend.
    fn auto_events(&self) -> &HashMap<u16, Vec<MidiEvent>> {
        self.auto_cache.get_or_init(|| {
            let mut out: HashMap<u16, Vec<MidiEvent>> = HashMap::new();
            if self.auto_clips.is_empty() {
                return out;
            }
            let master: Vec<(u64, f64)> = self.auto_clips.iter().filter(|c| c.target == 0x4000 && c.param == 2).flat_map(|c| self.clip_writes(c, 65536.0)).collect();
            for chan in self.used_channels() {
                let ch = self.channel(chan);
                // (tick, what, value): what 0 = master pitch, 1 = channel pitch, 2 = CC number + 2
                let mut w: Vec<(u64, u16, f64)> = master.iter().map(|&(t, v)| (t, 0, v)).collect();
                for c in self.auto_clips.iter().filter(|c| c.target == chan) {
                    let what = match c.param {
                        0 => 2 + 7,
                        1 => 2 + 10,
                        4 => 1,
                        p if p > 0x8000 => match ch.knobs.iter().find(|k| k.0 == p - 0x8001) {
                            Some(&(_, k)) => 2 + k as u16,
                            None => continue,
                        },
                        _ => continue,
                    };
                    // channel pitch steps are 1/16384 (Hypernova's riser: a write every ~1.88 ticks)
                    let steps = if what == 1 { 16384.0 } else { 65536.0 };
                    w.extend(self.clip_writes(c, steps).into_iter().map(|(t, v)| (t, what, v)));
                }
                if w.is_empty() {
                    continue;
                }
                w.sort_by_key(|x| x.0);
                let (mut pitch, mut mp) = (ch.pitch, self.master_pitch);
                let evs = out.entry(chan).or_default();
                for (tick, what, v) in w {
                    let msg = match what {
                        0 | 1 => {
                            let cents = ((v - 0.5) * 2.0 * if what == 0 { 1200.0 } else { ch.pitch_range as f64 * 100.0 }).round() as i32;
                            *(if what == 0 { &mut mp } else { &mut pitch }) = cents;
                            Msg::PitchBend(bend(pitch + mp))
                        }
                        n => {
                            let cc = (n - 2) as u8;
                            Msg::Cc { cc, val: ch.knob_cc(ch.knobs.iter().position(|k| k.1 == cc), v) }
                        }
                    };
                    evs.push(MidiEvent { tick, chan, msg, color: 0 });
                }
            }
            out
        })
    }

    fn events_of(&self, only: Option<u16>) -> Events<'_> {
        let auto: Vec<MidiEvent> = match only {
            Some(c) => self.auto_events().get(&c).cloned().unwrap_or_default(),
            None => {
                let mut v: Vec<MidiEvent> = self.auto_events().values().flatten().copied().collect();
                v.sort_by_key(|e| (e.tick, e.chan));
                v
            }
        };
        let (mut ons, mut pats) = (BinaryHeap::new(), Vec::with_capacity(self.clips.len()));
        for (ci, c) in self.clips.iter().enumerate() {
            pats.push(self.patterns.get(&c.pattern));
            let Some(pt) = self.patterns.get(&c.pattern) else { continue };
            // first event at or after the clip's start offset (lists are sorted by pos)
            let i = pt.ctrl.partition_point(|x| x.pos < c.start);
            if let Some(t) = pt.ctrl.get(i).and_then(|x| c.at(x.pos)) {
                ons.push(Reverse((t, 0u8, ci, i, pt.ctrl.len())));
            }
            for &(_, a, b) in pt.chans.iter().filter(|r| only.is_none_or(|o| o == r.0)) {
                let i = a + pt.notes[a..b].partition_point(|n| n.pos < c.start);
                if let Some(t) = pt.notes[i..b].first().and_then(|n| c.at(n.pos)) {
                    ons.push(Reverse((t, 1u8, ci, i, b)));
                }
                // FL plays notes the left trim cuts through from the clip start, shortened
                // (Nuker 3 FL 12, a collab FL 2025 project)
                if c.start > 0 && c.len > 0 {
                    for j in (a..i).filter(|&j| pt.notes[j].pos + pt.notes[j].len > c.start) {
                        ons.push(Reverse((c.pos as u64, 2u8, ci, j, 0)));
                    }
                }
            }
        }
        Events { p: self, pats, ons, offs: BinaryHeap::new(), only, auto: auto.into_iter().peekable() }
    }

    /// Rack channels FL exports, in rack order: those that play notes, and every MIDI Out.
    pub fn used_channels(&self) -> Vec<u16> {
        let mut s = BTreeSet::new();
        for c in &self.clips {
            let notes = self.patterns.get(&c.pattern).map(|p| &p.notes[..]).unwrap_or_default();
            s.extend(notes.iter().filter(|n| c.at(n.pos).is_some()).map(|n| n.chan));
        }
        // FL exports every MIDI Out channel, notes or not (when_e, a collab FL 2025 project)
        s.extend((0..self.channels.len() as u16).filter(|&i| self.channels[i as usize].plugin == "MIDI Out"));
        s.into_iter().collect()
    }

    /// Last playlist clip end, automation clips included (Hypernova: its tempo clip ends 5760 ticks
    /// after the last pattern clip, and FL's export runs that long).
    fn song_end(&self) -> u64 {
        // automation clips count, tempo clips don't (Hypernova re-exported in FL 12.3 ends at 797040,
        // its tempo clip at 798720)
        let ends = self.clips.iter().map(|c| (c.pos, c.len)).chain(self.auto_clips.iter().map(|c| (c.pos, c.len)));
        ends.map(|(p, l)| p as u64 + l as u64).max().unwrap_or(0)
    }

    /// FL renders 30 s past the song end, in 512-sample blocks at 44.1 kHz.
    /// ponytail: block size/rate inferred from two songs (256 and 1024 also fit).
    fn tail_tick(&self) -> u64 {
        if self.cut_remainder {
            return self.song_end() + 1;
        }
        // FL 12 adds no tail: song end + 2 (Hypernova re-exported in FL 12.3, lfo_everything).
        // ponytail: Nuker 3's reference ends at + 1; its exporter's FL version is unknown
        if self.version.split('.').next().and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v <= 12) {
            return self.song_end() + 2;
        }
        let den = (self.tempo * 1000.0).round() as u128 * self.ppq as u128; // ticks -> samples: x * K / den
        const K: u128 = 44_100 * 60 * 1000;
        let samples_num = self.song_end() as u128 * K + 30 * 44_100 * den;
        let blocks = samples_num.div_ceil(den * 512);
        (blocks * 512 * den / K) as u64
    }

    /// Controller value FL chases at song start: the event at tick 0, else the
    /// pattern's last event (it loops), else 0.
    fn chased(&self, chan: u16, param: u16) -> Option<u8> {
        let mut v = None;
        for c in self.clips.iter().filter(|c| c.pos == 0) {
            let Some(pt) = self.patterns.get(&c.pattern) else { continue };
            let mut it = pt.ctrl.iter().filter(|x| x.chan == chan && x.param == param && c.at(x.pos).is_some());
            let at0 = it.clone().filter(|x| x.pos == c.start).last();
            if let Some(x) = at0.or_else(|| it.next_back()) {
                v = Some(x.val.clamp(0, 127) as u8);
            }
        }
        v
    }

    /// Tempo per tick as FL exports it: (tick, BPM) change points, project tempo first.
    /// Inside a tempo clip: the value at the next tick in 1/128 BPM steps (as whole milli-BPM;
    /// Hypernova: within 4 us of FL, 62% exact), on the ticks
    /// `clip_writes` picks (every tick on Nuker 3's 96 PPQ, only on 16-bit steps at 960).
    /// Tempos are per quarter note: FL counts beats in the time signature's denominator,
    /// so a 4/8 project exports at twice its BPM (Hypernova: 315 -> 630).
    fn tempo_points(&self) -> Vec<(u64, f64)> {
        let f = self.time_sig.1.max(1) as f64 / 4.0;
        let mut ev = vec![(0u64, self.tempo * f)];
        for c in &self.tempo_clips {
            // steps of FL 12's 60..522 range (Hypernova: tempo events only every 6-7 us of change)
            if self.tempo_legacy {
                for k in 0..c.len {
                    let bpm = c.value_at(c.start + (k + 1) as f64 / self.ppq as f64);
                    ev.push((c.pos as u64 + k as u64, (bpm * f * 1000.0).round() / 1000.0));
                }
                continue;
            }
            for (t, bpm) in self.clip_writes(c, 128.0) {
                ev.push((t, (bpm * f * 1000.0).round() / 1000.0));
            }
        }
        ev
    }

    /// Tempo track data. With tempo automation FL writes the project tempo twice at tick 0,
    /// then the tempo clips' events (`tempo_points`), and the project tempo again one bar after
    /// the song end.
    /// ponytail: ~2% of events land 1 milli-BPM off FL's (rounding ties); 1 song checked.
    fn tempo_track(&self, song_end: u64) -> Vec<u8> {
        let us = |bpm: f64| (60_000_000.0 / bpm).round() as u32;
        let base = us(self.tempo * self.time_sig.1.max(1) as f64 / 4.0);
        let mut ev: Vec<(u64, u32)> = self.tempo_points().into_iter().map(|(t, b)| (t, us(b))).collect();
        let fl12 = self.version.split('.').next().and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v <= 12);
        if fl12 && !self.tempo_clips.is_empty() {
            // FL 12 (Hypernova): first its 522 BPM maximum, no closing tempo, nothing past the song end.
            // ponytail: one sample; the 522 may be where FL 12 parks a clip-linked tempo before playback
            ev.retain(|e| e.0 <= song_end);
            ev.insert(0, (0, us(522.0 * self.time_sig.1.max(1) as f64 / 4.0)));
        } else if !self.tempo_clips.is_empty() {
            ev.insert(0, (0, base));
            let bar = self.ppq as u64 * 4 * self.time_sig.0 as u64 / self.time_sig.1.max(1) as u64;
            ev.push(((song_end / bar + 1) * bar, base));
        }
        let (mut out, mut last) = (Vec::with_capacity(ev.len() * 7), 0);
        for (t, u) in ev {
            vlq(&mut out, (t - last.min(t)) as u32);
            out.extend([0xff, 0x51, 3, (u >> 16) as u8, (u >> 8) as u8, u as u8]);
            last = last.max(t);
        }
        out
    }

    /// Standard MIDI File, type 1, laid out like FL 20.6+'s exporter:
    /// track 0 time signature, track 1 tempo, then one named track per used channel
    /// with FL's reset blocks around the song.
    pub fn to_smf(&self) -> Vec<u8> {
        self.smf_pieces().concat()
    }

    /// `to_smf`'s bytes in pieces whose concatenation is exactly it (header, then each
    /// track's chunk header and data), so a reader can free each piece once read instead
    /// of holding a second copy of the whole file.
    pub fn smf_pieces(&self) -> Vec<Vec<u8>> {
        self.smf_pieces_progress(&mut |_, _, _| {})
    }

    /// `smf_pieces`, calling `progress(done, total, notes written)` about every 50 ms while
    /// the tracks are written (the slow part; one thread per track), then `(total, total, notes)`.
    pub fn smf_pieces_progress(&self, progress: &mut dyn FnMut(u64, u64, u64)) -> Vec<Vec<u8>> {
        use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
        let chans = self.used_channels();
        let end = self.song_end().max(1);
        let (ticks, notes): (Vec<AtomicU64>, Vec<AtomicU64>) = chans.iter().map(|_| (AtomicU64::new(0), AtomicU64::new(0))).unzip();
        let total = end * chans.len() as u64;
        let sum = |v: &[AtomicU64]| v.iter().map(|x| x.load(Relaxed)).sum::<u64>();
        // ponytail: one thread per used channel, unbounded; a pool if projects with hundreds appear
        let tracks: Vec<Vec<u8>> = std::thread::scope(|s| {
            let jobs: Vec<_> = chans
                .iter()
                .enumerate()
                .map(|(i, &c)| {
                    let (ticks, notes) = (&ticks[i], &notes[i]);
                    s.spawn(move || {
                        let mut r = self.track_reader(c);
                        let mut out = Vec::new();
                        let mut chunk = vec![0; 1 << 20];
                        loop {
                            let n = std::io::Read::read(&mut r, &mut chunk).unwrap(); // in memory: never fails
                            if n == 0 {
                                break;
                            }
                            out.extend_from_slice(&chunk[..n]);
                            ticks.store(r.tick().min(end), Relaxed);
                            notes.store(r.notes(), Relaxed);
                        }
                        ticks.store(end, Relaxed);
                        out.shrink_to_fit();
                        out
                    })
                })
                .collect();
            while !jobs.iter().all(|j| j.is_finished()) {
                progress(sum(&ticks), total, sum(&notes));
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            jobs.into_iter().map(|j| j.join().unwrap()).collect()
        });
        progress(total, total, sum(&notes));

        let mut pieces = vec![self.smf_header(chans.len())];
        for t in [self.time_sig_track(), self.tempo_track_data()].into_iter().chain(tracks) {
            pieces.push([&b"MTrk"[..], &(t.len() as u32).to_be_bytes()].concat());
            pieces.push(t);
        }
        pieces
    }

    /// The SMF header chunk for `channels` note tracks (plus the time signature and tempo tracks).
    pub fn smf_header(&self, channels: usize) -> Vec<u8> {
        let mut out = b"MThd".to_vec();
        out.extend(6u32.to_be_bytes());
        out.extend(1u16.to_be_bytes());
        out.extend((channels as u16 + 2).to_be_bytes());
        out.extend(self.ppq.to_be_bytes());
        out
    }

    /// Track 0's data (end of track included): the time signature.
    pub fn time_sig_track(&self) -> Vec<u8> {
        // FL writes 4 - log2(denominator): 4/4 -> 2, 4/8 -> 1 (Hypernova, FL 12.3 and newer)
        // ponytail: only denominators 4 and 8 seen; 2 and 16 follow the same formula unchecked
        let den_pow = 4u8.saturating_sub(self.time_sig.1.max(1).trailing_zeros() as u8);
        vec![0, 0xff, 0x58, 4, self.time_sig.0, den_pow, 24, 8, 0, 0xff, 0x2f, 0]
    }

    /// Track 1's data (end of track included): the tempo map.
    pub fn tempo_track_data(&self) -> Vec<u8> {
        let mut t = self.tempo_track(self.song_end());
        t.extend([0, 0xff, 0x2f, 0]);
        t
    }

    /// The note track of rack channel `chan` (one of `used_channels()`) as MTrk data bytes,
    /// made while it is read: memory stays at the sounding notes, so a player can stream a
    /// project of any size. Exactly the bytes `to_smf` has for that track.
    pub fn track_reader(&self, chan: u16) -> TrackReader<'_> {
        let tick0_ctrl = |ev: &MidiEvent| ev.tick == 0 && matches!(ev.msg, Msg::Cc { .. } | Msg::PitchBend(_));
        let at_zero: Vec<Msg> = self.channel_events(chan).take_while(|ev| ev.tick == 0).filter(tick0_ctrl).map(|ev| ev.msg).collect();
        let mut r = TrackReader {
            p: self,
            c: chan,
            m: self.channel(chan).midi_ch.unwrap_or(0),
            events: self.channel_events(chan),
            buf: Vec::new(),
            at: 0,
            last: 0,
            knob_vals: self.knob_start(chan),
            bend: at_zero.iter().rev().find(|m: &&Msg| matches!(m, Msg::PitchBend(_))).copied(),
            at_zero,
            notes: 0,
            done: false,
        };
        // FL writes names one byte per character (when_e: Ç = c7, ´ = b4)
        // ponytail: Latin-1, '?' above U+00FF; check what FL does with those if a name has one
        let name: Vec<u8> = self.channel(chan).name.chars().map(|c| u8::try_from(c as u32).unwrap_or(b'?')).collect();
        r.buf.extend([0, 0xff, 0x03]);
        vlq(&mut r.buf, name.len() as u32);
        r.buf.extend(&name);
        r.put_block(0, &self.reset_head(chan));
        // FL < 20 (Ascension, Hypernova in FL 12.3): B B at-zero B; FL 20+: B at-zero B at-zero (B)
        let once = self.pre20_6() && !self.fl20_0();
        for i in 0..2 {
            r.put_block(0, &self.reset_block(chan, &r.knob_vals.clone()));
            if !once || i == 1 {
                r.put_block(0, &r.at_zero.clone());
            }
        }
        if self.fl20_0() {
            return r; // FL 20.0: two head blocks, the third goes to the tail
        }
        r.knob_vals = self.channel(chan).knobs.iter().zip(self.knob_start(chan)).map(|(k, v)| self.chased(chan, 0x8001 + k.0).unwrap_or(v)).collect();
        r.put_block(0, &self.reset_block(chan, &r.knob_vals.clone()));
        r
    }

    /// FL 20.0..20.5 lay out the reset blocks differently from FL 20.6+ (see FORMAT.md).
    fn fl20_0(&self) -> bool {
        let v: Vec<u32> = self.version.split('.').map(|s| s.parse().unwrap_or(0)).collect();
        v[0] == 20 && v.get(1).copied().unwrap_or(0) < 6
    }

    /// FL before 20.6 writes the knobs' start values and the bend in every reset block
    /// (Ascension and Hypernova re-exported in FL 12.3, Gangnam in FL 20.0).
    fn pre20_6(&self) -> bool {
        self.fl20_0() || self.version.split('.').next().and_then(|s| s.parse::<u32>().ok()).is_some_and(|v| v < 20)
    }

    fn channel(&self, c: u16) -> &Channel {
        static BLANK: std::sync::OnceLock<Channel> = std::sync::OnceLock::new();
        self.channels.get(c as usize).unwrap_or_else(|| BLANK.get_or_init(|| Channel { pan: 6400, volume: 10000, pitch_range: 2, ..Default::default() }))
    }

    /// H: pan, volume, static pitch bend.
    fn reset_head(&self, c: u16) -> Vec<Msg> {
        let ch = self.channel(c);
        vec![Msg::Cc { cc: 10, val: Channel::cc(ch.pan) }, Msg::Cc { cc: 7, val: Channel::cc(ch.volume) }, Msg::PitchBend(bend(ch.pitch + self.master_pitch))]
    }

    /// Knob values of the head blocks: FL before 20.6 writes the state's, or the start value of a
    /// clip at tick 0 (Gangnam, Ascension, Hypernova re-exported in FL 12.3); later versions 0.
    fn knob_start(&self, c: u16) -> Vec<u8> {
        let ch = self.channel(c);
        if !self.pre20_6() {
            return vec![0; ch.knobs.len()];
        }
        // a clip at tick 0 sets the knob first (Gangnam); later clips don't (Hypernova CC74 at 7680)
        let first = |k: u16| self.auto_clips.iter().find(|a| a.target == c && a.param == 0x8001 + k && a.pos == 0);
        ch.knobs.iter().enumerate().map(|(i, k)| first(k.0).map_or(ch.knob_init[i], |a| ch.knob_cc(Some(i), self.auto_value(a, 0)))).collect()
    }

    /// B(v): RPN pitch range, pan, volume, then knob values (or static pitch bend if none).
    fn reset_block(&self, c: u16, vals: &[u8]) -> Vec<Msg> {
        let ch = self.channel(c);
        let head = self.reset_head(c);
        let mut v = vec![Msg::Cc { cc: 101, val: 0 }, Msg::Cc { cc: 100, val: 0 }, Msg::Cc { cc: 6, val: (BEND_RANGE_CENTS / 100) as u8 }];
        if self.pre20_6() {
            // FL < 20.6: RPN, pan, volume, bend, program, then the knobs (Gangnam, Ascension)
            v.extend(&head[..3]);
            v.extend(ch.program.map(|p| Msg::Program(p & 127)));
            v.extend(ch.knobs.iter().zip(vals).map(|(&(_, cc), &x)| Msg::Cc { cc, val: x }));
            return v;
        }
        v.extend(&head[..2]);
        if ch.knobs.is_empty() {
            v.push(head[2]);
        } else {
            v.extend(ch.knobs.iter().zip(vals).map(|(&(_, cc), &x)| Msg::Cc { cc, val: x }));
        }
        v.extend(ch.program.map(|p| Msg::Program(p & 127)));
        v
    }
}

/// One note track of `Project::to_smf`, generated as it is read (see `Project::track_reader`).
pub struct TrackReader<'a> {
    p: &'a Project,
    c: u16,
    m: u8, // MIDI Out channel (reset blocks; notes may go out on their color's)
    events: Events<'a>,
    buf: Vec<u8>,
    at: usize,
    last: u64,
    knob_vals: Vec<u8>,
    bend: Option<Msg>, // last pitch bend written (FL 20.0's tail blocks carry it)
    at_zero: Vec<Msg>, // controller events at song tick 0 (written in the head, not again)
    notes: u64,
    done: bool,
}

impl TrackReader<'_> {
    /// Song tick written up to.
    pub fn tick(&self) -> u64 {
        self.last
    }

    /// Note-ons written so far.
    pub fn notes(&self) -> u64 {
        self.notes
    }

    fn put(&mut self, m: u8, tick: u64, msg: Msg) {
        vlq(&mut self.buf, (tick - self.last) as u32);
        self.buf.extend(&msg.bytes(m)[..msg.len()]);
        self.last = tick;
    }

    /// `msgs` at `tick` (the first carries the delta, the rest 0).
    fn put_block(&mut self, tick: u64, msgs: &[Msg]) {
        for (i, &msg) in msgs.iter().enumerate() {
            self.put(self.m, if i == 0 { tick } else { self.last }, msg);
        }
        self.last = self.last.max(tick);
    }

    /// The next ~64 KB of events into `buf`, or at the end the closing blocks.
    fn fill(&mut self) {
        while self.buf.len() < 1 << 16 {
            let Some(ev) = self.events.next() else {
                let (song_end, tail) = (self.p.song_end(), self.p.tail_tick());
                let vals = self.knob_vals.clone();
                if self.p.pre20_6() {
                    // B(end), B(tail), B(tail), with the current pitch bend
                    let mut b = self.p.reset_block(self.c, &vals);
                    if let (Some(pb), Some(x)) = (self.bend, b.iter_mut().find(|m| matches!(m, Msg::PitchBend(_)))) {
                        *x = pb;
                    }
                    self.put_block(song_end.max(self.last), &b);
                    self.put_block(tail.max(self.last), &b);
                    self.put_block(tail.max(self.last), &b);
                    self.buf.extend([0, 0xff, 0x2f, 0]);
                    self.done = true;
                    return;
                }
                self.put_block(song_end.max(self.last), &self.p.reset_block(self.c, &vals));
                if !self.at_zero.is_empty() {
                    self.put_block((tail - 1).max(self.last), &self.at_zero.clone());
                }
                self.put_block(tail.max(self.last), &self.p.reset_block(self.c, &vals));
                self.buf.extend([0, 0xff, 0x2f, 0]); // end of track at the last event
                self.done = true;
                return;
            };
            if ev.tick == 0 && matches!(ev.msg, Msg::Cc { .. } | Msg::PitchBend(_)) {
                continue; // already in the head
            }
            let mut ev = ev;
            if let Msg::NoteOn { vel: ref mut v @ 0, .. } = ev.msg {
                if self.p.zero_vel_as_one {
                    *v = 1;
                }
            }
            match ev.msg {
                Msg::Cc { cc, val } => {
                    if let Some(k) = self.p.channel(self.c).knobs.iter().position(|k| k.1 == cc) {
                        self.knob_vals[k] = val;
                    }
                }
                Msg::NoteOn { .. } => self.notes += 1,
                Msg::PitchBend(_) => self.bend = Some(ev.msg),
                _ => {}
            }
            self.put(self.p.midi_channel(&ev), ev.tick, ev.msg);
        }
    }
}

impl std::io::Read for TrackReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.at == self.buf.len() {
            if self.done {
                return Ok(0);
            }
            self.buf.clear();
            self.at = 0;
            self.fill();
        }
        let n = out.len().min(self.buf.len() - self.at);
        out[..n].copy_from_slice(&self.buf[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

fn vlq(out: &mut Vec<u8>, v: u32) {
    let mut bytes = [(v & 0x7f) as u8; 5];
    let (mut n, mut v) = (1, v >> 7);
    while v > 0 {
        bytes[n] = (v & 0x7f) as u8 | 0x80;
        v >>= 7;
        n += 1;
    }
    out.extend(bytes[..n].iter().rev());
}

pub struct Events<'a> {
    p: &'a Project,
    pats: Vec<Option<&'a Pattern>>, // each clip's pattern (only clips with one are queued)
    // (tick, 0=ctrl/1=note/2=note cut by left trim, clip, index, end of the index's run)
    ons: BinaryHeap<Reverse<(u64, u8, usize, usize, usize)>>,
    offs: BinaryHeap<Reverse<(u64, u16, u8, u8, u8)>>, // (tick, chan, key, release, color)
    only: Option<u16>, // channel_events: the one channel
    auto: std::iter::Peekable<std::vec::IntoIter<MidiEvent>>, // playlist automation (Project::auto_events)
}

impl Iterator for Events<'_> {
    type Item = MidiEvent;
    fn next(&mut self) -> Option<MidiEvent> {
        loop {
            let next_on = self.ons.peek().map(|r| r.0 .0);
            let next_off = self.offs.peek().map(|r| r.0 .0);
            // same tick: note-offs, then automation, then note-ons
            if let Some(a) = self.auto.peek() {
                if next_off.is_none_or(|t| a.tick < t) && next_on.is_none_or(|t| a.tick <= t) {
                    return self.auto.next();
                }
            }
            if let Some(&Reverse((tick, chan, key, vel, color))) = self.offs.peek() {
                if next_on.map_or(true, |t| tick <= t) {
                    self.offs.pop(); // offs first on the same tick, like FL
                    return Some(MidiEvent { tick, chan, msg: Msg::NoteOff { key, vel }, color });
                }
            }
            let Reverse((tick, kind, ci, i, end)) = self.ons.pop()?;
            let clip = self.p.clips[ci];
            let pt = self.pats[ci].unwrap();
            if kind == 0 {
                let x = pt.ctrl[i];
                if let Some(t) = pt.ctrl.get(i + 1).and_then(|m| clip.at(m.pos)) {
                    self.ons.push(Reverse((t, 0, ci, i + 1, end)));
                }
                // ponytail: channel_events still walks every channel's controller events
                if self.only.is_some_and(|o| o != x.chan) {
                    continue;
                }
                match self.p.ctrl_msg(&x) {
                    Some(msg) => return Some(MidiEvent { tick, chan: x.chan, msg, color: 0 }),
                    None => continue,
                }
            }
            let n = pt.notes[i];
            let key = n.midi_key();
            // FL ends notes still held when their clip stops at its length (Nuker 3 FL 12,
            // collab FL 2025). ponytail: overrunning clips not cut (unknown where)
            let stop = if clip.play == clip.len { clip.pos as u64 + clip.len as u64 } else { u64::MAX };
            if kind == 2 {
                // note cut by the left trim: sounds from the clip start for what's left
                let left = (n.pos + n.len - clip.start) as u64;
                self.offs.push(Reverse(((tick + left).min(stop), n.chan, key, n.rel, n.color)));
                return Some(MidiEvent { tick, chan: n.chan, msg: Msg::NoteOn { key, vel: n.vel }, color: n.color });
            }
            if let Some(t) = pt.notes[i + 1..end].first().and_then(|m| clip.at(m.pos)) {
                self.ons.push(Reverse((t, 1, ci, i + 1, end)));
            }
            self.offs.push(Reverse(((tick + n.len as u64).min(stop), n.chan, key, n.rel, n.color)));
            return Some(MidiEvent { tick, chan: n.chan, msg: Msg::NoteOn { key, vel: n.vel }, color: n.color });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_event_sizes() {
        let mut f = b"FLhd".to_vec();
        f.extend(6u32.to_le_bytes());
        f.extend([0, 0, 3, 0, 96, 0]);
        let body = [
            vec![10, 0x7f],                // byte
            vec![65, 0x34, 0x12],          // word
            vec![156, 0x10, 0x27, 0, 0],   // dword
            [vec![199, 0x81, 0x01], vec![b'x'; 129]].concat(), // varint len 129
        ]
        .concat();
        f.extend(b"FLdt");
        f.extend((body.len() as u32).to_le_bytes());
        f.extend(&body);

        let (h, ev) = parse(&f).unwrap();
        assert_eq!((h.channels, h.ppq), (3, 96));
        let ids: Vec<u8> = ev.iter().map(|e| e.id).collect();
        assert_eq!(ids, [10, 65, 156, 199]);
        assert_eq!(ev[1].value(), 0x1234);
        assert_eq!(ev[2].value(), 10000);
        assert_eq!(ev[3].data.len(), 129);
    }
}
