#!/usr/bin/env python3
"""Rewrite the Keepsake Atelier download links from a fresh upload manifest.

Usage: python3 -I scripts/patch-release-links.py <links.txt>

`links.txt` rows are `name | gofile-url | kiwi-url | size | sha256`, one per
artifact. This script writes:

  - docs/downloads.json (the GitHub Pages site reads it at runtime)
  - the generated table between <!-- downloads:start/end --> in README.md
  - <lastmod> in docs/sitemap.xml

A row whose URL is missing or `EMPTY` still appears, with `pending` in the docs,
so a failed mirror does not blank out the half that worked.
"""
import datetime
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
README = ROOT / "README.md"
MANIFEST = ROOT / "docs" / "downloads.json"
SITEMAP = ROOT / "docs" / "sitemap.xml"
START = "<!-- downloads:start -->"
END = "<!-- downloads:end -->"
VERSION_RE = re.compile(r"\d+\.\d+\.\d+")


def label_for(name):
    low = name.lower()
    if low.endswith(".dmg"):
        return "macOS (Intel)" if "x64" in low else "macOS (Apple Silicon)"
    if low.endswith(".tar.gz"):
        if "linux" in low:
            return "Linux (x86_64, CLI)"
        if "windows" in low:
            return "Windows (x86_64, CLI)"
        return "macOS (Intel, CLI)" if "x64" in low else "macOS (Apple Silicon, CLI)"
    if low.endswith(".zip"):
        if "windows" in low:
            return "Windows (x86_64, CLI)"
        return "macOS app (zip)"
    if low.endswith(".appimage"):
        return "Linux (x86_64, AppImage)"
    if low.endswith(".deb"):
        return "Linux (x86_64, deb)"
    if low.endswith(".exe") and "windows" in low:
        if "setup" in low or "installer" in low:
            return "Windows (x86_64, installer)"
        return "Windows (x86_64)"
    return name


def identity(name):
    """Version-independent artifact slot, so a renamed build replaces the old row."""
    low = name.lower()
    if low.endswith(".dmg"):
        kind, osname = "dmg", "macos"
    elif low.endswith(".tar.gz"):
        kind = "cli"
        osname = "linux" if "linux" in low else "windows" if "windows" in low else "macos"
    elif low.endswith(".zip"):
        kind, osname = "bundle", "windows"
    elif low.endswith(".appimage"):
        kind, osname = "appimage", "linux"
    elif low.endswith(".deb"):
        kind, osname = "deb", "linux"
    elif low.endswith(".exe") and "windows" in low:
        kind, osname = "installer", "windows"
    else:
        return name
    arch = "arm64" if ("arm64" in low or "aarch64" in low) else "x64"
    return f"{kind}:{osname}:{arch}"


def order(name):
    low = name.lower()
    if low.endswith(".dmg"):
        return (0 if "x64" not in low else 1, name)
    if low.endswith(".tar.gz"):
        if "linux" in low:
            return (4, name)
        return (2 if "x64" not in low else 3, name)
    if low.endswith(".zip"):
        return (7, name)
    if low.endswith(".appimage"):
        return (5, name)
    if low.endswith(".deb"):
        return (6, name)
    if low.endswith(".exe") and "windows" in low:
        return (8, name)
    return (9, name)


def load(path):
    rows = {}
    for line in Path(path).read_text().splitlines():
        parts = [p.strip() for p in line.split("|")]
        if len(parts) < 5:
            continue
        name, gofile, kiwi, size, sha = parts[:5]
        if not name:
            continue
        if not (gofile.startswith("http") or kiwi.startswith("http")):
            continue
        rows[name] = {
            "name": name,
            "gofile": "" if gofile == "EMPTY" else gofile,
            "kiwi": "" if kiwi == "EMPTY" else kiwi,
            "size": "" if size == "EMPTY" else size,
            "sha256": "" if sha == "EMPTY" else sha,
        }
    return rows


def detect_version(rows, fallback):
    for name in rows:
        match = VERSION_RE.search(name)
        if match:
            return match.group(0)
    return fallback


def os_arch(name):
    low = name.lower()
    if low.endswith(".dmg"):
        return "macOS", "x64" if "x64" in low else "arm64"
    if low.endswith(".tar.gz"):
        if "linux" in low:
            return "Linux", "x86_64"
        if "windows" in low:
            return "Windows", "x86_64"
        return "macOS", "x64" if "x64" in low else "arm64"
    if low.endswith(".zip"):
        return "Windows", "x86_64"
    if low.endswith(".appimage") or low.endswith(".deb"):
        return "Linux", "x86_64"
    if low.endswith(".exe") and "windows" in low:
        return "Windows", "x86_64"
    return "macOS", "arm64"


def collapse(rows):
    """One artifact per identity; later rows (a fresh upload) win."""
    chosen = {}
    for name, row in rows.items():
        chosen[identity(name)] = (name, row)
    return {name: row for name, row in chosen.values()}


