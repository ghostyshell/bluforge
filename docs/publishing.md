# Publishing builds and download links

Keepsake Atelier ships macOS builds through two free file hosts. GoFile is the
primary mirror and file.kiwi is the encrypted backup. GoFile expires free-tier
files, so the links are refreshed weekly by a scheduled job instead of being
edited by hand.

The same pattern is used by the other ghostyshell tools. The pieces are:

- `scripts/publish-release.sh` - build, upload, write the manifest, patch the
  README, commit and clean up.
- `scripts/patch-release-links.py` - turn a `links.txt` manifest into
  `docs/downloads.json` and the README table.
- `scripts/refresh-release-links.sh` - the weekly entry point (a thin wrapper
  with a lock and a log).
- `scripts/install-refresh-job.sh` - write and load the launchd job.
- `docs/downloads.json` - what the GitHub Pages site reads.
- `docs/index.html` - the landing page; its download cards are rendered from the
  JSON.

## One-time setup

1. Put a GoFile API token in the repo root:

   ```sh
   printf '%s' 'YOUR_TOKEN' > .gofile-io-token
   chmod 600 .gofile-io-token
   ```

   A token is a string, not a password. It is gitignored. The script also
   accepts `GOFILE_TOKEN` in the environment.

2. Confirm the uploaders work with a small file:

   ```sh
   head -c 2048 /dev/urandom > /tmp/test.bin
   curl -sS -X POST https://upload.gofile.io/contents/uploadfile \
     -H "Authorization: Bearer $(cat .gofile-io-token)" \
     -F "file=@/tmp/test.bin"
   npx -y @file-kiwi/node /tmp/test.bin --title test
   ```

3. Make the scripts executable:

   ```sh
   chmod +x scripts/publish-release.sh scripts/refresh-release-links.sh \
            scripts/install-refresh-job.sh
   ```

## Build and upload

```sh
scripts/publish-release.sh
```

What it does, in order:

1. Reads the version from `app/src-tauri/tauri.conf.json`.
2. Builds the app DMG and the CLI tarball into `dist/`.
3. Uploads both to GoFile (with retries) and file.kiwi.
4. Writes `.release-<version>-<date>/links.txt`, recording
   `name | gofile | kiwi | size | sha256`.
5. Regenerates `docs/downloads.json` and the README download table.
6. Commits and pushes the link update.
7. Deletes the local `dist/` artifacts only when both hosts returned a URL for
   every artifact. A failed upload leaves the files in place so a re-run can
   finish the job.

Environment switches:

- `GOFILE_TOKEN` - override the token file.
- `KEEPSAKE_REPO` - run against another checkout.
- `SKIP_PUSH=1` - commit locally and stop before `git push`.
- `KEEP_DIST=1` - do not delete `dist/` after a successful upload.

## Cross-platform artifacts

The macOS arm64 DMG and CLI are built locally. Intel macOS, Linux and Windows are
built by the GitHub Actions matrix:

```sh
gh workflow run build.yml -f platforms=all       # or macos, linux, windows
gh run watch
gh run download <run-id> -D /tmp/ci-all
```

The workflow stores one artifact directory per platform:

- `macos-arm64` / `macos-x64`: `Keepsake-Atelier-<v>-arm64.dmg`,
  `Keepsake-Atelier-<v>-x64.dmg`, and a `macos-<arch>` CLI tarball.
- `linux-x64`: `keepsake-atelier-<v>-linux-x64.tar.gz`.
- `windows-x64`: `keepsake-atelier-<v>-windows-x64.zip` (exe plus the MSYS2
  runtime DLLs).

Publish a downloaded set without rebuilding the macOS arm64 side:

```sh
mkdir -p /tmp/ci-flat
find /tmp/ci-all -type f \( -name '*.dmg' -o -name '*.tar.gz' -o -name '*.zip' \) \
  -exec cp {} /tmp/ci-flat/ \;
scripts/publish-release.sh --artifacts /tmp/ci-flat
```

