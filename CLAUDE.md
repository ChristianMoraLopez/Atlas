# Atlas — notes for AI assistants

Atlas is a Tauri 2 desktop app (Rust in `src-tauri/`, React + TypeScript in `src/`)
that turns a person's calendar, mail and Teams evidence into rows of the Circana
interactions tracker. Commits are authored by Christian Rey Mora López only:
never add AI co-author or session trailers.

## Art direction: the survey sheet

Atlas is a cartographer's field book, not a SaaS dashboard. Every screen should
read like a printed survey sheet that the person's day is drawn on.

- **Metaphor.** A day is a *route*; each interaction is a numbered *waypoint*;
  the preview table is the *logbook*; the daily minimum is *three summits*; the
  header is the map's *title block*; an empty preview is a *blank sheet*.
  Name new UI after this vocabulary before inventing generic names.
- **Paper and ink.** Warm map paper (`--atlas-paper`), raised sheets
  (`--atlas-surface`), near-black pine ink (`--atlas-ink`), terracotta for the
  route and attention (`--atlas-signal`), brass for fine rules. The person's
  accent (`data-accent` on `<html>`) recolours everything; never hard-code a hex
  value in components, use the tokens in `src/design/tokens.css`.
- **Line work over boxes.** Prefer 1px ink rules, dashed lanes, neatlines,
  graticule ticks and contour lines to big rounded cards, gradients or glass.
  Corners stay small (`--atlas-radius-sheet`). Shadows are soft and low.
- **Symbols, not only colour.** Sources have shapes as well as colours
  (calendar ● circle, mail ◆ diamond, Teams ▲ triangle, manual ✚ cross) via
  `SourceMark` in `src/design/route.tsx`. Keep both cues everywhere.
- **Type.** Fraunces (display, italic for the person's name), Instrument Sans
  (interface), JetBrains Mono (times, sheet numbers, coordinates, eyebrows in
  small caps with wide tracking). Do not add other families.
- **Personal.** The contour map is seeded from the person's name (`TopoMap`,
  `seedFrom`): same person, same terrain. Greet them by first name.
- **Avoid.** Default Tailwind/shadcn looks, purple gradients, glassmorphism,
  emoji, stock illustrations, generic "metric cards", centred hero + three
  feature cards, and icons standing alone without a tooltip.

## Building UI

- New components: `src/design/*.tsx` with a sibling `*.module.scss` that
  `@use "mixins" as *;` (helpers `c()`, `survey-label`, `source-tones`,
  `reduced-motion`). Older screens still use Tailwind utilities mapped to the
  same tokens; migrate them when you touch them, do not add new Tailwind-only
  components.
- Functional primitives come from Radix (`@radix-ui/react-switch`, `-tooltip`,
  `-popover`): use them for anything with keyboard or focus behaviour.
- Motion: `motion/react` for UI transitions and small counters; GSAP for
  sequenced drawing (route trail, greeting letters). Animate one-off entrances
  with CSS keyframes using the individual `scale`/`translate` properties so they
  never fight hover transforms. Every animation must respect
  `prefers-reduced-motion`.
- Icons: Lucide only, always with an accessible label or tooltip.
- Text goes through `t()` with a Spanish entry in `src/i18n.tsx`; check the key
  does not exist already (duplicate keys fail the build).

## Checking your work

- `npm run build` (TypeScript + Vite) and `cargo test` in `src-tauri/`
  (`runtime_is_discovered_beside_atlas` only passes on Windows).
- Visual check without Tauri: `npm run dev`, then `npm run shots` renders the
  role, workspace and empty screens with mocked data into `screenshots/` using
  Playwright (`scripts/shots.cjs`, `scripts/tauri-mock.js`). Look at them before
  and after any visual change.
- Do not run `cargo fmt` on the whole crate; it rewrites unrelated files.
