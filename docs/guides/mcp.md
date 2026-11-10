# MCP

The app can expose every capability as an MCP tool on localhost. It is off by
default, binds to loopback, and destructive tools require explicit
confirmation.

## Enable it

In the app, open the sidebar and turn on **MCP server**. The app shows the port.
In config:

```json
{ "mcp_enabled": true, "mcp_port": 8765 }
```

Over the CLI:

```sh
keepsake-atelier mcp --port 8765
```

## Connect a client

```sh
claude mcp add --transport http keepsake http://127.0.0.1:8765/mcp
```

Cursor and other MCP clients take the same URL. The server refuses LAN clients
and cross-origin browser requests.

## What you can drive

- `devices`, `disc_info`, title and stream inspection
- `rip_start`, encode settings, queue and cancel
- `author_start` with `menu`, `menu_background`, `menu_template`
- `verify_tree`, `iso_export`, `clone_start`
- `keys_status`, `keys_native`, `keys_search`
- `dvd_info`, `dvd_start`, `burn_start`, `eject`
- `covers_search`, `covers_save`
- `menu_parse`, `menu_list`, `menu_show`, `menu_edit`, `menu_apply`, `menu_remove`

## Safety rules

- Rips and exports never touch the disc.
- `author_start` writes only to the destination you pass.
- `burn_start` is dry run unless `confirm` is true, and the CLI's `--yes` is the
  only other way to write.
- No tool uploads anything. Network endpoints are metadata and the release
  mirrors the project publishes.

## Example

```text
Use keepsake to inspect /Volumes/MY_DISC, pick the longest title, rip it
losslessly to /Volumes/Media, then verify the file's chapter count.
```

The assistant calls `disc_info`, `rip_start` and `job_status`. Destructive calls
stop at the confirmation boundary.
