# DVD-Video

DVD support is title extraction, not a DVD menu rebuild. It uses the bundled
ffmpeg `dvdvideo` demuxer.

## List the titles

```sh
keepsake-atelier dvd list /Volumes/DVD_NAME
```

The listing shows title numbers, durations and the audio/subtitle layout.

## Rip a title

```sh
keepsake-atelier dvd rip /Volumes/DVD_NAME --title 0
```

By default the output goes to your configured folder with the disc label and
title number. Pass a path to control it:

```sh
keepsake-atelier dvd rip /Volumes/DVD_NAME --title 0 --out /Volumes/Media/Movie.mkv
```

Subtitles are included when `--subs` is passed. Video is copied as MPEG-2 and
audio as AC-3 or DTS. There is no re-encode and no menu traversal.

## Notes

- CSS-protected DVDs are not supported. The rip fails and reports the error
  instead of writing a truncated file.
- Multi-angle titles rip the default angle.
- Chapter marks are not written for DVD titles yet.
