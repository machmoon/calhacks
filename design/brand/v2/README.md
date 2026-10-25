# Wright marks, v2

Five logo directions, each drawn by hand as coordinates. Comparison sheet: `index.html`
(rendered to `png/sheet.png`). The v1 concepts one folder up are untouched.

## What changed from v1, and why

v1 looked generated because of gradients and glows, too many parts (hex fields, nuts,
airfoil plus zigzag in one tile), coordinates like `89.89,84.00` from a hex formula rather
than a grid, and no small-size drawing. v2 rules:

- Master drawing on a 48 x 48 grid with integer coordinates (the one exception is D's
  camber curve, a quadratic whose control point is still on the grid).
- A separate 16 x 16 drawing for favicons, one unit to one pixel, not a scaled-down
  master. Oxide ships separate drawings per size (`icons/svg/cpu-16.svg` and `cpu-24.svg`
  in oxidecomputer/design-system are different shapes, not one shape scaled).
- One or two stroke weights per mark, one ink, optional single accent (bare copper
  `#B5622B`). No gradients, filters, shadows or text effects. Cognition's own logomark
  carries an inner-shadow filter (`cognition-logomark-inner-shadow`); we drop it, since
  that is what reads as rendered rather than drawn.
- Ink `#141414` on `#F4F3EF`; dark mode `#EDEBE6` on `#121212`. Holes are punched in the
  background colour so each file stays one ink.
- Wordmark: IBM Plex Sans SemiBold (OFL, from google/fonts `ofl/ibmplexsans`), outlined to
  paths, tracking -12/1000 em (Oxide's design.md: "tracking: optical ... display
  tightens"). Lockup ratio from Linear's lockup SVG (`viewBox 0 0 400 100`: mark 100 tall,
  caps about 70, text starts at x 146): here mark 48, caps 30, gap 22.

Files per direction: `mark.svg`, `mark-dark.svg` (48 grid, transparent),
`mark-16.svg`, `mark-16-dark.svg` (16 grid), `tile.svg`, `tile-dark.svg` (app-icon
square, mark at 75%), `lockup.svg`, `lockup-dark.svg`, and for A and E
`mark-accent*.svg`. PNGs of each tile are in `png/`.

## A. Trace (`a-trace/`) — recommended

A W routed like a PCB track: vertical runs, 45° mitres at the bottoms, round-ended
segments, both top ends landing on SMD pads that also act as the W's slab serifs.

- Grid: track `M8 12 V30 L16 38 L24 30 V18 V30 L32 38 L40 30 V12`, width 5, round
  caps and joins. Pads 12 x 6 at (2,8) and (34,8), corner radius 1.5.
- 16 px: track width 2, mitred; pads 5 x 2.
- Sources: pad corner radius 1.5 on a 6-high pad is `roundrect_rratio 0.25`, as on the
  pads in `kicad-footprints/Resistor_SMD.pretty/R_0603_1608Metric.kicad_mod`. Round-ended
  tracks with 45° mitres are how KiCad's router draws by default (from use, not from
  reading pcbnew source).
  Accent: copper on the pads only.

## B. Resistor (`b-resistor/`)

The US (ANSI) resistor zigzag already contains a W. Drawn as the zigzag on a wire.

- Grid: `M0 15 H10 L17 34 L24 15 L31 34 L38 15 H48`, width 4, mitre joins.
- Proportions from KiCad's `R_US` symbol in `kicad-symbols/Device.lib`: a full stroke is
  30 wide by 80 tall; here 7 by 19 (the same 8:3 slope within 2%).
- Deviation, stated: KiCad's zigzag is centred on the leads and has six strokes. Here the
  leads sit on the top line and there are four strokes, so it reads as a W. Leads are 10
  units long so it reads as a component on a net and not as a serif letter.

## C. Planform (`c-planform/`)

A flying wing from above whose sawtooth trailing edge spells W, like the B-2's (the
Wikipedia B-2 article describes the "sawtooth trailing edge").

