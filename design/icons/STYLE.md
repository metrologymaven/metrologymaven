# {{FAMILY_NAME}} Icon Style Guide, Grid and SVG Profile

Spec node: `UX.ICONS.GRID` (REQ-UX.ICONS.GRID-001 .. -004). Licence: CC-BY-4.0.

This guide defines the drawing grid, keylines, stroke weights, corner radii, optical padding,
colour rules and restricted SVG profile for every original icon in the {{FAMILY_NAME}} design
system. `fid-design icons-lint` enforces the machine-checkable rules in CI.

## 1. Two masters: 16 px and 24 px

Every icon ships as two separate master files under `icons/svg/`:

- `icons/svg/<name>.16.svg` — `viewBox="0 0 16 16"`, used in dense feature trees, tables,
  status badges, tabs and compact toolbars.
- `icons/svg/<name>.24.svg` — `viewBox="0 0 24 24"`, used in primary toolbars, empty states,
  wizards, dialogs and navigation rails.

### Redrawn for 16 px, never scaled down

A 16 px master **must be redrawn for the 16 px grid**, never mechanically scaled by `2/3` from
the 24 px master (REQ-UX.ICONS.GRID-002). Scaling a 24 px drawing down puts strokes at
fractional coordinates, narrows gaps below 1 px and causes blur at 100 % display scale across
light, dark, high-contrast and shop-floor themes.

When redrawing for 16 px:

- Keep the silhouette recognisable as the same metaphor, but drop secondary notches, ticks or
  inner details whose gap would fall below 1 px.
- Snap every straight horizontal and vertical stroke to half-pixel centrelines (`.5`) so a
  `1 px` stroke fills whole device pixels cleanly at 100 % scale.
- Maintain at least `1.5 px` clear space between parallel strokes wherever possible.

## 2. Grid, stroke width, optical padding and keylines

All anchor points (`M`, `L`, `H`, `V`, `C`, `S`, `Q`, `T`, `A` endpoints; `rect`, `circle`,
`line`, `polyline`, `polygon` geometry) must sit on a **0.5 px grid** (within `1e-6` px).

| Property | 16 px master | 24 px master |
|---|---|---|
| Canvas (`viewBox`) | `0 0 16 16` | `0 0 24 24` |
| Coordinate grid | `0.5 px` | `0.5 px` |
| Optical padding (trim) | `1 px` each side | `2 px` each side |
| Live area | `14 × 14 px` (`1..15`) | `20 × 20 px` (`2..22`) |
| Stroke width | `1 px` (`stroke-width="1"`) | `1.5 px` (`stroke-width="1.5"`) |
| Stroke caps & joins | `round` / `round` (`miter` for datum/GD&T ticks) | `round` / `round` (`miter` for datum/GD&T ticks) |
| Outer corner radius | `1 px` (`rx="1"`) | `1.5 px` (`rx="1.5"`) |
| Inner corner radius | `0.5 px` (`rx="0.5"`) or `0` | `0.5 px` (`rx="0.5"`) or `1 px` |

### Optical padding and overshoot

Keep square and upright shapes inside the **live area** (`1..15` on 16 px; `2..22` on 24 px).
Circles and pointed diagonals may overshoot into the trim zone by at most `0.5 px` so round
and pointed icons match the perceived weight of rectangular ones. Never touch the canvas edge
(`0` or `16` / `24`) with stroke outer edges.

### Keyline shapes

Use these four keyline bounding boxes so different icons have equal optical mass:

| Keyline shape | 16 px master (centre 8, 8) | 24 px master (centre 12, 12) |
|---|---|---|
| Square | `2.5, 2.5` to `13.5, 13.5` (`11 × 11 px`) | `4, 4` to `20, 20` (`16 × 16 px`) |
| Circle | `cx="8" cy="8" r="6"` (`12 × 12 px`) | `cx="12" cy="12" r="9"` (`18 × 18 px`) |
| Horizontal rectangle | `1.5, 3.5` to `14.5, 12.5` (`13 × 9 px`) | `2.5, 5` to `21.5, 19` (`19 × 14 px`) |
| Vertical rectangle | `3.5, 1.5` to `12.5, 14.5` (`9 × 13 px`) | `5, 2.5` to `19, 21.5` (`14 × 19 px`) |

#### Visual keyline diagram (16 px and 24 px)

```text
16 px master (0..16, trim 1 px, live 1..15)      24 px master (0..24, trim 2 px, live 2..22)
+----------------+                               +------------------------+
| .------------. | <- live area 14x14            |                        |
| |  .------.  | | <- vertical rect 9x13         |  .------------------.  | <- live area 20x20
| | .--------. | | <- square 11x11               |  |   .----------.   |  | <- vertical rect 14x19
| | | (circ) | | | <- circle d=12, horiz 13x9    |  |  .------------.  |  | <- square 16x16
| | '--------' | |                               |  |  |  (circle)  |  |  | <- circle d=18, horiz 19x14
| |  '------'  | |                               |  |  '------------'  |  |
| '------------' |                               |  |   '----------'   |  |
+----------------+                               |  '------------------'  |
                                                 |                        |
                                                 +------------------------+
```

