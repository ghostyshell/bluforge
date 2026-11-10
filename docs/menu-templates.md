# Disc menus and templates

A Blu-ray disc has two things people call "the menu":

1. the **navigation table** in `BDMV/index.bdmv`, which says what runs on insert
   (first playback), what the top-menu button opens, and which movie object or
   BD-J object backs each title; and
2. the **playable titles** libbluray enumerates (playlist, duration, chapters,
   streams).

Keepsake Atelier parses both into a *menu document*, which can be edited, saved as a
named template, and applied to a different disc.

## What a menu document holds

```json
{
  "version": 1,
  "label": "The Jungle Book Diamond Edition - Blu-ray™",
  "disc_id": "784a72ec…",
  "source": "/Volumes/THE_JUNGLE_BOOK",
  "navigation": {
    "first_playback": { "kind": "bdj", "name": "65535" },
    "top_menu":       { "kind": "bdj", "name": "00000" },
    "titles": [ { "kind": "bdj", "name": "00001" },
                { "kind": "hdmv", "object": 1 } ]
  },
  "titles": [
    {
      "index": 50,
      "playlist": 800,
      "duration_secs": 4707.16,
      "chapters": 25,
      "name": "The Jungle Book",
      "main": true,
      "chapter_names": ["Opening", "…"],
      "resolution": "1080p",
      "audio_streams": 4
    }
  ]
}
```

Editable fields: `name`, `main`, `chapter_names` per title, and any part of the
JSON by hand. Everything under `navigation` is disc-specific and informational;
`apply` never copies it onto another disc.

## The `index.bdmv` layout

The file is `INDX0200`, a `u32` offset to `Indexes()` at byte 8, then a
`u32` extension-data offset and reserved bytes. `Indexes()` is self-describing:

```
u32 length                         (excludes these 4 bytes)
entry first_playback
entry top_menu
u16 number_of_titles
entry titles[number_of_titles]
```

Each entry on the discs seen so far is 12 bytes: a type byte (top two bits: 0
none, 1 HDMV, 2 BD-J), flags, a `u16` object id at offset 6, and, for BD-J, the
five ASCII digits of the BD-J object name (`00001`) in the same slot.

The parser tries the 12-, 6- and 8-byte strides and accepts the one whose
arithmetic (`4 + 2*stride + 2 + count*stride`) matches the declared length
exactly, so a legacy layout still parses. The real Jungle Book file is the
regression fixture in `core/src/menu.rs`.

## Commands

```sh
# Parse a mounted disc or a BDMV folder. `--json` emits the document.
keepsake-atelier menu parse /Volumes/MY_DISC
keepsake-atelier menu parse /Volumes/MY_DISC --save my-disc

# Templates live in ~/Library/Application Support/keepsake-atelier/menus/
keepsake-atelier menu list
keepsake-atelier menu show my-disc
keepsake-atelier menu show my-disc --json

# Edit in place: rename title 51, make it the main feature, label chapters.
keepsake-atelier menu edit my-disc --title 51 --name "The Jungle Book"
keepsake-atelier menu edit my-disc --title 51 --main
keepsake-atelier menu edit my-disc --title 51 --chapter 1:"Opening Credits" --chapter 2:"Bare Necessities"

# Apply the saved document to a different disc.
keepsake-atelier menu apply my-disc /Volumes/OTHER_DISC
keepsake-atelier menu apply my-disc /Volumes/OTHER_DISC --save other-disc

keepsake-atelier menu remove my-disc
```

`show`, `edit` and `apply` accept either a template name or a path to a `.json`
document, so a hand-authored or exported document works the same way.

## How `apply` matches titles

For each template title, in order:

1. same **playlist id** among the target's unused titles;
2. **duration** within one second;
3. the same **position**, only when both discs have the same title count.

Two template titles can never claim the same target title. Chapter labels are
copied only when the chapter counts match; otherwise the target keeps its own.
If the template marks a main feature, the matched target title becomes the
single main feature.

This is what makes a template reusable across editions of the same film (same
playlists) and across a different disc of the same show (same durations and
order), while a completely unrelated disc simply matches nothing and is left
unchanged.

## Limits

- Parsing reads files only; it never sends SCSI commands and never writes to the
  disc.
- An ISO must be mounted first (`hdiutil attach`), because the parser reads
  `BDMV/index.bdmv` through the filesystem.
- BD-J menus are Java applications in `JAR/`. Keepsake Atelier records the BD-J object
  names from the navigation table but does not run or parse the Java UI.
- Enumerating titles needs libbluray to open the disc, which needs a key for an
  AACS disc. Without one the document still contains the navigation table and a
  `note` explaining that titles were unavailable.