def build_manifest(rows, version, preserved=None):
    merged = dict(preserved or {})
    for name, row in rows.items():
        merged[name] = row
    artifacts = []
    for name in sorted(merged, key=order):
        row = merged[name]
        osname, arch = os_arch(name)
        artifacts.append(
            {
                "label": label_for(name),
                "name": name,
                "version": version,
                "os": osname,
                "arch": arch,
                "gofile": row.get("gofile", ""),
                "kiwi": row.get("kiwi", ""),
                "size": row.get("size", ""),
                "sha256": row.get("sha256", ""),
            }
        )
    today = datetime.date.today().isoformat()
    return {
        "version": version,
        "updated": today,
        "artifacts": artifacts,
    }


def table(manifest):
    lines = [
        "| Build | GoFile (primary) | file.kiwi (backup) | Size |",
        "| --- | --- | --- | --- |",
    ]
    for art in manifest["artifacts"]:
        name = art["name"]
        gofile = f"[Download]({art['gofile']})" if art["gofile"] else "pending"
        kiwi = f"[Mirror]({art['kiwi']})" if art["kiwi"] else "pending"
        size = art["size"] or "pending"
        lines.append(f"| `{name}` | {gofile} | {kiwi} | {size} |")
    return "\n".join(lines)


def patch_readme(manifest):
    text = README.read_text()
    start = text.find(START)
    end = text.find(END)
    if start < 0 or end < 0 or end < start:
        raise SystemExit(
            "README.md is missing the <!-- downloads:start/end --> markers"
        )
    body = f"{START}\n{table(manifest)}\n{END}"
    README.write_text(text[:start] + body + text[end + len(END):])


def patch_sitemap(today):
    if not SITEMAP.is_file():
        return
    text = SITEMAP.read_text()
    text = re.sub(r"<lastmod>[^<]*</lastmod>", f"<lastmod>{today}</lastmod>", text, count=1)
    SITEMAP.write_text(text)


def main(argv):
    if len(argv) != 2:
        raise SystemExit(__doc__)
    rows = collapse(load(argv[1]))
    if not rows:
        raise SystemExit("no usable rows in the link manifest - run the upload first")

    previous = {}
    if MANIFEST.is_file():
        try:
            previous = json.loads(MANIFEST.read_text())
        except Exception:
            previous = {}
    version = detect_version(rows, previous.get("version", "0.0.0"))
    # Keep current-version artifacts this run did not rebuild (for example the
    # Linux and Windows CLI builds from CI) unless a row now occupies the same
    # slot. Older-version rows are dropped.
    new_ids = {identity(name) for name in rows}
    preserved = {}
    for art in previous.get("artifacts", []):
        name = art.get("name", "")
        if not name or art.get("version") != version:
            continue
        if identity(name) in new_ids:
            continue
        if not (art.get("gofile") or art.get("kiwi")):
            continue
        preserved[name] = {
            "name": name,
            "gofile": art.get("gofile", ""),
            "kiwi": art.get("kiwi", ""),
            "size": art.get("size", ""),
            "sha256": art.get("sha256", ""),
        }
    manifest = build_manifest(rows, version, preserved)

    MANIFEST.parent.mkdir(parents=True, exist_ok=True)
    MANIFEST.write_text(json.dumps(manifest, indent=2) + "\n")
    patch_readme(manifest)
    patch_sitemap(manifest["updated"])

    print(f"wrote {MANIFEST.relative_to(ROOT)} and the README table for {version}")
    for art in manifest["artifacts"]:
        print(
            f"  {art['name']}: "
            f"gofile={'yes' if art['gofile'] else 'no'} "
            f"kiwi={'yes' if art['kiwi'] else 'no'}"
        )


def demo():
    rows = {
        "Keepsake-Atelier-2.0.0-arm64.dmg": {
            "name": "Keepsake-Atelier-2.0.0-arm64.dmg",
            "gofile": "https://gofile.io/d/NEW",
            "kiwi": "",
            "size": "32.1 MiB",
            "sha256": "abc",
        },
        "keepsake-atelier-2.0.0-arm64.tar.gz": {
            "name": "keepsake-atelier-2.0.0-arm64.tar.gz",
            "gofile": "https://gofile.io/d/CLI",
            "kiwi": "https://file.kiwi/x",
            "size": "8.4 MiB",
            "sha256": "def",
        },
    }
    m = build_manifest(rows, "2.0.0")
    assert m["version"] == "2.0.0"
    assert m["artifacts"][0]["label"] == "macOS (Apple Silicon)"
    assert m["artifacts"][1]["label"] == "macOS (Apple Silicon, CLI)"
    t = table(m)
    assert "| `Keepsake-Atelier-2.0.0-arm64.dmg` | [Download](https://gofile.io/d/NEW) | pending | 32.1 MiB |" in t
    assert "[Mirror](https://file.kiwi/x)" in t
    print("selftest OK")


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "--selftest":
        demo()
    else:
        main(sys.argv)
