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
    if name.endswith(".dmg"):
        return "macOS (Apple Silicon)"
    if name.endswith(".tar.gz"):
        return "Command line (Apple Silicon)"
    if name.endswith(".zip"):
        return "macOS app (zip)"
    return name


def order(name):
    if name.endswith(".dmg"):
        return (0, name)
    if name.endswith(".tar.gz"):
        return (1, name)
    return (2, name)


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


def build_manifest(rows, version):
    artifacts = []
    for name in sorted(rows, key=order):
        row = rows[name]
        artifacts.append(
            {
                "label": label_for(name),
                "name": name,
                "version": version,
                "os": "macOS",
                "arch": "arm64",
                "gofile": row["gofile"],
                "kiwi": row["kiwi"],
                "size": row["size"],
                "sha256": row["sha256"],
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
    rows = load(argv[1])
    if not rows:
        raise SystemExit("no usable rows in the link manifest - run the upload first")

    previous = {}
    if MANIFEST.is_file():
        try:
            previous = json.loads(MANIFEST.read_text())
        except Exception:
            previous = {}
    version = detect_version(rows, previous.get("version", "0.0.0"))
    manifest = build_manifest(rows, version)

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
    assert m["artifacts"][1]["label"] == "Command line (Apple Silicon)"
    t = table(m)
    assert "| `Keepsake-Atelier-2.0.0-arm64.dmg` | [Download](https://gofile.io/d/NEW) | pending | 32.1 MiB |" in t
    assert "[Mirror](https://file.kiwi/x)" in t
    print("selftest OK")


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "--selftest":
        demo()
    else:
        main(sys.argv)
