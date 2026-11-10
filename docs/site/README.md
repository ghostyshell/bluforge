# Keepsake Atelier - project site

A single-file static landing page for Keepsake Atelier, plus the assets GitHub
Pages serves with it.

## Layout

- `docs/index.html` - the page. Hand-written HTML and CSS, no build step.
- `docs/site/` - assets referenced by the page:
  - `icon.png`, `icon-512.png` - the app icon.
  - `og.png`, `og-image.svg` - social preview (1200x630 PNG, SVG source).
  - `assets/menu.png` - a captured frame of an authored BD-J menu.
  - `site.webmanifest` - install metadata.
- `docs/downloads.json` - the build manifest the page reads at runtime.
- `docs/robots.txt`, `docs/sitemap.xml`, `docs/.nojekyll`.

## Fonts

Fraunces for display, Inter for body, JetBrains Mono for code, all from Google
Fonts with `preconnect`. If the network is unavailable the system serif and
sans-serif fallbacks keep the layout intact.

## Local preview

```sh
cd docs
python3 -m http.server 8000
# open http://localhost:8000
```

The download cards fetch `downloads.json`; serve over HTTP so the fetch is
same-origin.

## Deploy

GitHub Pages serves this repository from `main` and the `/docs` folder:

- Settings -> Pages -> Source: Deploy from a branch.
- Branch `main`, folder `/docs`.
- URL: https://ghostyshell.github.io/keepsake-atelier/

There is no GitHub Actions workflow for the site, by design. A link refresh only
changes `downloads.json`, and the next page load picks it up.

## Updating

- **Download links**: let `scripts/publish-release.sh` write
  `docs/downloads.json`. Do not hand-edit the cards in `index.html`; they are
  rendered from the JSON.
- **Copy**: edit `docs/index.html` directly. Keep the canonical URL, Open Graph
  and JSON-LD in sync if the URL changes.
- **Assets**: replace `icon.png` and regenerate `og.png` if the icon changes.

## Accessibility and performance

- One `h1`, sequential headings, landmarks, a skip link and visible focus.
- `prefers-reduced-motion` removes transitions.
- No raster background images; the page is text and a single icon.
- No JavaScript framework. The only script renders downloads, copies the ETH
  address and updates the footer year.
