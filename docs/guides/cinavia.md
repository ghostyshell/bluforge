# Cinavia

Cinavia is Verance's audio watermark for film and Blu-ray. Players that support
it mute the audio, or stop playback, when a copy is played without the matching
AACS key. It is a technological protection measure, so a "clean" file is useful
when you want the disc's audio to play anywhere.

Keepsake Atelier measures and removes the watermark without a licence key. It
does not decode the payload.

## What the watermark is

The details below are from Verance's own account and from DVD-Ranger's 2013
CinEx white paper, the first published description of a working remover:

- A narrow-band spread-spectrum signal is mixed into **400-4000 Hz**.
- It is **amplitude modulated by the programme**, so it scales with loudness.
- The encoder adds **slow per-band speed variations of up to ±0.2%** as a
  jammer: a correlator has to resynchronise before it can read anything.
- The payload is a redundant, error-corrected symbol stream at roughly three
  bits per fifteen seconds. Reading it needs the secret sequence.

Because the signal scales with the programme, it is inaudible in loud passages
and becomes the dominant content in quiet ones. That is what this implementation
exploits: it looks at the quietest seconds of a track, where programme material
has stopped and the watermark has not.

## Detection

`cinavia detect` decodes the centre channel (or the only channel on stereo),
averages the spectrum of the quietest minute, subtracts the local spectral tilt
and counts narrow ridges left above the floor.

Measured on a clean control and on three independently dubbed tracks of one
watermarked title:

| | clean control | watermarked dubs |
| --- | --- | --- |
| Ridges in 300-600 Hz | 2 | 47-69 |
| Ridge mass (dB above floor) | 1 | 80-93 |
| In-band / high-band level | 22 dB | 36-40 dB |

The same ridge field appears on every dub because each is watermarked from the
same master, which is why the check needs all three numbers and not just a
single tone.

```sh
keepsake-atelier cinavia detect movie.mkv
keepsake-atelier cinavia detect movie.mkv --track 1 --json
```

The verdict is advisory: `clean`, `watermark likely`, `watermark present`. A
track with a continuous musical drone in the low mids can score `likely`
without a watermark.

## Removal

`cinavia strip` cuts the detected band with raised-cosine edges, adds a
shallower cut across the rest of the watermark band, and re-encodes the audio
(FLAC in MKV, AAC in MP4) because a stream copy cannot carry the filter. The
depth is chosen from the measurement and re-checked after the pass; it deepens
once if the band is still above the clean level.

```sh
keepsake-atelier cinavia strip movie.mkv movie-clean.mkv
keepsake-atelier cinavia strip movie.mkv out.mkv --track 2 --depth 12 --band 280-1300
keepsake-atelier cinavia strip movie.mkv out.mkv --aggressive   # whole 280-4000 Hz band
keepsake-atelier cinavia strip movie.mkv out.mkv --dry-run      # print the plan
```

Video and subtitles are copied untouched; only the chosen audio track is
re-encoded. Metadata and chapters carry over.

In the app, **Strip Cinavia** sits with the other audio options on the Rip page,
and **System -> Automation -> Strip Cinavia from automatic rips** applies it to
watched discs. On the command line, `rip --cinavia` and `watch --cinavia` do the
same; the config key is `cinavia`.

## How good it is

Honest limits, in the same order the white paper puts them:

- Cinavia **irreversibly** alters the audio. A remover models what the programme
  probably looked like and removes the model; it cannot restore the original.
- The cut costs level and some brightness. A default pass typically drops
  300-1300 Hz by 8-14 dB and the rest of the band by about 4 dB. Volume
  compensation is expected; `--depth` trades residue against dullness.
- Removing the watermark cannot be verified without a Cinavia-aware player. The
  score falling to the clean level is evidence, not proof. A PS3 or a recent
  standalone player is the real test.
- Stripping is a second disc read, so an automatic rip takes about twice as
  long, and audio is re-encoded: a 5.1 DTS-HD MA track becomes 5.1 FLAC rather
  than a copy.
- Stereo-only titles give the detector one channel of evidence and are the
  likeliest to need `--band` by hand.

## Why a notch and not a decoder

Reading the payload means reimplementing Verance's synchroniser and correlator
against a secret sequence. The pieces are public in outline (phase-modulated
spread spectrum, 15-second blocks, convolutional coding) but not in detail, and
a decoder that mis-syncs by a fraction of a symbol is worse than useless.

A notch removes the energy a correlator integrates over, so it defeats the
player without ever knowing what the bits said. That is what the commercial
removers converged on too: filter, then post-process. The difference is that
theirs is tuned by ear on a large corpus; this one is tuned by measurement on a
small one.
