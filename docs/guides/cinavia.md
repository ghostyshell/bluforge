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
because a stream copy cannot carry the filter. The depth is chosen from the
measurement and re-checked after the pass; it deepens once if the band is still
above the clean level.

```sh
keepsake-atelier cinavia strip movie.mkv movie-clean.mkv
keepsake-atelier cinavia strip movie.mkv out.mkv --codec ac3      # keep Dolby Digital
keepsake-atelier cinavia strip movie.mkv out.mkv --codec dts      # keep DTS
keepsake-atelier cinavia strip movie.mkv out.mkv --track 2 --depth 12 --band 280-1300
keepsake-atelier cinavia strip movie.mkv out.mkv --aggressive     # whole 280-4000 Hz band
keepsake-atelier cinavia strip movie.mkv out.mkv --dry-run        # print the plan
```

## Keeping the source format

`--codec` (and `--cinavia-codec` on `rip`) picks the encoder. The default,
`auto`, follows the source track:

| source | `auto` result |
| --- | --- |
| AC-3 | AC-3, 192-640 kbps, at the source bitrate |
| E-AC-3 | E-AC-3, 192-1024 kbps, at the source bitrate |
| DTS core | DTS, 768-1536 kbps, at the source bitrate |
| Dolby TrueHD | TrueHD (lossless, ffmpeg's experimental encoder) |
| DTS-HD MA | FLAC, because no encoder exists for the extension |
| Dolby Atmos (TrueHD or E-AC-3) | FLAC: no Atmos encoder exists, so the objects are dropped and the bed stays lossless and 7.1 |
| LPCM / AAC / anything else | FLAC in MKV, AAC in MP4 |

The explicit values are `ac3`, `eac3`, `dts`, `truehd`, `flac` and `aac`, and a
hopeless pairing is rejected rather than silently substituted. Atmos is
detected from the track's profile or tags, shown in `detect` output, and always
flagged in the plan: ffmpeg has no Atmos (JOC) encoder, so an Atmos track comes
back as a 7.1 bed with objects discarded, whichever codec you choose. Keeping
FLAC is the least lossy option; the object metadata is gone in every case. Two constraints
are real: the bundled Dolby and DTS encoders take **5.1 at most**, so a 7.1
track is folded down (FLAC and AAC keep 7.1), and MP4 only carries AC-3, E-AC-3
and AAC, so `dts`/`truehd`/`flac` with `.mp4` become AAC with a note.

Re-encoding to the source format is lossy on top of the notch when the source
was lossy, and it also re-adds that codec's own noise. Measured on a watermarked
track, an AC-3 640k, E-AC-3 768k or DTS 1536k re-encode still read clean
(mass 23-24, in-band ratio 25 dB against a 27 dB threshold), so the codec noise
does not resurrect the watermark.

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
  long, and audio is re-encoded: a copy cannot carry the filter.
- The Dolby and DTS re-encoders are ffmpeg's own: `dca` and `truehd` are still
  marked experimental, the DTS encoder is the core only (no DTS-HD extension),
  and all four cap at 5.1. A 7.1 track is folded down unless you keep FLAC.
- Atmos object metadata is never re-created. Only the bed survives, at 7.1 if
  you keep FLAC and 5.1 if you pick a Dolby or DTS target.
- AC-3 and DTS have hard bitrate ceilings (640 kbps and 1536 kbps). Re-encoding
  a DTS-HD MA core to DTS at those rates is a quality step down from FLAC, so
  `auto` prefers FLAC for HD sources even though it is not the source family.
- Stereo-only titles give the detector one channel of evidence and are the
  likeliest to need `--band` by hand.

## References

- Verance's own description of the system, and the Wikipedia summary of the
  mechanism (phase-domain spread spectrum, per-channel, 400-4000 Hz, speed
  variation jammer).
- DVD-Ranger's 2013 CinEx white paper: the first published account of a
  working removal, and the source of the band and the jammer description.
- [shadowforge](https://github.com/greysquirr3l/shadowforge), an unrelated
  steganography toolkit, was used as a sanity check on the signal model. Its
  DSSS phase technique embeds at about 22% of the carrier RMS, which is
  consistent with the 5-15 dB ridge excess measured here, and it embeds per
  segment with an adaptive amplitude rather than a fixed one. It cannot decode
  Cinavia: its payloads use its own keys and sequence, so there is nothing to
  correlate against in a Verance track.

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