`--artifacts` skips the build, uploads each file, rewrites the manifest and the
README, commits and pushes. The patcher merges by **artifact identity** (kind,
OS, arch), so a same-version row it did not rebuild survives; the weekly macOS
job cannot drop the Linux or Windows links. Older-version rows are discarded.

The local weekly job still rebuilds the macOS arm64 DMG and CLI. Run the
workflow again when you bump the version so every platform is regenerated.

## The weekly refresh

```sh
scripts/refresh-release-links.sh
```

The wrapper takes a lock so two runs cannot overlap, logs to
`.release-refresh.log`, and then calls the publisher against the version
currently in the config. It does not bump the version; a release is a deliberate
version change, a refresh only renews the mirrors.

The uploader is idempotent for a day: if `.release-<version>-<date>/links.txt`
already has a GoFile and a file.kiwi URL for an artifact, that artifact is
skipped.

## Install the scheduled job

```sh
scripts/install-refresh-job.sh
```

This writes `~/Library/LaunchAgents/click.keepsakeatelier.weekly-refresh.plist`
and loads it. The default is Sunday at 03:47 under `caffeinate -i`, so the Mac
does not sleep in the middle of an upload.

Options:

```sh
scripts/install-refresh-job.sh --time 04:15 --weekday 1
scripts/install-refresh-job.sh --repo /path/to/keepsake-atelier
scripts/install-refresh-job.sh --uninstall
```

launchd gets a thin `PATH`. The installer writes the current `PATH` plus
`/opt/homebrew/bin`, `/usr/local/bin`, `$HOME/.cargo/bin` and `$HOME/.local/bin`
into the plist, so `node`, `npx`, `cargo`, `caffeinate` and `gh` are found.

### Running it on the MacBook Air

The job is a normal launchd user agent, so it can live on any Mac that has the
checkout and the GoFile token. On the Air:

1. Clone the repo to the same absolute path, or pass `--repo` when installing.
2. Copy `.gofile-io-token` over (scp or a password manager).
3. Make sure `node`, `npx`, `cargo` and the Rust toolchain are installed.
4. Run `scripts/install-refresh-job.sh --repo /absolute/path`.

The job writes the repository, so the Air needs push access to
`origin/main` and an active `gh` account with the ghostyshell identity.

## What the manifest looks like

`.release-0.1.0-20261111/links.txt`:

```
Keepsake-Atelier-0.1.0-arm64.dmg | https://gofile.io/d/xxxx | https://file.kiwi/xxxx#yyyy | 32.1 MiB | 9f2c...
keepsake-atelier-0.1.0-arm64.tar.gz | https://gofile.io/d/zzzz | https://file.kiwi/zzzz#wwww | 8.4 MiB | 1a3b...
```

`docs/downloads.json` is generated from it and read by the site at runtime:

```json
{
  "version": "0.1.0",
  "updated": "2026-11-11",
  "artifacts": [
    {
      "label": "macOS (Apple Silicon)",
      "name": "Keepsake-Atelier-0.1.0-arm64.dmg",
      "gofile": "https://gofile.io/d/xxxx",
      "kiwi": "https://file.kiwi/xxxx#yyyy",
      "size": "32.1 MiB",
      "sha256": "9f2c..."
    }
  ]
}
```

## README and site

The README carries a generated block between `<!-- downloads:start -->` and
`<!-- downloads:end -->`. Do not hand-edit inside it; the publisher replaces it.

The site reads `downloads.json` with `cache: no-store` and falls back to a static
card if the file is missing. That means a link refresh is a data change, not an
HTML change.

## Security notes

- The GoFile token and any `gh` credentials stay on the machine that runs the
  job. They are never committed.
- The publisher only uploads `dist/` artifacts it just built. It does not walk
  arbitrary folders.
- The weekly job commits only `README.md` and `docs/downloads.json`.
- Review `git diff` after the first run before trusting the job unattended.
