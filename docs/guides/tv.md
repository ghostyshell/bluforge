# TV and anime

A TV disc is usually a pile of episodes, openings, endings, previews and menus.
The TV planner turns the title list into named episode files.

## Plan first

```sh
keepsake-atelier tv plan /Volumes/MY_DISC --start 1
```

The plan prints the series, season, disc and the episode ranges it inferred. It
writes nothing. Adjust `--start` if the disc is not the first of a season.

## Rip

```sh
keepsake-atelier rip /Volumes/MY_DISC --tv --start 1
```

Output names follow the configured TV template, for example
`Show - S01E03 - Episode Title.mkv`. Metadata comes from TMDB when a match is
found.

## Openings and endings

Anime discs often put an opening, an ending and a preview on the disc as their
own titles, or as chapter blocks inside an episode. The planner detects cold
opens and standalone openings, labels them, and avoids gluing an opening onto
the previous episode. You can inspect the labels in the plan output before
ripping.

## Multi-disc seasons

Use `--disc N` when the set has several discs. The planner keeps episode numbers
continuous across discs.

```sh
keepsake-atelier tv plan /Volumes/SHOW_DISC2 --start 1 --disc 2
```

## When the split is wrong

- **Everything is one file**: the disc has one title with chapters, not separate
  episode titles. Use the chapter-based split mode.
- **An extra file**: a preview or menu was included. Raise the minimum duration
  or drop the title from the queue.
- **Wrong episode numbers**: pass `--start` and `--disc` explicitly.
