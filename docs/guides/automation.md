# Automation

Rip on insertion, watch a folder, and get notified when a job ends. Everything
here is opt-in.

## Desktop app

**System -> Automation**:

- **Auto-rip a disc when it is inserted** polls the optical drive list, waits
  six seconds for the volume to settle, opens the disc and queues it.
- **Titles** chooses the main feature or every title at least the minimum
  length (menus and trailers below that floor are skipped; a short single-title
  disc still falls back to its main title).
- **Eject when the queue finishes** releases the drive after the last job.
- **Desktop notification** and **notification webhook** fire on completion and
  failure.
- **Keep the machine awake while ripping** uses `caffeinate` on macOS,
  `systemd-inhibit` on Linux, and the Windows execution-state API.

The notification webhook receives a JSON body:

```json
{ "title": "Rip complete", "body": "Movie -> /path/out.mkv", "app": "keepsake-atelier" }
```

The app queues one job per selected title through the same queue the buttons
use, so progress, cancel and the session inspector behave identically.

## CLI watcher

```sh
keepsake-atelier watch                 # poll drives and watch_dir forever
keepsake-atelier watch --once          # one scan, then exit
keepsake-atelier watch --dry-run       # print what would be ripped
keepsake-atelier watch --source /path/to/disc.iso   # one source, then exit
```

The watcher:

- Keys each disc by name, title count and total runtime, so re-inserting the
  same disc does not rip it twice, while a different disc in the same drive
  does.
- Ignores a drive with no media.
- Reads `watch_dir` from the config and processes new `.iso` files and BDMV
  folders once their modification time is at least ten seconds old, so a file
  still being copied in is not opened half-written.
- Uses the same title-selection rules as the app (`main` or `all`).
- Remuxes to MKV by default, keeping every audio and subtitle track.
- Ejects after the rips when `auto_rip_eject` is set.
- Sends the configured notifications.

Run it from `launchd` on macOS or a user service on Linux to get an
always-on ripping station.

## Config keys

| Key | Default | Meaning |
| --- | --- | --- |
| `auto_rip` | `false` | The app queues inserted discs |
| `auto_rip_mode` | `main` | `main` or `all` |
| `auto_rip_min_secs` | `600` | Length floor for `all` |
| `auto_rip_eject` | `false` | Eject when the queue drains |
| `watch_dir` | unset | Folder the CLI watcher scans |
| `notify_native` | `true` | Desktop notification |
| `notify_webhook` | unset | POST target for job events |
| `prevent_sleep` | `true` | Inhibit sleep during rips |

Set them in **System -> Automation**, or through MCP `patch_settings` with the
same names.

## Limits

- The watcher rips the main feature by default. It does not invent TV series
  names or episode mappings; use the Episodes page for a season disc.
- A failed automatic rip is reported, not retried indefinitely. The disc stays
  marked as handled so a bad disc cannot loop.
- One optical drive is assumed for ejection. Multi-drive ripping still needs
  the queue buttons or one watcher per drive.