- Grid: one polygon `M24 12 L46 26 L40 32 L34 26 L24 36 L14 26 L8 32 L2 26 Z`.
  Leading edge 14:22 (about 32.5° sweep); every trailing-edge segment at 45°.
- Reads clearly as an aircraft at every size; reads as W only once you are told. Also
  carries a military silhouette, which may be the wrong signal.

## D. Bay (`d-bay/`)

One braced bay of the Wright biplane: two surfaces with the 1903 Flyer's 1-in-20 camber
(Wikipedia, Wright Flyer: "a camber of 1-20"), two uprights, and cross wires.

- Grid: surfaces `M6 10 Q24 6 42 10` and `M6 40 Q24 36 42 40`, width 4 (control point 4
  above the ends gives a sag of 2 on a chord of 36, about 1 in 20). Uprights at x 14 and
  34, wires corner to corner, width 2.
- Simplification, stated: camber is a side-view fact and cross-bracing a front-view one;
  the mark shows both in one view. The aspect (gap about equal to chord) is an
  approximation; chord comes from the article's 510 sq ft over a 40 ft 4 in span, and the
  gap is not given there.
- Weakness: at 16 px the wires blur, and without them it can read as the Gemini sign.

## E. Ball grid (`e-ballgrid/`)

A W read off a BGA footprint: 9 balls on a 5 x 4 grid at pitch 10, ball diameter 8. The
closest of the five to Cognition's construction (a handful of identical cells forming one
figure), with a component footprint in place of hexagons.

- Grid: rows `X...X`, `X.X.X`, `X.X.X`, `.X.X.`; centres at (4 + 10c, 9 + 10r), r 4.
- 16 px: 2 x 2 squares at pitch 3.
- Sources: KiCad `Package_BGA.pretty` footprints are round SMD pads on a fixed pitch, and
  their fab outline chamfers the A1 corner. The accent version colours ball A1 copper for
  the same reason.
- Weakness: reads as dots or Braille before it reads as W at 32 px and below.

## Recommendation

A, Trace. It is the only direction that reads as a W at 16 px and as hardware at 256 px,
and it uses the vocabulary every EE looks at all day (track, mitre, pad) with the
restraint of Cognition's mark: one path and two identical rectangles.

## Sources read (source files, not blog posts)

- Cognition: logomark SVG inlined on cognition.ai (`aria-label="Cognition"`): pointy-top
  hex cells with small corner radii, fused by concave fillets, plus an inner-shadow
  filter.
- Linear: logo SVG inlined on linear.app: mark on a 100-unit box; lockup `viewBox 0 0 400 100`.
- Vercel: logo SVG inlined on vercel.com: triangle `M57.5 0 115 100H0z`. The true
  equilateral height would be 99.6; they round it to 100, an integer-grid choice.
- Oxide: `oxidecomputer/design-system` on GitHub, `icons/svg/cpu-16.svg`, `cpu-24.svg`
  (integer coordinates, 2-unit pin pitch, radius 1, separate drawings per size) and
  `design.md` (tracking, radius scale).
- KiCad: `KiCad/kicad-symbols` `power.lib` (GND, Earth) and `Device.lib` (`R_US`, `R`);
  `KiCad/kicad-footprints` `Package_BGA.pretty/BGA-64_9.0x9.0mm_Layout10x10_P0.8mm.kicad_mod`
  and `Resistor_SMD.pretty/R_0603_1608Metric.kicad_mod`.
- NACA 4-digit equations as implemented in AeroSandbox
  `aerosandbox/geometry/airfoil/airfoil_families.py` (`get_NACA_coordinates`), used for an
  airfoil-section direction that was cut (too thin to hold up as a square mark).
- Wikipedia: Wright Flyer (camber 1-in-20, span, wing area), Northrop B-2 Spirit
  (sawtooth trailing edge).

Not read, so not cited: IBM 8-bar guidelines (the page did not load), Teenage
Engineering, Ableton and the IEC 60617 database itself (KiCad's symbols stand in for it).

## Rebuilding

The SVGs were written by a small generator that outlines the wordmark with fontTools. It
is kept outside the repo, because the repo rules say no new Python; ask if you want it
added under `design/`.
