# Daily time signals

The cues start only at 1× during Operating and play once per crossing. Each clip
finishes naturally if the player changes speed. The two metro cues share one
original three-note signal. Sunrise follows the region's monthly daylight table; the other times use the
simulation's local clock.

| Time | Sound | File | Clip length |
| --- | --- | --- | --- |
| Sunrise | Rooster | `rooster.ogg` | 2.52 s |
| 08:30 | Metro chime | `metro-chime.ogg` | 2.4 s |
| 12:00 | Gong followed by children laughing | `noon.ogg` | 11.5 s |
| 17:00 | Metro chime | `metro-chime.ogg` | 2.4 s |
| 17:30 | Doorbell | `doorbell.ogg` | 3.2 s |

## Original tonal cues

`metro-chime.ogg`, `gong.ogg`, and `doorbell.ogg` are original synthesized sounds
made for Megawatt Mogul and covered by the repository's license. They use metallic
resonator modes with short attacks and decaying tails. No transit operator's
recording or branded melody is sampled.

Regenerate them with `python3 tools/audio/generate_cues.py` (Python standard
library and ffmpeg with its native Vorbis encoder). The output is stereo Ogg
Vorbis at 44.1 kHz, with a peak amplitude of 0.68 before encoding. Playback gain is
set in `crates/game/src/audio.rs` for standalone cues.

## Noon sequence

`noon.ogg` concatenates the 5.5-second gong and six-second children’s laughter
with no added gap. Combining them into one clip keeps the order and timing
independent of frame rate and simulation speed changes.

The source clips are `gong.ogg` and `children-laughing.ogg`. Their gains are baked
into the combined asset: **0.85 for the gong** (about 4.6 dB louder than its previous
0.5 playback gain) and **0.45 for laughter**. The game plays `noon.ogg` at unity
gain. `generate_cues.py` regenerates both the tonal sources and this combined clip,
using a PCM intermediate before encoding it as stereo Ogg Vorbis at 44.1 kHz.

## Children laughing

`children-laughing.ogg` is the first six seconds of **Kids laughing** by
**kikorurelas**, recorded with a Zoom H6 for a theater production.

- Source and CC0 license: https://freesound.org/people/kikorurelas/sounds/642516/
- Downloaded HQ preview: https://cdn.freesound.org/previews/642/642516_6252416-hq.mp3
- Preparation: trim to six seconds, 80 ms fade-in, 800 ms fade-out starting at
  5.2 seconds, loudness normalization to -19 LUFS / -3 dBTP, and conversion to
  stereo Ogg Vorbis at 44.1 kHz. No pitch or speed change.

The laughter excerpt uses ffmpeg's native Vorbis encoder:
`-ac 2 -c:a vorbis -strict experimental -b:a 128k`.

## Sunrise rooster

`rooster.ogg` contains the **Medium rooster crowing** recording by **alys**,
originally published on PDSounds (record 539, 6 May 2008), and transferred to
Wikimedia Commons by Fæ.

- Source and public-domain dedication:
  https://commons.wikimedia.org/wiki/File:Medium_rooster_crowing.ogg
- Original file:
  https://upload.wikimedia.org/wikipedia/commons/0/0d/Medium_rooster_crowing.ogg
- License: released into the public domain worldwide by its author; the source
  also grants unconditional use where a public-domain dedication is not possible.
- Format: Ogg Vorbis, stereo, 44.1 kHz, approximately 2.52 seconds.
- Preparation: keep 2.88–5.40 seconds of the original recording to isolate the
  crow and omit the muffled clucking before and after it. Apply a 5 ms fade-in
  and 20 ms fade-out at the cuts, with no gain, pitch or speed change. Encode an
  audio-only stereo Ogg with ffmpeg's native Vorbis encoder; the original's extra
  Ogg Skeleton data stream is unsupported by Bevy's Vorbis decoder.

The game plays it once when the regional monthly sunrise is crossed at 1×,
at reduced volume. It is not stretched with simulation speed.
