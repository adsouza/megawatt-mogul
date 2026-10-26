#!/usr/bin/env python3
"""Regenerate the game's original tonal cues (Python stdlib + ffmpeg)."""
import array
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import wave

RATE = 44_100
OUTPUT = Path(__file__).resolve().parents[2] / "assets/audio"
TAU = 2 * math.pi


def bell(t, frequency, decay):
    """A struck metallic resonator: higher, inharmonic modes decay faster."""
    if t < 0:
        return 0.0
    modes = [(1, 1, 1), (2.76, .3, .6), (5.4, .12, .35), (8.93, .04, .2)]
    attack = 1 - math.exp(-t * 350)
    return attack * sum(
        gain * math.exp(-t / (decay * sustain)) * math.sin(TAU * frequency * ratio * t)
        for ratio, gain, sustain in modes
    )


def metro(t):
    # Original ascending three-note platform signal, shared by both workday cues.
    return sum(bell(t - start, frequency, .48)
               for start, frequency in [(0, 659.25), (.28, 783.99), (.56, 1046.5)])


def gong(t):
    # Low, gently beating inharmonic modes and a slow metallic bloom.
    modes = [(110, 1, 2.2), (173.3, .6, 2.8), (233.2, .4, 1.8),
             (338.8, .22, 1.4), (492.1, .12, .9), (773, .07, .55)]
    return (1 - math.exp(-t * 65)) * sum(
        gain * math.exp(-t / decay) * math.sin(TAU * frequency * t + .15 * math.sin(TAU * 1.3 * t))
        for frequency, gain, decay in modes
    )


def doorbell(t):
    return bell(t, 783.99, .8) + bell(t - .65, 622.25, 1.05)


def render(name, duration, sound, temp):
    samples = [sound(i / RATE) for i in range(round(duration * RATE))]
    peak = max(abs(sample) for sample in samples)
    pcm = array.array("h", (
        round(32767 * .68 * sample / peak * min(1, (duration - i / RATE) / .15))
        for i, sample in enumerate(samples)
    ))
    if sys.byteorder != "little":
        pcm.byteswap()
    wav_path = temp / f"{name}.wav"
    with wave.open(str(wav_path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        wav.writeframes(pcm.tobytes())
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(wav_path),
                    "-ac", "2", "-c:a", "vorbis", "-strict", "experimental", "-b:a", "128k",
                    str(OUTPUT / f"{name}.ogg")], check=True)


def compose_noon():
    # One clip guarantees the laugh follows the gong immediately, at natural speed.
    # Gains are baked in; the game plays this combined clip at unity gain.
    with tempfile.TemporaryDirectory() as directory:
        wav = str(Path(directory) / "noon.wav")
        subprocess.run([
            "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
            "-i", str(OUTPUT / "gong.ogg"), "-i", str(OUTPUT / "children-laughing.ogg"),
            "-filter_complex", "[0:a]volume=0.85[gong];[1:a]volume=0.45[kids];"
            "[gong][kids]concat=n=2:v=0:a=1[noon]", "-map", "[noon]",
            "-ac", "2", "-c:a", "pcm_s16le", wav,
        ], check=True)
        # Quantize the intermediate: the experimental encoder can assert on the float concat.
        subprocess.run([
            "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", wav,
            "-c:a", "vorbis", "-strict", "experimental", "-b:a", "128k",
            str(OUTPUT / "noon.ogg"),
        ], check=True)



if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as directory:
        temp = Path(directory)
        for name, duration, sound in [("metro-chime", 2.4, metro), ("gong", 5.5, gong),
                                      ("doorbell", 3.2, doorbell)]:
            render(name, duration, sound, temp)
        compose_noon()
