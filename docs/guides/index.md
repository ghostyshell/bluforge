# Guides

Task-shaped pages for Keepsake Atelier. Each one ends with the commands to run
and the check that tells you it worked.

| Guide | What it covers | You need |
| --- | --- | --- |
| [Install](install.md) | Download, Gatekeeper, first launch, drive check | macOS 12+, Apple Silicon |
| [Rip a movie](rip.md) | Pick a title, choose tracks, remux or encode | A Blu-ray drive and disc |
| [Author a disc](author.md) | One or many titles, folders and ISOs, splitting | Finished MKV/MP4 files, tsMuxeR |
| [Menus](menus.md) | BD-J title menu, background, templates, checks | tsMuxeR |
| [Keys and AACS](keys.md) | KEYDB, native recovery, AACS2, LibreDrive | A drive, optionally a KEYDB |
| [DVD-Video](dvd.md) | List and rip DVD titles | A DVD drive and disc |
| [TV and anime](tv.md) | Episode split, SxxExx names, openings and endings | A disc with episodes |
| [Burn and verify](burn.md) | Size query, dry run, write, read back | A writer and blank BD-RE/BD-R |
| [Automation](automation.md) | Auto-rip on insertion, watch folders, notifications |
| [Cinavia](cinavia.md) | Measure and remove the Verance watermark |
| [Logs](logs.md) | Read run detail and failure output |
| [MCP](mcp.md) | Drive the app from Claude, Cursor or a script | The app running |
| [Troubleshooting](troubleshooting.md) | The errors people actually hit | - |

## The shortest path

```sh
# see what is on the disc
keepsake-atelier disc /Volumes/MY_DISC

# copy the main feature losslessly
keepsake-atelier rip /Volumes/MY_DISC

# check the drive, keys and helpers at any time
keepsake-atelier doctor
```

The desktop app exposes the same steps as cards. The CLI is the reference; use
`--json` on any command when another tool needs to read the result.

## Related reading

- [Features](../features.md) - the complete capability list.
- [Publishing](../publishing.md) - how release builds and download mirrors work.
- [Roadmap](../roadmap.md) - what is done and what is next.
- [README](../../README.md) - overview, install and support.
