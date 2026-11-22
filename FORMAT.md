# FLP version map (empirical)

Source: `samples/newstuffFromFL8MIDI(*Export).flp`, one FL8 project re-saved through every version.
All 8 FL MIDI exports have identical notes (1016 notes, 128 BPM, ppq 96). FL8's export has 4 tracks, later ones have 5.

## Container (all versions)
`FLhd` (len 6: format i16, channels u16, ppq u16) then `FLdt` (len u32) + events.
Size by ID: 0-63 = 1 byte, 64-127 = 2, 128-191 = 4, 192-255 = varint length + blob.
The first event is always 199 (FLVersion text). Branch on it.

**Exception, FL 25.2.3+:** ID 172 has a 3-byte payload (`01 01 00`, `00 01 00`). Read as 4 it desyncs the whole stream.

## Text encoding
8-bit through FL11, UTF-16LE from FL12. Channel name: 192 (<= FL11), 203 (FL12+).

## Tempo
| Version | Event |
|---|---|
| FL8 | 66 (word) = BPM |
| FL11+ | 156 (dword) = BPM * 1000 |

## Pattern notes (224): record size
| Version | Size | Layout |
|---|---|---|
| FL8 | 20 | pos u32, flags u16, chan u16, len u32, key u8, fine u8, release u8, midich u8, pan u8, vel u8, modx u8, mody u8 |
| FL11-26 | 24 | pos u32, flags u16, chan u16, len u32, key u16, group u16, fine u8, ?u8, release u8, midich u8, pan u8, vel u8, modx u8, mody u8 |

Both versions hold the same 532 pattern notes.

## Playlist items (233): record size (13 items)
| Version | Size | Notes |
|---|---|---|
| FL8 | 28 | different layout; start/end offset as f32 (`00 00 80 bf` = -1.0) |
| FL11-20.9 | 32 | pos u32, pattern_base u16 (0x5000), item u16 (pattern = item - base), len u32, track u16 (reversed), ... start/end i32 (-1 = untrimmed) |
| FL2025 | 80 | 32-byte FL11 layout + 48 bytes (contains f32 1.0, f64 1.0) |
| FL2026 | 88 | FL2025 + 8 bytes (`ff ff ff ff 00 00 00 00`) |

Track index base: FL11 `0xc2..` vs FL2025+ `0x1ef..`. The reversed-index base (max track count) changed.

## Registration text (200)
Present in FL8, FL11, FL12, FL20.0 and FL2026. Empty (`""`) in FL20.6 and FL20.9.1 (trial), placeholder `"    "` in FL2025.
Not needed for MIDI; the parser ignores it.

## Patterns, channels, MIDI mapping (verified: all 8 exports match note-for-note)
- `65` (pattern select) precedes that pattern's `224` notes. `64` (channel select) precedes that channel's `201`/`192`/`203`/`212`/`213`.
- Mixer-effect plugin events (`201`, `213`) also appear after the playlist, so only the first value after `64` belongs to the channel.
- MIDI Out channel = u32 at byte 4 of its `213` state (state version u32 at 0: 2 in FL8, 7 in FL2026).
- MIDI Out "Map note color to MIDI channel" = byte 29 of the state (≠ 0 on; Nuker 3, v4). Notes then go out on
  the MIDI channel of their color group: byte 19 of the 24-byte note (FL8: byte 15). Controllers stay on the
  MIDI Out channel. Matches FL's Nuker 3 export channel for channel. **FL 12 adds the MIDI Out channel**, clamped
  to 15 (Hypernova "Arts 11x": ch 3 + color 13 = 15); FL 20.0 (ts6, ch 15) and FL 2025 (collab, ch 12) use the
  color alone. Only one FL 12 sample with a nonzero channel, and that channel also has echo delay on.
- **Echo delay** (event 209, per channel): `[feed i32 (/25600)][pan i32][pitch i32][echoes i32][time i32]`. With
  time 0 and feed > 0, FL writes 1 + echoes copies of every note on the same tick, copy k at velocity
  `round(vel · feed^(k/2))`, velocity-0 copies included, each with its own note-off (Hypernova "Arts 11x": 6700,
  10 echoes, velocity 4 → 4 2 1 1 0 0 0 0 0 0 0). Time > 0 is not exported (unit unknown, no sample).
- MIDI Out byte 30 ≠ 0: a program change (`Cn 00`) follows every reset block `B` (ts6 FL 20.0, Nuker 3 FL 12; 0 in
  every sample). Only program 0 seen; where the number lives is unknown.
- **FL exports every MIDI Out channel**, with or without notes (an empty one is just its reset blocks); other
  generators only when they play notes (when_e: 3x Osc with unplaced notes skipped).
- Track names are one byte per character (Latin-1: `Ç` = `c7`), not UTF-8.
- Note flags, u16 at +4 of the 24-byte note: `0x0008` = slide note. **Slide notes are not exported** (ts6 FL 20.0:
  8,722 per clip). `0x4000` is always set.
- FL key = MIDI key. Note velocity is raw, and the note-off velocity is the note's release (default 64).
- Clip plays pattern notes with `start <= pos < start + clip.len`, at `clip.pos + note.pos - start`.
- FL8 playlist: pattern = `0xE7 - byte[12]` (row == pattern). FL11+: pattern = `item - base` when `item > base`.
- Playlist record (FL11+): +12 u16 track, +18 u16 flags (`0x2000` = muted, not exported), +24/+28 start/end
  offsets in pattern ticks. Untrimmed = i32 −1 **or f32 −1.0 (`00 00 80 bf`, FL 12)** or 0/len (FL 2026).
  Channel clips (`item < base`, item = channel) have f32 beat offsets.

### Playback rules (Nuker 3 FL 12.3, ts6 FL 20.0, when_e FL 20.7, collab FL 2025: all note-ons match)
- **A pattern plays once at a time** (all versions). A clip starting while an earlier clip of the same pattern still plays (on any
  track, same position included) cuts the earlier one there. Copies of a clip on several tracks play once. Clips of
  different patterns overlap freely.
- **Untrimmed clips overrun.** With end offset −1 and no clip starting exactly at its end on the same playlist track,
  a clip keeps playing the pattern's notes past its length (Nuker 3: up to 38 ticks). Clips with an explicit end
  offset cut exactly. FL 12 only: FL 20.0 (ts6) and FL 2026 (cctest) cut at the end.
- **Left trim through a note** (all versions): a note that starts before the trim point and is still held there plays
  from the clip start, shortened.
- **Notes held at the clip end** (clip stopping at its length) get their note-off at the clip end (all versions).
- Not matched yet (Nuker 3): a few hundred note-offs 1–76 ticks off at clip edges.
- **Same-tick note-off order** differs from every FL version (no sample is byte-exact): FL 20.0 and 2026 order the
  same project differently, and no FIFO, LIFO or global heap queue reproduces either. Playback is unaffected.

### Playlist automation clips (Hypernova FL 12.3, Nuker 3, collab FL 2025, Aimonortsa FL 20.0)
- 227 link: source channel u16 at 2, param u16 at 8, target u16 at 10 (`0x4000` = project: param 5 = tempo,
  2 = master pitch). Other params: 0 = volume → CC7, 1 = pan → CC10, 4 = channel pitch, `0x8001 + k` = MIDI Out
  knob with idx k → its CC.
- 234 points: count u32 at 17, then 24-byte points `[f64 Δbeat][f64 value][f32 tension][u8 mode][3 bytes]`.
  A segment uses its end point's tension and mode. Mode 0 (single curve): `g(f) = expm1(a f)/expm1(a)`,
  `a = −t · ln 1e6`; jumps are ~5e-8-beat segments, within 1e-6 of one FL takes the average. Mode 12
  (double curve): an S of two mirrored halves `h(x) = x r / (1 − x + x r)`, `r = ((1 + t)/(1 − t))^1.7`
  (Hypernova t −0.36/−0.505/−0.715 → r 0.29/0.15/0.042). Mode 10 seen only on a flat segment.
