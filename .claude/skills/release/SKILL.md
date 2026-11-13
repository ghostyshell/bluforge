---
name: release
description: Cut and publish a keepsake-atelier release — bump the version, date the changelog, run the full platform matrix, verify every artifact, publish with scripts/publish-release.sh, then confirm the links. Use when asked to release, publish, ship, or bump the version.
---

# Release runbook

Version moves in one direction and only with an entry. The guard is
`scripts/check-changelog.sh`; CI runs it as the "Changelog and version bump"
job.

## 1. Version and changelog

```sh
scripts/check-changelog.sh          # shows what is missing
scripts/bump-version.sh minor       # features; patch for fixes, major for breakage
./scripts/check-changelog.sh v$(git describe --tags --abbrev=0 | tr -d v)
```

The bump must leave `Unreleased` empty and create the dated section for the new
version; the guard enforces both.

That dates the `Unreleased` section, opens a new empty one, and writes the
version to `Cargo.toml`, `app/src-tauri/tauri.conf.json` and
`app/package.json`. `scripts/version.sh get|set|bump` does the pieces alone if
you need them.

## 2. Build the whole matrix

```sh
gh auth switch --user ghostyshell
git push origin main
gh workflow run build.yml --ref main -f platforms=all
```

Every job must be green. A red job is fixed and the matrix re-run — no partial
releases, and never publish artifacts from a run whose `headSha` is not the
commit being shipped.

## 3. Verify before publishing

- `gh run download` every artifact for that run.
- macOS CLIs: `otool -l` shows `minos 12.0` on the binary, `lib/` and `ffmpeg`,
  and `otool -L` has no Homebrew paths; each CLI runs `--version`.
- Both DMGs mount and detach.
- Linux tarball: no `libc`/`libmvec` in `lib/`.
- Smoke the new feature by name in the binary (`strings … | grep <marker>`)
  when the run added one.

## 4. Publish

```sh
scripts/publish-release.sh --artifacts ~/relpub
grep -c '\[done\]' <log>            # one per artifact
```

The script uploads to GoFile and file.kiwi, rewrites `docs/downloads.json` and
the README table, and commits. Keep the artifact directory until step 5 passes.

## 5. Confirm

- Every `sha256` in `docs/downloads.json` matches the local file.
- Every link returns `200` (both mirrors, all artifacts).
- `https://ghostyshell.github.io/keepsake-atelier/downloads.json` is
  byte-identical to the committed file.
- Remove the previous staging directory once confirmed.

## Rules

- Version, changelog and code ship in the same commit.
- A release never reuses a version that already has published artifacts.
- `docs/downloads.json` is generated; do not hand-edit it.
