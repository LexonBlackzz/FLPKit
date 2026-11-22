# FLPKit

Tools for FL Studio project files (`.flp`):

- **MIDI export.** `flpkit midi` writes a `.mid` with the same notes FL's own exporter produces, without opening FL.
- **Downgrading.** Open a project from FL Studio 2025/2026 in FL 20, 12, 11, 8, or as far back as FruityLoops 3.5.

FLPKit is not affiliated with Image-Line. FL Studio is their trademark.

## Build

You need [Rust](https://rustup.rs).

```sh
cargo build --release
```

This gives you `target/release/flpkit.exe`.

## MIDI export

```sh
flpkit midi song.flp                # writes song.mid
flpkit midi song.flp -o out.mid --cut
```

`--cut` matches FL's "Cut remainder" export option. `--zero-vel` writes velocity-0 notes as velocity 1.

## Downgrading

Back up your project first. Each step rewrites the file for the older version and drops or substitutes what that version lacks. The tool prints a warning for each loss.

```sh
flpkit downgrade song.flp --to 20.6       # also 20.9, 12.3, 11.1, 8.0, 7.0, 6.0, 5.0, 4.5, 3.5
```

`flpkit` refuses a project when the target can't load something in it, like a plugin newer than that version. `--force` converts it anyway.

`flpkit` also takes FL 12.3 saves for the FL 11 and FL 8 targets, FL 8 saves for FL 7 and older, and FL 4 saves for FruityLoops 3.5. From FL 8 down it copies samples the older FL's Data folder lacks into a `<output> samples` folder, using [ffmpeg](https://ffmpeg.org) on your PATH for Ogg samples.

| From | To | How | Tested on |
|---|---|---|---|
| FL 2025 / 2026 | FL 20.9, 20.6 | `flpkit downgrade --to 20.6` | Opened and played in FL 20.6 |
| FL 2025 / 2026 | FL 12.3 | `flpkit downgrade --to 12.3` | Opened and played in FL 12.3 |
| FL 2025 / 2026, FL 12.3 | FL 11.1 | `flpkit downgrade --to 11.1` | An Avicii remake, sounds right in FL 11 (64-bit) |
| FL 2025 / 2026, FL 12.3 | FL 8.0 | `flpkit downgrade --to 8.0` | Scott Fisher's Moholithic Filter in FL 8 |
| FL 2025 / 2026, FL 12.3, FL 8.0 | FL 7.0 | `flpkit downgrade --to 7.0` | SolidInc's What I Found; the slicer sounds a bit different |
| FL 2025 / 2026, FL 12.3, FL 8.0 | FL 6.0 | `flpkit downgrade --to 6.0` | What I Found loads in FL 6 |
| FL 2025 / 2026, FL 12.3, FL 8.0 | FL 5.0 | `flpkit downgrade --to 5.0` | What I Found plays in FL 2026; not yet heard in FL 5 itself |
| FL 2025 / 2026, FL 12.3, FL 8.0 | FL 4.5.2 | `flpkit downgrade --to 4.5` | What I Found in FL 4.5 |
| FL 2025 / 2026, FL 12.3, FL 8.0, FL 4.x | FruityLoops 3.5.4 | `flpkit downgrade --to 3.5` | Moholithic Filter in FL 3.5, a little louder than FL 4 |

### What you lose

Older versions lack plugins and features, so FLPKit swaps in the closest match:

- Fruity Delay 3 becomes Fruity Delay 2 with the same time, feedback, wet and dry. You lose its modulation, diffusion and saturation.
- Maximus becomes a Multiband Compressor (FL 6) or Fruity Compressor (FL 5). Slicex becomes Fruity Slicer. Parametric EQ 2 becomes Parametric EQ.
- FL 2025's per-clip gain and fades become channel copies and volume automation clips.
- FL 4 has no automation clips, so FLPKit moves each one into an X-Y Controller driven by pattern automation.
- FL 3.5 has no mixer faders or EQ, so a Parametric EQ in a free slot takes their place.
- FL 3.5 has 16 insert tracks. Effects on FL 4's inserts 17–64 get dropped, with a warning, and with them the X-Y Controllers that carry FL 5 automation clips.
- Transient Processor (FL 11 and older) and plugin states FLPKit can't convert get dropped, with a warning.

### Samples and plugins

The converters copy samples the older FL lacks next to the output file. They look for FL installs in `C:\Program Files (x86)\Image-Line`. Set `FLPKIT_IL` to your Image-Line folder if yours lives elsewhere, and `FLPKIT_PF` to the folder holding `FLStudio4` / `FruityLoops3.5`.

Third-party VSTs need a version the old FL can load. FL 8 runs 32-bit only. FL 11 and up load 64-bit VSTs, and FL 11 also loads VST3.

## Format notes

[FORMAT.md](FORMAT.md) documents the FLP format as reverse-engineered for this project, across FL versions from FruityLoops 3.5 to FL 2026: event IDs, plugin states, mixer layouts, and every downgrade rule with the evidence behind it.

## Tests

`cargo test` checks MIDI output against FL's own exports and downgrades against FL's own saves. Those test projects can't ship with the repo, so the tests skip when `samples/` is missing.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