- **Range = the automation channel's levels (219)**: pan = Min, volume = Max, `out = (min + v (max − min)) / 12800`.
  Tempo BPM = `10 + 512 · out` (Hypernova 1250/12800: 0.270563 = 185; Nuker 3 1250/4250: 0.75 = 150);
  `lfo_everythingnested` Max 6400 centers a value-1 clip on 0.5. Not for channel pitch: Hypernova
  "RISERS! - Channel pitch" (4800/8000) bends over the full range.
- **Nested clips** (target = an automation channel) hold their last value after they end. Param 0 = that
  channel's Max ("Output max level"); `0x0600..0x0604` = its LFO speed, tension, skew, PW, level, each
  bipolar `2v − 1` (level → amount). Verified: level, tension (`lfo_everythingnested`), Max (Hypernova).
  Automated skew and PW shift FL's LFO phase as they move (skew: peak pinned at 1/4 period); not modeled.
- **LFO**: on = u32 −1 at the start of the 16 bytes before the speed (0 = off). Hypernova: on for racks
  27/35/44 only; rack 45 (AMT set, off) would otherwise swing the pitch thousands of bend units. Event 212
  byte 20 is 5 for on and off alike. Points event 234: i32 at 4 = AMT, 64 = 0
  (128 = +1, clamped), byte 8 = 0 for Multiply. After the points: `[u32 1][16 ff][6 u32][u32 m at +44]
  [m × 24][u32][16 bytes]` then `[u32 speed][i32 tension][i32 skew][i32 pw]` (0..128, −1 = untouched).
  Speed is tempo-synced and exponential: period = `2^(4 (65536 − s) / 19717) / 16` beats (65536 = 1/16
  beat, 45819 "4:0" = 1 beat; Hypernova 50944 = 0.4864, measured 0.4863). Phase from the clip origin in f32:
  `f32(tick) · f32(1/(ppq · period))`. Shape: one triangle, 0 at the start rising, peak at `r/2`, 0 at 1/2,
  trough at `1/2 + (1 − r)/2`, rise time `r = (1 + skew)/2`. PW: the cycle runs in `1 − |pw|` of the period
  and holds its peak (pw > 0) or trough (pw < 0) for the rest. Tension t: `1 − g(1 − |x|)`, `g(f) =
  expm1(a f)/expm1(a)`, `a = 30 t |t|` (negative = spiky). Depth scales with the range: `d = amt · (max −
  min) · lfo`; Multiply `out (1 + d)`, else `out + d/2`; clamped 0..1. `lfo_everything` matches 744–765 of
  768 ticks per clip (rest ±1 CC). Open: Hypernova's Main Vol #4 (tension 1, value 0.78, nested Max)
  swings 0.58× our depth; its additive pitch LFOs ~15% less than ours.
- Written value: at tick + 1, whenever `round(v · steps)` changes, plus the clip's first tick; steps 65536 (vol, pan,
  knobs, master pitch), 16384 (channel pitch), 128 per BPM (tempo). The value written is quantized.
  CC = `floor(v · 128)` (max 127). Channel pitch cents = `round((v − 0.5) · 2 · range · 100)`, range = 215[112]
  semitones; master pitch cents = `round((v − 0.5) · 2400)`; both add into one pitch bend.
- Tempo clips with any point > 1 (Nuker 3, collab) are written on **every tick**, whole milli-BPM, unquantized.
- Exported tempo × time-signature denominator / 4. Tempo track: project tempo twice at tick 0, clips, then the
  project tempo one bar after the song end.
- Open: the collab's (FL 2025) channel pitch and knob clips write fewer events than FL (t2 41287/54545); predates
  the LFO work.

## FL's MIDI exporter (what we compare against)
- Track 0: time signature. Track 1: tempo. Then one track per used channel, named, on its MIDI Out channel.
- Note-offs are `0x8n`, written before note-ons on the same tick.
- **FL8's exporter ends every note 1 tick early.** FL11+ export the same data at full length. We output full length.
- Note fine pitch (0..240, 120 = none) transposes the exported key by `(fine-120)/10` semitones (fine 0 = -12; only this value seen).
- Event 219 (channel levels): u32 pan, u32 vol (0..12800, × 128/12800 → CC), i32 pitch in cents (→ static pitch bend, range ±1200, clamped).
- **Master pitch** (event 80, i16 cents) is added to every channel's pitch before the bend: +250 → 9898 instead of
  8192 (`cctest masterpitch(FL2026)`). Notes and tempo unchanged. Also added to automated pitch (unchecked).
