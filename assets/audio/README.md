# Sunrise rooster

`rooster.ogg` contains the **Medium rooster crowing** recording by **alys**,
originally published on PDSounds (record 539, 6 May 2008), and transferred to
Wikimedia Commons by Fæ.

- Source and public-domain dedication:
  https://commons.wikimedia.org/wiki/File:Medium_rooster_crowing.ogg
- Original file:
  https://upload.wikimedia.org/wikipedia/commons/0/0d/Medium_rooster_crowing.ogg
- License: released into the public domain worldwide by its author; the source
  also grants unconditional use where a public-domain dedication is not possible.
- Format: Ogg Vorbis, mono, 44.1 kHz, approximately 8.3 seconds.
- Preparation: losslessly remuxed to an audio-only Ogg container with
  `ffmpeg -i input.ogg -map 0:a:0 -c:a copy rooster.ogg`; the original's extra
  Ogg Skeleton data stream is unsupported by Bevy's Vorbis decoder.

The game plays it once when the regional monthly sunrise is crossed at 1×,
at reduced volume. It is not stretched with simulation speed.
