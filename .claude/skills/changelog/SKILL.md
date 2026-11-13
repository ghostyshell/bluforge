---
name: changelog
description: Write the CHANGELOG entry for a change, in the house style, before any commit. Use whenever work in keepsake-atelier changes behaviour, docs, the build, or fixes a bug — the entry is written while the change is fresh, under "## Unreleased".
---

# Writing a changelog entry

Every behaviour change lands with an entry and a version bump. Both are checked
in CI (`.github/workflows/build.yml` → "Changelog and version bump") and by
`scripts/check-changelog.sh`.

## Where it goes

Under `## Unreleased` in `CHANGELOG.md`, in one of these subsections:

```md
### Added
### Changed
### Fixed
### Removed
```

Pick by what changed. If several kinds are involved, use several subsections.

## How to write it

- Lead with a bolded summary sentence: `- **Short name.** What changed.`
- Say what a user can now do, or what stopped happening, and how. Name the
  command, flag, key or page.
- Give the numbers when the change rests on measurement ("2 ridges on a clean
  control against 47-69 on watermarked dubs").
- Keep the honest limit in the same entry. A change that works on one title and
  not another says so.
- Two to six sentences. No marketing words, no "revolutionary", no exclamation
  marks.
- No author names or handles.

## Then

```sh
scripts/bump-version.sh patch   # or minor for features, major for breakage
scripts/check-changelog.sh
```

Commit the changelog and the version files together with the code. Never bump
the version without an entry, and never edit a released section: corrections go
in the open one.