- **Tick-0 knob values in the `Z` blocks are FL's live knob state at export time**, not the file: three exports of
  the same project gave CC7 = 0, 127 and 116 (the knob's ramp value wherever FL last left it). Exporting right
  after opening the project, without playing, is the most stable.

### Automation (verified on cctest, FL2026)
- Event 223, after pattern select: 12-byte records `pos u32, param u16, chan u16, val i32`, already sampled, one per value change.
  - param `0x0004` = channel pitch (cents) → pitch bend. Param `0x8001 + k` = MIDI Out controller `k` → CC.
- MIDI Out "On reset" options: last u32 of the state, a bitmask of *disabled* options (0 = all on; pan, volume,
  pitch, pitch bend range unticked = 27 in `cctest without on reset(FL20.6)`). Bytes 38 / 42 vary between FL 20.6
  saves of the same settings (44/83, 51/76; always summing to 127) and are 0 in every other save.
- MIDI Out controller list at byte 322: `[idx u8][cc u8][19 bytes]` (+4 zero bytes in v7) `[pascal name][pascal label]`…, `0xff` ends it.
  Entry +9 u32 = Min, +13 u32 = Max CC: clip value v writes `floor(min + v (max − min + 1))` clamped to Min..Max
  (Ascension FL 12: Damper 63..64 writes 63/64, not 0..127). The knob's saved value: u32 slot at state byte `34 + 4·idx`.
- MIDI Out byte 30 = program + 1 (0 = none): Gangnam FL 20.0 0x52 → `C 51`; ts6 / Nuker 3 1 → `C 00`.
- Placed through clips like notes (`pos < clip.len`).

### Reset blocks (FL 20.6–2026)
`H` = [CC10 pan, CC7 vol, static PB]. `B(v)` = [RPN 101=0, 100=0, CC6=12, CC10 pan, CC7 vol] + knob CCs at values `v`, or + static PB when there are no knobs. `Z` = the channel's automation events at song tick 0.
- Tick 0: `H`, `B(0)` `Z`, `B(0)` `Z`, `B(chased)`. Chased = event at tick 0, else the pattern's last event (it loops). `Z` is not repeated in the stream.
- Song end (last clip end): `B(current)`. At tail − 1: `Z`. At the tail: `B(current)` + end of track.
- **Tail = 30 s after the song end, rounded up to a 512-sample block at 44.1 kHz.** Fits 9216→15360 @128 BPM/96 PPQ and 15360→39362 @125 BPM/384 PPQ (256/1024 blocks also fit).
- **FL 20.0–20.5** (newstuff FL20.0 sample, ts6): tick 0 `H`, `B(0)` `Z`, `B(0)` `Z` (no chased `B`); song end
  `B`, tail `B` `B`, no `Z`. Their `B` carries the current pitch bend, not the static one. Checked in tests.
- **FL before 20.6** (Gangnam FL 20.0, Ascension and Hypernova re-exported in FL 12.3): `B(v)` = [RPN, CC10, CC7,
  PB, program, knob CCs], the PB always present. `v` = the knob's saved value, or a tick-0 clip's start value
  (later clips don't count). FL 12: tick 0 `H`, `B`, `B` `Z`, `B`; the tail is FL 20.0's (`B` `B` `B`, current PB).
  The creator's Hypernova and Nuker 3 .mids (knobs 0, PB dropped) came from a newer FL. Open: FL 12 omits CC7
  from `B` on a channel whose volume is clip-automated (Hypernova), and its `Z` re-sends every automated target's
  saved value before the clip writes.
- FL8 has no time-signature track. Pitch range 12 is still unexplained.
- **Cut remainder** (FL's export option, not in the .flp; `flpkit midi --cut`): the tail is song end + 1, not 30 s.
- **FL 12 has no tail**: tracks end at song end + 2 (Hypernova re-exported in FL 12.3, lfo_everything; Nuker 3's
  reference has + 1). The song end counts automation clips but not tempo clips (Hypernova: 797040, its tempo clip
  ends at 798720). FL 12's tempo track starts with a 0-tick 522 BPM (× denominator/4) event, has no closing tempo
  and stops at the song end.
- Time signature meta: FL writes `dd = 4 − log2(denominator)` (4/4 → 2, Hypernova's 4/8 → 1, i.e. "4/2").
  ts6 and Nuker 3 references were exported this way.

## Open questions
- FL8 playlist layout: decode its fields.
- Track index base per version.
- Meaning of the extra FL2025/2026 playlist bytes (maybe clip stretch/speed).
- Arrangements (99/241, FL20+): only one arrangement exists in these samples.

## Downgrade FL 25/26 → FL 20.x (`flpkit downgrade in.flp --to 20.6`)
Rules come from diffing the same song saved by FL 2025/2026 and by FL 20.0/20.6/20.9.1, plus the
~800 demo FLPs bundled with the FL installs (FL 3.2 … 26.1.1, 110 versions). `src/intro.rs` lists the
first version each event ID appears in, and events newer than the target are dropped.
The test checks 8 downgrades against FL's own 20.6/20.9.1 saves: the event sequence must be identical.

| Event | FL 25/26 | FL 20.x |
|---|---|---|
| 199 / 159 | version / build | target's |
| 172 (+ following 0, 192 app name) | 25.2.3+ header | dropped |
| 104 u16 + 50 | per channel (24.2.99+) | 22 u8 = low byte of 104 |
| 233 playlist | 80 (25) / 88 (26) bytes, untrimmed = 0/len | first 32 bytes, untrimmed = -1/-1 |
| 235 insert routing | 104 bytes, or 1 byte when empty | zero-padded to 127 |
| 238 | 70 bytes (24.2.99+) | 66 (20.1+), 62 (20.0) |
| 215 channel params | 168 bytes, f32 at 96 (24.2.99+) | u32 at 96; 168 (20.9) / 158 (20.6–20.8) / 157 |
| 165 / 166 per mixer track | 0 | 3 / 1 (20.9+ only) |
| 213 MIDI Out | state v7 | v6: controller entries lose 4 bytes, name "Controller" → "", label CC 7/10 → Volume/Pan (byte-exact vs cctest FL 20.6) |

**Parameter address** `[id u8][type u8][target u16]` in 225 (mixer params), 216 (controller data),
223 (pattern automation) and 227 (controller links, addresses at offsets 0 and 8):
- Mixer target: FL 2026 = `0x7000 + track<<6 + slot`, "Current" = track 501. FL 20 = `0x2000 + track<<6 + slot`, "Current" = 126. Tracks 126–500 can't exist in FL 20 and are dropped.
- Route level to track n: FL 2026 type `0x20`, id n. FL 20 type `0x1f`, id 64 + n.
- Channel volume/pan: FL 2026 `80:6e` / `80:6f`, FL 20 `00:02` / `00:03`.

**Not converted:** native plugin states other than those listed under "Plugin states" below (Sytrus, Slicex… may change between versions),
default mixer params FL 20 writes but 2026 omits (FL fills defaults), colours and window positions.
Event 172 is 3 bytes from **25.2.3** (not 26): every 25.2.3+ demo file desyncs otherwise.

### Tested in FL 20.6 (user, 2026-11-20)
- newstuff, What I Found: open fine, no visible difference (What I Found has 3x Osc, Sytrus, Parametric EQ 2, Maximus… in newer state formats and they still load).
- cctest: Emphasizer (FL 26.1.0+) fails. MIDI Out #2 lost its controllers and "On reset" options while its v7 state was kept as is; now converted to v6 (byte-exact vs the user's FL 20.6 save).
- Avicii remake: every VST fails ("saved with a newer version"). Fruity Wrapper state is a record list (`kind 12`, records `[u32 id][u64 size][data]`, ids 1, 2, 30–32, 50–57), with record 1 holding 12, likely its format version. Audio distorts/stretches oddly, most likely mis-read native effect states (Gross Beat, Transient Processor, Maximus…).
- So the downgrade now **refuses** projects with VSTs or plugins first seen after the target (`--force` overrides).

### Plugin states (from `vsttest w automation` v1/v2, saved by FL 20.6 and re-saved by FL 2026)
**Fruity Wrapper (VSTs)**: u32 kind (10 in FL 20.6, 12 in FL 2026), then records `[u32 id][u64 size][data]`:
1 (header, holds 12), 2 (flags; byte 9 bit 0 set by 2026 on VST3), 30–32, 50–52, 53 (plugin data),
54 (name), 55 (path), 56 (vendor).
- Record 53, VST3: `01 00 00 00` + sub-records 1 (64 B), 3 (component state), sometimes 2 (controller state). FL 2026 appends sub-record 4 (parameter table, ~8.5 KB).
- Record 53, VST2: starts `f7 ff ff ff`; byte 4 bit 0 set by 2026.
- Downgrade: kind 12→10, clear both flag bits, drop sub-record 4. Result is byte-identical to FL 20.6
  for Nexus, Pro-L 2, Pro-C 2 and SVStage. Absynth 6 and Kickstart 2 differ only inside their own data.

**Fruity Limiter**: v7 = v6 + one trailing byte. **Soundgoodizer**: unchanged.
**Gross Beat**: v9 = v8 + one trailing byte (`01`). FL 2026 also held one more 24-byte envelope point (counts
90→91, 180→181), which is self-consistent data a v8 reader handles. Left at v9, FL 20.6 resets Gross Beat
(all presets lost, user-tested). Downgrade sets the version to 8 and drops the trailing byte (one sample so far).

**Append-only native states** (u32 version at byte 0; the newer version adds bytes at the end; byte-exact vs SolidInc 20.6/20.9.1):
| Plugin | States by FL version | Downgrade |
|---|---|---|
| Parametric EQ 2 | v3/309 (≤20.7), v6/350 (20.8–20.99), v7–8/354 (21+) | truncate; u32 at 292 is 0/1 up to 20.99 but 2/3 from 24.1 → `&= 1`; byte 305 is 0 before 20.8. Left as 3, FL 20.6 froze and played noise (user-tested) |
| 3x Osc | v12/92 (≤20.8), v13/93 (20.9), v14/460 (20.99+) | truncate |
| Fruity Compressor | v2/32 (≤25.1), v3/36 (25.2+) | truncate |
| Fruity Free Filter | v1/20 (≤20.6), v2/21 (20.9+) | truncate |

**Maximus**: version at byte 2. 0x0a/1342 (≤20.6), 0x0f/1375 (20.8+); 0x0f = 0x0a + 33 bytes inserted at 507.
20.7/20.8 have in-between layouts (0x0b/1346, 0x0e/1371) and are not converted.
**Unchanged 20.0–26.1** (same header and size): Delay 3, Reeverb 2, Transient Processor, Harmless, Wave Candy, Soundgoodizer.

**Event 212** (plugin slot/window info, 52 bytes): u32 at 0 = mixer track. FL 2026 writes 501 for the "Current"
track; FL 20 (126) aborts loading on it ("error reading the FLP file", user-bisected on Avicii's Wave Candy).
**Clip trims (233)**: FL 20 also writes start/end 0/len and it means "cut at len"; only -1/-1 shows the whole
pattern. So 0/len is kept as is (rewriting it un-trimmed clips).

### Per-clip settings (FL 2025+ playlist record, offsets from the record start)
From `samples/persamplevariations(FL2026Export).flp` (clip = 768 ticks = 3870.97 ms at 124 BPM):
+32 u32 clip id · +36 f32 fade-in ms · +40 f32 fade-in curve · +44 f32 fade-out ms · +48 f32 fade-out curve ·
+52 f32 gain (linear: 0.5017 = −6 dB, 3.98 = +12 dB; automation clips store 0) · +56 flags (3 = settings on) ·
+72 f64 1.0 · FL 2026: +80 `ff ff ff ff 00 00 00 00`.
FL 20 has none of these. Downgrade: each sampler/audio channel used with gain ≠ 1 gets a copy (named
"… (−6.0 dB)") with its Levels adjustment volume × √gain (raw ↔ dB measured in FL 2026: 6705 −12, 9575 −6, 12800 0, 16240 +6, 19798 +12, 25600 = ×12.00 +21.6 max; linear in between), and those clips use it.
Fades: each faded clip gets a volume automation clip placed exactly over it (FL 20.9.1's own automation
channel as the template, linked to channel param 0 = volume): 0 → volume over the fade-in, volume → 0 over
the fade-out, then a step back to the volume at its end so later unfaded clips aren't muted. Clips with the
same target, length and fades share one automation channel (Avicii: 1967 clips → 47). Linear ramps (curve
shapes not ported); constant tempo assumed. Overlapping clips of one channel (crossfades) go to channel
copies ("… (crossfade n)"). Playlist records are 32 bytes: a 34-byte record once shifted every later clip
(FL showed 100k+ bars); `per_clip_ports_keep_playlist_valid` guards it.
Channels whose volume is already automated are skipped (warned).

**Automation clip data (234)**, FL 20.x: u32 1, u32 64, u8 0, u32 4, u32 3, u32 point count, points of
24 bytes (f64 Δposition in beats, f64 value 0..1, f32 tension, u32 flags), then 108 bytes of clip settings. Point flags: [mode, 0, 0, tension sign] — sign 02 = no tension, 01 = +, ff = −, 00 on the first point;
mode 0 = Single curve (default); only applies with tension ≠ 0 (user). FL 2026 clip fades have only a tension
control and look like FL 20's Double curve at 10% (user): converted fades use mode 1 (assumed Double curve), tension 0.1.
Its playlist clip's start/end offsets are f32 beats. FL 20 channel params (227/216 address `[param u16][chan]`):
0 = volume, 1 = pan, 2 = mod X, 3 = mod Y, 4 = pitch; 0x8000 + n = plugin parameter n.

**Event 229 — channel Levels adjustment** (20 bytes, same in FL 20.6 and 2026; `samples/adjustments(*).flp`):
i32 pan (0 centre, 6400 full right), i32 vol (12800 = 100% default, 25600 max), i32 ?, i32 mod X, i32 mod Y.
(Event 221, 9 bytes, default `0, 500, 0`, is something else.)

## Downgrade to FL 12.3 (`--flp 12.3.0.72`)
Derived from newstuff / SolidInc saved by FL 12.3.0.72 vs FL 2026, plus the 26 FL 12.3 demo projects.
`downgrade_matches_fl12_saves` checks the event sequence (newstuff: identical; SolidInc: identical apart
from 238s we keep for named tracks).

| What | FL 20 | FL 12.3 |
|---|---|---|
| Mixer tracks | 127 (0..125, "Current" 126) | 105 (0..103, "Current" 104): tracks 104..125 dropped (each runs to its 147, after its 236); addresses keep 0x2000 + track<<6 |
| 235 insert routing | 127 bytes | 105 |
| 236 | 12 bytes | 8; byte 4 bit 0x20 always set (FL 20 clears it); some FL 12 tracks have 0x80 there instead of 0x40 (not in FL 20) |
| Playlist tracks | 500, clip track field 499 = track 1 | 199, 198 = track 1 (field − 301; clips on tracks 200+ dropped) |
| 238 | 62/66 bytes, all 500 | first 49 bytes, written for max(33, last used track) |
| 215 | 157+ | 125 |
| 196 sample paths | `%FLStudioFactoryData%\Data\…` | `%FLStudioData%\…` |
| MIDI Out | v6 (+ "On reset" u32) | v4 = v5 (20.0) = v6 without the trailing u32 |
| Reeverb 2 | v17/66 | v16/58: v17 inserted a u32 at 56 |
| Fruity Wrapper | kind 10 | kind 8, same record list (no FL 12 VST save to compare contents) |
| 16 legacy effects (Balance, Blood Overdrive, Chorus, Compressor, Delay, Fast LP, Filter, Flanger, Free Filter, Mute 2, Phaser, Reeverb; also Bass Boost, 7 Band EQ, Center, Phase Inverter) | native: u32 version + i32 params (Balance: no version) | VST2 DLLs (`Plugins\VST\<name>.dll`) in a kind-8 Fruity Wrapper: 201 "Fruity Wrapper"; records 1, 2, 30, 31, 32, 50 fixed; 51 VST id (`CFfU`, `eRfU`, `x413`, `tFFF`, …); 54 name; 55 path; 53 VST2 chunk `f7ffffff 04 feffffff` + zeros to 21, u32 count, f32 params 0..1, u32 1, 25-byte program name (FL ≤ 11: 17-byte header, no count). float = (int − offset) / scale, scales mostly 1024 / 5000 / 10000 / 65536 (`LEGACY_VSTS`). The last four aren't converted (no FL 20+ states to fit) |

Same state versions as FL 20.6 (existing conversions apply): Limiter v6, EQ 2 v3/309, 3x Osc v12/92,
Maximus 0x0a/1342, Gross Beat v8, Soundgoodizer v3, Transient Processor v1. Sytrus and Love Philter are `[u16 3][u16 version][zlib]`; decompressed, Love Philter v11 (20.0)
inserted a byte at 54 and v12 (20.6) appended one, Sytrus v24 (20.0) and v25 (20.6) each appended one
(converted; kept as is, FL 12 misread them and their automation drove the wrong knobs). Untrimmed clips: FL 12 writes start/end as i32 −1 or f32 −1.0
(both seen, carried from older saves); untrimmed clip lengths can differ (FL recomputes pattern length).

## FL 12 slot flag (212)
Byte 16 of a channel's 212: FL 12 sets bit 0x04 on channels without a plugin (sampler, audio clip, layer, automation;
channel type != 2), FL 20+ writes 0x50 there.

## Downgrade structure (`src/downgrade.rs`, `STEPS`)
Each step turns one format into the one before it and runs when the target is older than its boundary, newest first:

| Step | Runs for targets below | Converts |
|---|---|---|
| `to_20_9` | 21 | FL 25/26 → 20.9: header block, 104→22, playlist 80/88 → 32-byte records, mixer addresses (0x7000/501 → 0x2000/126), MIDI Out v7, wrapper kind 12, EQ 2 v8, 3x Osc v14, Compressor v3, Gross Beat v9; post pass: per-clip gain/fades |
| `to_20_8` | 20.9 | Limiter v7, 3x Osc v13, Free Filter v2, 215 → 158 |
| `to_20_7` | 20.8 | EQ 2 v6 → v3 |
| `to_20_6` | 20.7 | Maximus 0x0f → 0x0a |
| `to_20_5` | 20.6 | MIDI Out v6, Love Philter v12, Sytrus v25, 215 → 157 |
| `to_20_1` | 20.1.1 | 238 → 62 |
| `to_12_3` | 20 | everything in "Downgrade to FL 12.3"; post pass: mixer/playlist track layout |

Event ids newer than the target (`INTRODUCED`) are dropped after all steps; 199/159 get the target version.
A new FL release: diff its saves against the previous version's and add a step on top (or split `to_20_9`).

## Overlapping clips (FL 2025+)
FL 2025+ doesn't play overlapping clips on one playlist track together. The later clip cuts the earlier one off at its
start, unless a fade spans the overlap (the earlier clip's fade-out or the later one's fade-in), in which case they crossfade.
FL 20/12 play both, so `cut_overlaps` shortens the earlier clip's length and scales its end offset (from Avicii in
FL 2026, user screenshot). Per-clip fades of 25 ms or less are declicks and get no automation clip.

## Sampler stretch (215)
| Offset | Field |
|---|---|
| 96 | stretch time: FL 20 u32, FL 24.2.99+ f32, same value (192 ↔ 192.0) |
| 100 | pitch (cents) |
| 104 | Mul, as 10000 × log2 (10000 = 200%) |
| 108 | stretch mode |
| 160 | f64 (FL 25+ only): Stretch Pro formant, 0.5 + cents / 2400; no FL 20 equivalent, dropped |

FL 2026's mode −2 isn't applied by FL 20 on load (it shows the settings but plays unstretched until the mode is toggled,
which saves −1), so the downgrade writes −1 (mode −2 = Stretch Pro). Stretch pitch stays in 215 at +100 and channel
pitch in 219 at +8. Confirmed on Avicii's reverse vocals and VEDH Impact against a user-toggled FL 20.6 save. FL 2025+ clip start/end offsets are in
unstretched sample ms, and FL 20 reads them the same way.
Mul without a stretch time: FL 2026 applies the Mul alone, but FL 12 doesn't until the mode is re-picked, which fills in
time = sample length × Mul × BPM/60 × 96 (vocal 02: 2.0468 s × 2 at 128 BPM = 838, user-toggled FL 12.3 save). The downgrade
computes it from the WAV header (`stretch_times`). Other formats are left alone, with a warning.

## Downgrade FL 8.0 → FL 6.0 (`src/fl6.rs`)
Rules from 6 demo songs saved by both FL 8.0.0 and FL 6.0.x (Blake-Hanging On, Blake-Nowhere near, Fuego-ThreeHours,
Pilchard-Tzachitx match FL 6's own save after conversion, except window-position junk in 212):
- Every event ID FL 8 writes also appears in FL 6 saves (FL 5 lacks 26, 27, 95, 230, 235, 236).
- Mixer: FL 8 has 70 tracks, FL 6 69. Each track's events start at a 149; drop the last track (149 … up to 225).
  225 is 12-byte records `[u32 0][param u8][31][slot/track u8][track u8]`: drop param 133 (new in FL 8, every track)
  and track 69 (b7 49, b6 & 0xC0 = 64). 235 loses its 70th byte.
- 212 (window info): FL 8 sets bit 0x40 in byte 16 and moved the u32 at 20 to 24; the rest is pointer junk.
  Byte 8 is 2 for native Fruity plugins, 0 for VST-hosted ones (FL 6 runs Compressor, Reeverb, Phaser, Free Filter
  from Plugins/VST): a plugin substituted by one of those needs 0, or FL 6 reports it not found.
- 224 notes: byte 14 is FL 8's release (default 0x40); FL 6 has none and writes 0.
- 233 playlist (28-byte items, both versions): row items `[4..8] = 00 00 00 60` (FL 8) / `… 50` (FL 6), row byte 12 =
  0xE7 − pattern, byte 14 = 3. FL 8 free pattern clips (`u16@4 = 0x5000, u16@6 = 0x5000 + pattern`, i32 −1 trims) don't exist
  in FL 6: they become row items on their pattern's row (byte 14 0 → 3, f32 −1 trims). Channel items (`u16@6 < 0x5000`) are the same.
- 234 automation v2 → v1: byte 13 2 → 1, points `[f64 Δ][f64 v][f32 t][u32 mode]` → `[f32 Δ][f32 v][f32 t][u32 mode]`;
  in the tail the 6th u32 after the 16 ff bytes 2 → 1 and its records `[f64][f64][8 bytes]` → `[f32][f32][8 bytes]`.
- Plugin states: identical in both for 3x Osc, BooBass, Compressor, Delay 2, Delay Bank, Flangus, Free Filter, MB
  Compressor, Parametric EQ, Peak Controller, Phaser, Reeverb, Stereo Enhancer (FL 6 hosts Compressor, Reeverb,
  Phaser, Free Filter as VST DLLs under the same names and states). Not in FL 6: Slicex, Love Philter, Parametric EQ 2,
  Maximus (FL 5 also lacks Delay Bank, MB Compressor).
- Love Philter (FL 8) → Free Filter: state `[u32 2][zlib]`; inflated, u32 text length at 60, the text, then 8 slots
  of `[12 u32 knobs 0..256: 1 cutoff, 2 width (0 = narrow), 8 pan (signed)][u32 order: 0 OFF, 1 x1, 2 x2, 3 x3, 4/5 alt][u32 type: 0 SVF (default sliders = LP), 1 LP, 2 HP,
  4 BP, 11 resonant LP][envelopes]` (7420 bytes per default slot in presets, 8524 in the song: f64 points). Type 0
  = no filter, only volume/pan envelopes (all Gate presets), so such a Love Philter is dropped. Free Filter state:
  4 normalized f32 type (index/6: LP, BP, HP, notch, low shelf, peak, high shelf), freq (not one exponential: 0.2001 =
  253 Hz and 0.2703 = 407 Hz read in FL, presets 0.7747 ≈ 5512, 0.918 ≈ 11025, 1 = 16000 Hz; log-interpolated), Q, gain (0.66 default).
  Love Philter cutoff log up to 20 kHz, 108/256 = 42 % ≈ 500 Hz (FL shows only %; matched by ear against FL 8);
  slot 0 only, envelopes dropped.
- Sytrus (`src/sytrus.rs`): FL 8 v18 = u32 0x12 + zlib, FL 6 v15 = 0x0f + zlib. On the inflated data: drop the 3
  bytes `01 00 01` at 974; curve objects `[u32 type ≤ 8][u32 2][u32 n][n × (f64 x, f64 y, f32 tension, u32 mode)]`
  → version 1 with `f32 x, f32 y` (type-1 curves also sit in a wrapper `[u32 2][u32 0][f32][f32]`, version 2 → 1);
  per operator two f32 tables of 256 entries → 128 (FL 8 pads the upper halves with zeros), 52 bytes after the
  operator start (after the preset's RTF text `7d 0d 0a 00` or an 8 × ff run). Matches all 20 demo pairs except
  real parameter differences.
- Substitutes: Parametric EQ 2 → Parametric EQ (same 7-band i32 gain/freq/width/type layout; PEQ 2 has a u32
  version first, PEQ 1 a trailing i32 128; widths scaled 17500/25412, a guess from the defaults; high/low pass bands get width 0.05 = 3277, scaled ones distort). Maximus →
  Fruity Multiband Compressor (native in FL 6) with FL 6's "Soft limiter" preset state (124 bytes, 31 i32: output gain, then 3 bands of crossover, 4 normalized params, attack ms, release ms; not decoded further). Fruity Compressor state: 6 normalized f32 (threshold, ratio, gain, attack, release, type).
- Slicex → Fruity slicer (both decks in one wav): Slicex stores deck A's path as a short string at state byte 96 (deck B's at
  2112) and its slices as the wavs' `cue ` points (tlst entry k = cue k → key 60 + k; deck B's keys follow deck A's,
  What I Found plays deck B slices 23–26 on keys 104–107). The converter joins B's audio after A's and lists all cues. Fruity slicer v6 state
  (FL 6 tutorials): `[u32 6][u32 1][u32 bpm][u32 0][u32 0][str path][u32 n][n × (str name, u32 sample pos, u8 0)]
  [u8 1][u8 60][u32 0][u32 rate][3 flags]`. v13 (what FL 6.0 saves): `[u32 13][f32 loop beats][f32 loop bpm]
  [i32 pitch cents (0/480/1200)][i32 stretch?][u32 mode 0..3][u32 0][u32 0..100][str path][u32 n][n × (str name,
  u32 pos, i32 −1, f32 −1, u8 0)]` + the same tail. The slicer stretches the loop from its bpm to the project tempo,
  Slicex doesn't: write bpm = project tempo, beats = frames / rate · tempo / 60 (ratio 1). FL 8 demo wavs can be Vorbis inside RIFF (format 0x6771, data = Ogg stream).
- Samples (196) not in FL 6's Data are copied from FL 8's Data next to the output (.ogg → .wav with ffmpeg; FL 6
  can't load ogg) and referenced by absolute path.

## Downgrade FL 8.0 → FL 7.0 (`src/fl6.rs`)
From the 9 demos saved by both 7.0.0 and 8.0.0 (Cool stuff/*, NewStuff is a different song in FL 7). FL 7.0 already
writes FL 8's Sytrus v18, window info (212), free pattern clips, automation v2 (234), note release byte, Love
Philter v2 and Parametric EQ 2 (state version 1; FL 8 writes 2, same 305-byte layout). FL 8 only adds the 70th mixer
track and mixer param 133 (225), and 235 grows 69 → 70 bytes. Converting the 8 FL 8 saves back gives the FL 7 event
sequence exactly (remaining diffs: 200, window pointers, re-zlibbed states). Maximus and Slicex (FL 8 only) get
the FL 6 substitutes; samples FL 7's Data lacks are copied as for FL 6.

## Downgrade FL 6.0 → FL 5.0 (`src/fl5.rs`)
From FL 5's seven 5.0.0 demo saves and the 5 songs FL 5 ships as 4.x saves and FL 6 as 6.0 saves (Cool stuff:
DanR, JasonC-Abstraction, MrSpecial, Pilchard, ScottFisher). Converting the FL 6 saves reproduces FL 5's mixer
event order, 147 and 225 byte for byte where the song wasn't edited between saves.
- FL 5 never writes 26, 27, 95, 235, 236 (230 = link formula, which FL 5 does write). Mixer track: FL 6 `149 204 27 236 [201 212 213]* 235 154 147`,
  FL 5 `[201 212 213]* 149 204 154 147` (slots before the header); a slot with no state still gets an empty 213.
- 147 (track output): FL 5 = one target, 0 master, j = insert j, 65 on the master; FL 6 writes −1 and routes with
  235 (69 bytes, byte j = 1 if the track feeds track j; sends 65–68 always set). FL 5 target = first insert j set.
- 225 (12-byte records, byte 4 param, byte 5 = 31, u16 at 6 >> 6 = 128 + track): params 0/1 (slot on, slot mix)
  as is; FL 6 64+j = route level to track j → FL 5 64..67 = send level to send tracks 65..68 (FL 6 129..132, inserts
  1–64 only), other routes dropped; FL 6 192.. → FL 5 96.. (−96). Slot mix (1), sends, volume (96), pan (97) are
  0..128 / ±64 in FL 5, ×100 in FL 6. FL 6's single (0, 0) record is dropped. FL 5 order: per track its slots'
  0/1 records, then the track params by number.
- Channel levels: 209 and 219 fields 0 and 1 (i32: pan, volume; an automation channel's range) are 0..128 in FL 5,
  ×100 in FL 6 (162/162 fields of the 5 pairs); so are 229 fields 0–1 (80/81 channels). 216 = current values of
  automated params, 12-byte records `[u32 0][u16 param][u16 target][i32 value]` (227 addresses): mixer params
  renumbered like 225, and mix/send/volume/pan, channel volume (0) / pan (1) and the main volume (target 0x4000,
  param 0: 12800 → 128) ÷100; plugin params as is. 226/227 link records: FL 6 writes 4 / 8 in the u32 at 12, FL 5 always 0. A 230 right after a link is
  its mapping formula (text; "1-Input" = inverted). FL 5 writes it the same way (an FL 5 save with "inverted"
  ticked), so it is kept; FL 5's demos just never use one. Negating the i32 at 16 does not invert in FL 5. FL 5 writes the main volume as byte event 12 (128 in its demos),
  FL 6 doesn't. Left unscaled, FL 5 (and FL 2026 reading a 5.0 file) sees every channel at max, panned right.
- 227 (automation/controller links, 20-byte records): a mixer target is `[u8 param][u8 31][u16 128 + track << 6]`
  at 8, the 225 coding, so it is renumbered the same way (FL 5 demos link volume as `60 1f`).
- Plugins missing in FL 5 (What I Found: Multiband Compressor → Fruity Compressor, FL 5's "FastMaster" preset for
  the Maximus stand-in, "Light" otherwise, 212 byte 8 = 0 as FL 5 hosts it as VST; Delay Bank → Delay 2): Multiband Compressor, Delay Bank, Reeverb 2, Squeeze (and FL 6's new generators).
- Sytrus v15 → v7 (`src/sytrus.rs`, byte-exact on the ScottFisher pair, 43146 bytes): FL 5 reads v15 as v7
  garbage (What I Found then pinned CPU and the output meter with nothing playing). Inflated payload: header up
  to 966 equal; v15 adds 6 bytes at 966, 38 at 1043 and a u32 index (0, 1, 2) at 1095/1112/1129 (17-byte records),
  then the RTF text (u32 length at 1133, v7 1077). 6 operator blocks (op 0 right after the RTF, ops 1–5 after
  `00000000 ff×16`): v15 adds a u32 at field start + 48 (operator noise 0..128), 2 harmonic tables of 128 f32 (v7: 24) at + 52, and the
  12 bytes at end − 28 are `80 00 00 00 ×8 +1 +3` vs v7's 15: v7 = 4 bytes, noise u32 (FL 5 save with noise 100% = 128), 4 zero, 3. The same 499 v1 curve objects
  (copied as is); 3 sections lose 7 trailing bytes (108 → 101) and 3 lose the byte after their ff run (37 → 36).

## Downgrade FL 5.0 → FL 4.5.2

Learned from the demos FL 4 ships as 4.5.2 saves and FL 5 as 5.0.0 saves (Fuego-FL303, Fuego-Momentum; after
these rules only song edits, window positions and FL 4's 153 sample-rate events differ). Code in `src/fl4.rs`.
- 199 → `4.5.2`; drop 206/207 (song genre/author strings) and 234.
- 226/227 records: u32 at 16 = 128 (FL 4 constant; FL 5 writes signed values such as −512).
- 215: first 92 bytes (FL 5 appends 20).
- 143 channel flags: FL 4 keeps bit 0 only; FL 5 bit 3 becomes FL 4's event 19 = 1 right after 143.
- 140 (u32 0) between 70 and 22 on every channel.
- Fruity slicer v13 → v6: `[u32 6][i32 bars][i32 bpm][i32 0][i32 0]`, then the same path and count; each slice is
  name, u32 pos, u8 0 (v13 adds i32 −1 and f32 −1); same 13-byte trailer with the last byte 0.
- Sytrus v7 → v6 (both `[u32 version][zlib]`, `src/sytrus.rs`, from "Synth string - fat texture" saved by
  FL 4.5.2 and FL 4.9.0): drop byte 967; curves `[type][u32 1][n][n × 16]` → `[type][u32 0][n][n × 12]` (the
  4th u32 per point goes); flag byte 1 → 0 at op + 271, after the ff run of each 36-byte section, and at 60 of
  the 76-byte section. Result is byte-equal on that pair except 3 values edited between the saves.
- Plugin states otherwise equal (3x Osc, BooBass, Parametric EQ, Peak Controller, Delay 2, Stereo Enhancer,
  Flangus; Compressor, Reeverb, Phaser and Free Filter are VSTs in FL 4 as in FL 5).
- 226 `ff000000 ff000000 0400 ff0f 00000000 80000000`: every FL 4/5 save has it once, before the 227 list (or
  the first channel); FL 8 → 5 output lacks it, which FL 5 accepts but FL 4 rejects ("error while reading").
- Channel type (21) 5 = automation clip, unknown to FL 4 (types 0–4; the file is rejected). The FL 4 step makes them empty
  samplers and drops their playlist items (233 records with u16 0x5000 at 4, channel at 6).
- Audio clips (type 4) exist in FL 4 and keep their playlist items; trims at 20/24: FL 5 f32 ms (−1 untrimmed) →
  FL 4 u32 start / end frames at 44.1 kHz (untrimmed `ffffffff` / `7fffffff`; NewStuff 0 / 881999 = 20 s at 90 BPM).
- Clip automation (`clips` in `src/fl4.rs`): each linked clip gets a Fruity X-Y Controller in a free mixer slot (state
  `[u32 1][X][X min 0][X max 65536][0][Y][Y min][Y max]…`, param 0 = X, link output 0 = X; 212 u32 0/4 = mixer
  track/slot); the clip's 227 links take the X-Y as source (`[0][0][0x2000 + track << 6 + slot]` at 0 and 4), so
  FL 4 maps X onto each target's range and keeps 230 formulas. Each placement becomes a pattern of 223 records
  `[u32 tick][u16 0][u16 X-Y address][i32 X]`, X = 65536 · (min + v (max − min)) / 128 (clip channel 219), last
  record at length − 1 to set the pattern length. Pattern playlist items: u16 0x5000 at 6, length ticks at 8,
  u16 at 12 = 0x78E7 − pattern, 3 at 14. Pattern names (65 193 150 151) sit after the channels.
- 135 goes before the sample path (196) in FL 4.
- 140 (after 70) = time stretch in steps, 0 = off (user's FL 4.5.2 save: AutoPhresh 16 vs its unstretched copy 0).
  From FL 5/8's stretch time (215 u32 at 96, 192 per beat: 768 = 4 beats = 16 steps); Mul/pitch/mode dropped.
- 215 is 96 bytes in FL 4.5.2 saves (u32 at 92 = 1); older 4.x demos 92. 153 = sample rate on audio clip
  channels (before 143).

## Downgrade FL 12.3 → FL 8.0 (`src/fl8.rs`, `src/sytrus.rs`, `flpkit downgrade --to 8.0`)

From newstuff / SolidInc saved by FL 8.0 and FL 12.3, checked on Scott Fisher - Moholithic Filter (FL 2026 export →
12.3 → 8.0: same 9476 notes in our MIDI export as the 2026 file; Sytrus byte-equal to FL 8's own copy of the demo).
newstuff converted from its FL 12 save has FL 8's exact event sequence; values differ only in window positions,
sample paths, MIDI Out/Limiter states.
- Header: 199 `8.0.0`; drop 28, 29, 31, 32, 97, 98, 202, 237, 238, 239; 156 (BPM × 1000) → 93 (u16 fraction) + 66
  (u16 BPM); 195 → 198; empty 216 before the first pattern if none.
- Text (193–207, 230, 231, 239, 241): UTF-16 → 8-bit.
- 224 notes 24 → 20 bytes: key u16 → u8; the u16 group and the u8 after fine go.
- 233 playlist 32 → 28 bytes: track u16 at 12 (199 − track) → u8 (99 − track); the u8 at 16 moves to 13; untrimmed
  start/end i32 −1 → f32 −1.
- 234 automation clips: versions 3 → 2 at 13 and tail + 40, point mode's top byte cleared, last 4 bytes dropped.
- Channels: FL 12 `64 21 201 212 203 155 128 …`; FL 8 has no empty 201/212 on samplers, and writes the name as
  192 + color 128 at the channel's end (after 20 / 196). 215 → first 112 bytes. 150 pattern color + 151 (0); 152 (0)
  before 233. FL 12's default pattern color 0x565148 = FL 8's 0xc8c1b8.
- Mixer: FL 12 has 105 tracks; FL 9+ kept FL 8's inserts 1–64, moved its 4 send tracks to 100–103 and the current
  track to 104 (65–99 are new, dropped). FL 8 track: `149 [95] 204 27 236(00000000 01) [201 212 213]* 235(70) 154 147`;
  27 = FL 12's 236 byte 4 & 0x1f; colors = FL 12's − 0x24 per channel; 235 bytes remapped, inserts 1–64 always route
  to 65–68, track 69 nowhere; 147 and channel 22 targets remapped. 98 (slot index), 155/128 slot colors dropped.
- 225: slots 0–7 only (FL 12 has 10); per track: slot params, track params, then route levels 64–133 (to FL 8
  tracks 0–69; 12800, sends 0 unless FL 12 has them as 64 + its track number).
- 216 / 226 / 227 addresses: mixer tracks and route-level params remapped as above; plugin params flagged 0x8000 in
  FL 12 are bare on mixer effects, 0x2000 + param on channel generators (2 Sytrus links).
- Legacy effects: FL 12's kind-8 Fruity Wrapper chunk floats are FL 8's native state (201 = the effect name, 212 byte
  8 = 0). Checked against FL 8's Moholithic: Compressor param 5 is int / 7, Delay param 4 (int − 1) / 15 (fixed in
  `LEGACY_VSTS`).
- Sytrus v23 → v18 (`src/sytrus.rs`, SolidInc's 2 patches): drop byte 977; text [varint n][UTF-16] → [u32 len]
  [RTF]; curves version 3 → 2 with the point mode's top byte cleared; version bytes 3 → 2 at op + 2128, 36-byte
  sections at 20, 37 at 21, 76 at 60; 40-byte sections lose their last 4.
- Fruity slicer v16 → v15: v15 = header, path, slices, 13-byte trailer, u32 0 (v16 appends per-slice articulator
  blocks). FL 2026's Moholithic has no slicer sample or slices left (FL 8's points to a `.speech` file).
- 7 Band EQ: FL 20+ = u32 1 + FL 8's 7 floats.
- Step-sequencer steps: FL 12+ saves them as length-0 notes (224), which FL 8 doesn't play. FL 8 keeps them as
  91 `[u8 step][u8 1]` (step = ticks / (PPQ / 4)) under a 65 (pattern) right after the channel's block
  (FL 8's Moholithic: 65 1, 91 00 01 / 04 01 / 08 01 / 0c 01 after the kick); patterns left empty lose their 65.
- 212 byte 8 = 0 for every effect FL 8 hosts from Plugins/VST (7 Band EQ, Balance, Bass Boost, Blood Overdrive,
  Center, Chorus, Compressor, Delay, Fast LP, Filter, Flanger, Free Filter, Mute 2, Phase Inverter, Phaser,
  Reeverb), even when FL 12+ saved it as native (2): FL 8 otherwise reports "plugin could not be found".
- Sample paths `%FLStudioData%\X`: FL 8 Data-relative if FL 8's Data has a file of that name (FL 12 moved packs:
  `Packs\Legacy\Drums\Dance` = FL 8's `Packs\Dance`), else copied from FL 2026's Data.
- Not converted (warned): Love Philter, Slicex, Maximus, Parametric EQ 2, MIDI Out, Limiter states.
- Love Philter → Free Filter links: Filter 1 cutoff (param 9) → Free Filter freq (param 1), the source clip's
  values mapped through the same cutoff calibration; links to other Love Philter params are dropped.

## Downgrade FL 4.x → FruityLoops 3.5.4 (prototype: `tools/fl4to3.py`)

From the demos both ship (Saw, Clouds, LogDrumThing, BeepMap Ambi as 4.1.2 / 3.5.4; Hoovalicious, SubwooferTest,
NewStuff). Converting the FL 4 saves of Saw/Clouds/BeepMap reproduces FL 3.5's event sequence; left: effect order,
window junk (212), 217 values, 84 (0 or 8), playlist order within a bar.
- Header: 199 `3.5.4`, 200 = one 0 byte; 17 = 16 then 24 = 16 (FL 4: 17 = 4); 23 = 1; 217 (16 bytes) after 18.
- 226/227: 16 bytes (FL 4 appends u32 128). No 150/151/152 (pattern colors), 26, 142.
- Playlist: no 233; one 129 per item, `[u16 bar][u16 pattern]`, bar = ticks / (4 · PPQ). PPQ is kept (384 stays 384).
- Mixer: 21 tracks (0 master, 1–16 inserts, 17–20 sends = FL 4's 65–68), 8 slots. All effects (201 212 [203] 213,
  212 u32 0/4 = track/slot, byte 20 = 0x10) come before the first channel. After the playlist: 84, 225 = only the
  send levels (params 64–67) of inserts 1–16, then per track 204 name + 147 (0 on the master, −1 elsewhere), 133.
  No 149/154. FL 4's insert fader (225 param 96, 0..128) and 3-band EQ (112–114 gain 1/100 dB, 120–122 freq,
  128–130 width) have no FL 3 slot: a Fruity parametric EQ goes in the track's next free slot. Its state uses the
  same coding (FL 4's mixer EQ defaults = its Default preset): `[7 gains][7 freqs 0..65536][7 widths][7 types: 0 off,
  1 LP, 3 HP, 4 notch, 5 low shelf, 6 peak, 7 high shelf][global gain 0..128]`; bands 1–3 = low shelf / peak /
  high shelf, global gain = the fader. Pan (97) / stereo separation (98) not carried.
  Verified: FL 4 faders 128/100/64/32 vs FL 3.5 EQ global gain at the same values export sample-identical (out/fadertest).
- Channels: 22 (insert) = FL 4's − 1; 215 = first 80 bytes; 219 byte 20: FL 4 6 = FL 3 7 (all pairs); FL 4 1 is 1
  or 5 in FL 3, 4 never appears in FL 3 (left as is).
- 3x Osc state v12 → v11: first 88 bytes, version byte 11. Other Moholithic plugin states are the same size/format.
- Plugin names as FL 3.5 spells them (`Fruity reeverb`, `Fruity 7 band EQ`, `Fruit kick`).
- Samples: FL 3.5 paths are relative to its `Samples` folder (`\Instruments\...`); FL 4 Data samples FL 3.5
  lacks are copied. FL 4's speech synth (`.speech`, rendered on load) doesn't exist in FL 3.5: the slicer is pointed
  at `<stem>.wav` in the samples folder for a render made in FL 4.


## Downgrade FL 12.3 → FL 11.1 (`src/fl11.rs`, `flpkit downgrade --to 11.1`)

From the newstuff / SolidInc pairs saved by both versions (`samples/*FL11Export`, `*FL12Export`); after the
rules below both convert to FL 11's own saves except edited content and runtime pointers (212 bytes 20..35).
- FL 11 keeps FL 12's 24-byte notes (224), 32-byte playlist items (233), 105 mixer tracks, automation clips (234).
- Text events 8-bit (as FL 8), 202 included; 195 → 198; 199 `11.1.1`; 32, 97, 98 dropped; 215 first 112 bytes;
  238 first 47 bytes with byte 17 `ff` → `f0`.
- Channels: `64 21 [201 212 213] … 143 20 [196] 192 155 128` (FL 12's 203 name / 155 / 128 move to the end;
  samplers drop the empty 201/212).
- Mixer track: `[149] [95] [204] 27 236(5) [201 212 [203] 213]* 235 154 147`. 149 only for colored tracks (FL 12
  color − 0x24 per channel; FL 12's default `78 81 86` = none), 27 = 236 byte 4 & 0x1f, 236 = `00000000 01`.
  235 bytes 100..103 = 1 on tracks 1..99 (inserts route to the sends). Wrapped effects get a 203 with the
  wrapper's name record (54). 8 slots: FL 12 slots 8/9 move to the track's free slots (212, 225, addresses).
- 225: send levels (params 164..168) of 0 omitted. Addresses (216/226/227) as FL 12 → 8 with no track remap.
- Steps: length-0 notes → 91 like FL 8, plus 72 `[step][velocity]` before the 91 when velocity ≠ 100.
- States: Maximus / Gross Beat / Slicex `[u16 3][u16 v]` → `[u32 v]`; Parametric EQ 2 v3 → v2 (drop the last
  u32); Harmless v12 → v11 (drop the last byte). Not done: Sytrus v23 → v22, Love Philter v10 → v5,
  MIDI Out v4 → v2 (+2 bytes, first diff at 28). Fruity Delay 3 → Fruity Delay 2 (`delay3()`: wet → input,
  dry, feedback, synced time in 1/192 beat, ping pong, cutoff; links to wet → Delay 2 param 1, other knobs
  dropped). FL 11 has no Transient Processor (dropped);
  Wave Candy v9 dropped (no rule, visual only).
- Fruity Wrapper (kind 8) as FL 11 writes it (user's FL 11 save with Nexus / Sylenth1 VST3, GClip / GMax /
  TapeStop VST2): no records 56 (vendor) / 57; record 2's u32 at 9 has no top byte (FL 2026 sets 0x24 / 0xa4 /
  0xa0 there; Nexus and TapeStop with 0x80 were "not found"); a VST2's name (54) is its DLL's file name
  (`dblue_TapeStop`, FL 2026: `dblue.TapeStop`). VST3 entries (52 = class ID, 55 = the .vst3 path) load as is.
- VST2 plugin data (record 53, `f7ffffff`): FL 12+ `[u8 flags: 4 params / 0x0c chunk][i32 -2][u32 chunk size]
  [8 bytes][data]` → FL 11 `[u8 flags | 1][u32 chunk size, 0 for params][u32 0][data]` (TapeStop params in the
  user's FL 11 save, BassMidi VSTi chunk in an FL 11.1.1 project). Without it Sylenth1 / Nexus VST2 load with no
  preset and TapeStop fails.
- VST2 program: FL 12's u32 at 17 (current program) → FL 11's u32 after the chunk size (guess; Sylenth1 channels
  with a non-zero program lost their preset without it). Channel 215 i32 at 108 = stretch mode; FL 11 demos use
  0..9, FL 12's negative Elastique Pro modes → 1 (E3 generic), else FL 11 resets them to Resample.
- FL 11 ignores the channel pitch knob (219 i32 at 8, cents) on a stretched sample (215 mode at 108 ≠ 0) and
  only applies the stretch pitch (215 i32 at 100, cents): the knob is added to it and zeroed (user's FL 11 test).

## Window sizes and fade-clip ends (all downgrade targets, 2026-11-22)
- 212 bytes 36..51 = window x, y, w, h. FL 11..20 save w = h = 0 for never-resized windows (plugin default);
  the 20.9 step zeroes w/h for every target so newer / replaced plugins don't open tiled (Delay 3's 747 x 217
  made FL 11's Delay 2 tile). FL 8 never saves 0 but reads it fine (user-tested in FL 8 and FL 11), so
  the FL 8 step writes 0 too; FL 8 sizes made FL 11 open e.g. 7 Band EQ at FL 8's narrow 94 x 321.
- Per-clip fade automation clips are placed 1 tick longer than the clip (unless the same channel's next fade
  clip starts there): FL doesn't apply a point on a clip's last tick, so the back-to-full-volume step never
  happened and Avicii's Progressive Kick stayed at 0 after bar 65.
