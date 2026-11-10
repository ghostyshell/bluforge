# Rip a movie

## 1. Read the disc

```sh
keepsake-atelier devices
keepsake-atelier disc /Volumes/MY_DISC
```

`disc` prints the label, AACS state and every title with playlist, duration,
resolution, dynamic range and audio stream count. The main feature is usually
the longest title, and the app marks it for you.

Inspect one title in detail:

```sh
keepsake-atelier disc /Volumes/MY_DISC --title 2
```

## 2. Choose the output

Lossless remux (default):

```sh
keepsake-atelier rip /Volumes/MY_DISC --title 2
```

The file lands in your configured output folder, named from the disc metadata.
Pass a path to control it:

```sh
keepsake-atelier rip /Volumes/MY_DISC /Volumes/Media/Movie.mkv --title 2
```

Encode for size:

```sh
keepsake-atelier rip /Volumes/MY_DISC --encode --codec x265 --quality 20
```

- `--codec x265 | x264 | hevc_videotoolbox | h264_videotoolbox`
- `--quality` is CRF for software encoders and a quality value for VideoToolbox.
- `--preset` maps to the x264/x265 preset.
- `--ten-bit` selects a 10-bit software encode.

MP4 output:

```sh
keepsake-atelier rip /Volumes/MY_DISC --container mp4
```

## 3. Tracks and metadata

Defaults are chosen for you: English non-commentary audio, subtitles off, the
first video stream labelled and default. Override them in the app's track list.
From the CLI:

- `--no-chapters` drops chapter marks.
- `--no-poster` skips the TMDB poster.
- `--movie "Title"` forces the metadata lookup string.

Metadata is best effort. If TMDB has no match, the rip still completes and the
file is named from the disc label.

## 4. Queue several titles

The app's Titles card has **Queue all**, which queues every title of at least a
minute and skips menus and trailers. Jobs run one at a time and survive quitting
the app; progress is visible per job.

## 5. Check the result

Play the file, or inspect it:

```sh
ffprobe -v error -show_streams -show_format /path/Movie.mkv
```

Ripping never writes to the disc and does not need the writer. If the disc is
AACS and the key is missing, `disc` says so before a rip starts.

## When a title will not rip

- **No titles**: the disc may be AACS with no key, or the volume is not mounted.
  Run `keepsake-atelier plan` and read the backend line.
- **Zero duration on every title**: a BD-J first play is not a title. Look for
  the HDMV entries with real durations.
- **Copy protection errors mid-rip**: clean the disc. A failing outer edge is
  usually a scratch, not a software problem.
