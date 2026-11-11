# Keepsake Atelier interface

The desktop app is a workshop, not a landing page. The interface has to make a
technical job legible: read a disc, choose what plays, decide how it is written,
then watch it happen. Everything below serves that.

## Direction

**Optical atelier.** Near-black ink, a laser spectrum that runs cyan to violet,
one warm brass note for the thing being preserved, hairline glass and
monospaced data. It should read as a precision instrument that happens to be
beautiful, not as a game overlay.

Three rules keep it usable:

1. **One job per page.** The left rail is the table of contents. A page shows
   only what belongs to that stage.
2. **The session is always visible.** The right inspector carries the current
   disc, the primary action, live progress and the queue. You never have to
   hunt for "start" or "cancel".
3. **Motion explains, never decorates.** Entrances, page changes and progress
   are the only places it appears. Anything ambient is slow enough to ignore
   and is removed under `prefers-reduced-motion`.

## Shell

```
topbar       brand · drive and disc state · command palette · updates
rail         8 sections, collapsible to icons below 1200px
workspace    one scrolling page, max 980px, section head + panels
inspector    session card, progress, queue, next-up summary
statusbar    version, drive, keys, tool check, MCP port
```

The window minimum is 960 x 640. At 1180px the inspector narrows to 300px; at
980px it moves below the workspace. The rail becomes an icon strip at 1200px.

## Tokens

All colour lives in `app/src/styles.css` under `:root`. Do not use raw hex in a
component.

| Token | Value | Use |
| --- | --- | --- |
| `--ink-0` | `#05060a` | app background |
| `--surface` | `rgba(17,21,32,.72)` | panels |
| `--line` / `--line-strong` | white 6.5% / 13% | hairlines |
| `--text` / `--text-dim` / `--text-faint` | `#eef2fb` / `#a5b0c7` / `#7b869f` | type ramp |
| `--laser-2` | `#67e8f9` | eyebrow, active icon, focus |
| `--laser` | `#8ba4ff` | primary accent |
| `--violet` | `#b79bff` | gradient end |
| `--brass` | `#f2c078` | the main feature, "keepsake" note |
| `--good` / `--warn` / `--bad` | `#4ae3a5` / `#ffc46b` / `#ff8484` | state |
| `--grad-laser` | cyan to violet | primary buttons, progress, rail marker |

Type: the platform UI font stack (`-apple-system`, `Segoe UI Variable`, system
fallback) at 14px root, and `ui-monospace` for identifiers, paths, percentages
and every number that changes. Body copy never goes below 12px. Section titles
are 12px uppercase with 0.12em tracking; page titles are 24 to 32px with
negative tracking.

Shape: 22px panels, 16px inspector cards, 12px rows, 10px controls. Spacing
follows 4, 8, 12, 16, 20, 26, 40. There is no separate spacing scale variable;
use the rhythm.

## Components

- **Primary button.** Laser gradient, dark text, light sheen on hover, lift on
  press. One per view, at most.
- **Ghost button.** Hairline border on transparent, fills on hover. Everything
  secondary.
- **Panel.** `.card` / `.panel`: translucent surface, hairline, inset highlight.
  Hover only brightens the border.
- **Chip / pill.** Rounded, dot plus label. Colour carries state and the dot is
  the non-colour cue.
- **Row.** `.title` rows are focusable buttons with a left laser marker when
  selected.
- **Progress.** 8px track, gradient fill, sheen only while `.live`. Always
  paired with numeric stats.
- **Empty state.** Dashed container, icon, one sentence of what to do. No dead
  ends.

## Motion

| Moment | Motion | Duration |
| --- | --- | --- |
| Rail entrance | fade and slide from left, 20ms stagger | 500ms |
| Page change | fade and rise 10px | 340ms |
| Panel entrance | fade and rise 14px, 30ms stagger | 420ms |
| Toast | rise and scale from 96% | 380ms |
| Palette | fade, drop 8px, scale from 98% | 300ms |
| Primary hover | sheen sweep | 700ms |
| Progress | width ease plus sheen | 350ms / 1.8s loop |

Easing is `cubic-bezier(.22,.8,.24,1)` for state and
`cubic-bezier(.16,1,.3,1)` for entrances. Animate transform and opacity only.
Never animate width, height, top or left. The one width animation is progress
fill, which is a single element and cheap.

`prefers-reduced-motion: reduce` shortens every animation to 0.001ms and stops
the ambient orbs.

## Accessibility

- Every control has a visible `:focus-visible` ring in `--laser-2`.
- The rail uses `aria-current="page"`; icon-only rail items keep an
  `aria-label`.
- Title rows are `role="button"` with Enter and Space handling and
  `aria-pressed`.
- Covers are focusable with `aria-pressed`.
- State is never colour alone: chips pair a dot with a word.
- `Cmd/Ctrl+K` opens the command palette, `Esc` closes it, `Alt+1..8` jumps
  between sections. The palette has arrow-key and Enter handling.
- Text contrast targets WCAG AA against the panel surface.

## Preview and review

The UI can be laid out and clicked without a disc drive. `app/src/mock.ts`
answers the Tauri commands with realistic fixtures and only loads in dev:

```sh
cd app
npm run dev:mock        # http://localhost:1420/?mock=1
```

Click **Read disc** to load the fixture disc, then move through the rail. Use a
1440 x 900 viewport for the reference layout and 1000 x 760 for the compact one.

Before shipping a UI change:

1. `npm run build` (type check plus production bundle).
2. Screenshot the changed section at both widths.
3. Check keyboard order, focus rings and the reduced-motion pass.
4. Keep copy short and specific. No dashes used as punctuation, no promises the
   code does not keep.

## Skills

The design work follows the repo skill set installed under `~/.claude/skills`
(OpenCode symlinks under `~/.config/opencode/skills`). The routing that matters
for this app:

| Task | Skill |
| --- | --- |
| Direction, palette, type, product guidance | `ui-ux-pro-max` |
| Distinctive interface, anti-slop | `frontend-design` |
| Motion timing and choreography | `motion-design`, `emil-design-eng` |
| Component polish | `make-interfaces-feel-better`, `better-ui` |
| Accessibility audit | `accesslint-audit`, `better-accessibility` |
| Whole-interface review | `better-interface`, `ui-review` |
| Copy | `better-writing`, `humanizer` |

See `docs/design-skills.md` for the install and refresh commands.