## 3. Colour and token roles

Icons carry **no literal colours** (REQ-UX.ICONS.GRID-003). The native UI, web build, reports
and docs supply theme colours at render time.

- **Single-tone icons (default):** set `stroke="currentColor"` and `fill="none"` (or
  `fill="currentColor"` and `stroke="none"` for solid fills). The host sets CSS/theme
  `color` from semantic tokens (`sem.icon.primary`, `sem.icon.muted`, etc.).
- **Multi-role or status icons:** attach `data-role="<token.name>"` on an element or `<g>`
  group naming a valid token role (matching `^[a-z][a-z0-9]*(\.[a-z0-9-]+)*$`, such as
  `data-role="sem.status.warn"` or `data-role="sem.icon.accent"`), while still using
  `currentColor` or `none` in `stroke`/`fill` so fallback rendering stays monochrome.
- **Forbidden colour values:** hex (`#ff0000`, `#000`), `rgb()`, `rgba()`, `hsl()`,
  `oklch()`, named CSS colours (`red`, `black`, `white`), `var(...)` and `url(...)` fail
  `fid-design icons-lint` with `ICON-COLOR-LITERAL` or `ICON-EXTERNAL-REF`.
- **Never colour alone (`P-NOCOLOR`):** any icon conveying conformance or side-effect class
  must remain distinguishable by silhouette alone in single-tone greyscale.

## 4. Restricted SVG profile

Every icon SVG must conform to this minimal, deterministic profile (enforced by
`fid-design icons-lint`):

- **File size:** at most **256 KiB**, valid UTF-8, well-formed XML with **no DTD**
  (`<!DOCTYPE`), **no entity declarations** (`<!ENTITY`) and no custom entity references.
- **Allowed elements only:** `svg`, `g`, `path`, `circle`, `rect`, `line`, `polyline`,
  `polygon`.
- **Forbidden elements:** `text`, `tspan`, `textPath`, `image`, `script`, `style`, `defs`,
  `use`, `filter`, `linearGradient`, `radialGradient`, `pattern`, `mask`, `clipPath`,
  `foreignObject`, `a`, animations and any non-SVG namespace element.
- **Forbidden attributes:** `style`, `class`, `transform`, `href`, `xlink:href`, `src`,
  `xmlns:xlink`, `filter`, `mask`, `clip-path`, `marker-*`, and all `on*` event handlers.
- **No external references:** no `url(...)`, `http://`, `https://`, `data:`, `file://`,
  `javascript:` or `<?xml-stylesheet ...?>` anywhere in the SVG.
- **No transforms:** bake geometry directly into final 0.5 px grid coordinates so every
  renderer produces identical geometry without matrix evaluation.

## 5. Original example drawings

The two masters below show an original **measurement target** icon (concentric reticle with
four cardinal ticks). Notice how the 16 px master is redrawn rather than scaled down: the
outer ticks shorten cleanly to 2 px on half-pixel centres and the inner dot stays a crisp
1 px radius circle on the 0.5 px grid.

### Example 1: 24 px master (`measure.target.24.svg`, monochrome `currentColor`)

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="7.5"/>
  <circle cx="12" cy="12" r="2"/>
  <line x1="12" y1="2.5" x2="12" y2="6.5"/>
  <line x1="12" y1="17.5" x2="12" y2="21.5"/>
  <line x1="2.5" y1="12" x2="6.5" y2="12"/>
  <line x1="17.5" y1="12" x2="21.5" y2="12"/>
</svg>
```

### Example 2: 16 px master (`measure.target.16.svg`, redrawn for 16 px with `data-role`)

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="8" cy="8" r="5" data-role="sem.icon.primary"/>
  <circle cx="8" cy="8" r="1" fill="currentColor" stroke="none" data-role="sem.icon.accent"/>
  <path d="M8 1.5V4M8 12V14.5M1.5 8H4M12 8H14.5" data-role="sem.icon.primary"/>
</svg>
```

## 6. Checking icons locally

Run the structural linter from the repository root:

```sh
cargo run -q -p fid-design -- icons-lint --root design/icons --json
```

Error codes reported by `fid-design icons-lint`:

- `ICON-ELEMENT` — forbidden element (e.g. `text`, `image`, `script`, `style`, `filter`).
- `ICON-ATTR` — disallowed or malformed attribute, invalid `stroke-width` or malformed path.
- `ICON-COLOR-LITERAL` — literal colour in `fill` or `stroke` instead of `currentColor`/`none`.
- `ICON-OFF-GRID` — coordinate not on the 0.5 px grid (within `1e-6`).
- `ICON-VIEWBOX` — missing or invalid `viewBox` (or mismatched `width`/`height`).
- `ICON-MASTER-MISSING` — missing `16` or `24` px master for an icon in the directory.
- `ICON-EXTERNAL-REF` — external reference (`href`, `xlink:href`, `url(...)`, `<?xml-stylesheet?>`).
