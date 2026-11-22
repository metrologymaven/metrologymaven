# {{FAMILY_NAME}} Design-System Consumption Contract

Spec node: `UX.SYSTEM.LINT` (REQ-UX.SYSTEM.LINT-001 .. -005; satisfies REQ-UX.SYSTEM-005).
Licence: CC-BY-4.0.

Consuming domains (`VIZ`, `RPT`, `OUTR`, `I18N`) reference design tokens, icons and components
by their published names from the design-system package (`UX.SYSTEM.PKG`), never by hard-coded
literal values. This keeps light, dark, high-contrast and shop-floor themes consistent and
ensures every colour pair passes contrast and colour-vision checks in one place.

## 1. The rule

Do not write literal **colours**, **font sizes**, **spacing dimensions** or **motion durations**
in UI or report-style source files. Instead, resolve the named token through `fid-design-pkg`
or your domain's theme loader (for example, `sem.focus.ring`, `type.body`, `space.2`,
`motion.fast`).

`fid-design lint-literals` enforces this rule over a source tree:

```sh
fid-design lint-literals \
  --root <source dir> \
  [--rules <rules.toml>] \
  [--package <design package dir>] \
  [--allowlist <allowlist.txt>] \
  [--json]
```

Exit codes:

- `0` — no unallowed literal findings.
- `1` — one or more literal, allow-marker or rule-schema findings (fail CI).
- `2` — unreadable input, file over 4 MiB, bad package, or invalid CLI usage.

## 2. Default rule sets (`lint-rules@1`)

Rules are data, not hard-coded per language. When `--rules` is omitted, `fid-design` ships
default `lint-rules@1` rules covering **Rust** (`.rs`), **TOML** (`.toml`), **JSON** (`.json`),
**CSS** (`.css`) and **SVG** (`.svg`):

| Rule ID | Kind | Languages | What it catches |
|---|---|---|---|
| `LIT-COLOR-RUST` | `color` | Rust | `from_rgb(...)`, `from_rgba*(...)` colour constructors |
| `LIT-COLOR-HEX` | `color` | Rust, TOML, JSON, CSS, SVG | `#rgb`, `#rrggbb`, `#rrggbbaa` hex colours |
| `LIT-COLOR-FUNC` | `color` | CSS, SVG, TOML, JSON | `rgb(...)`, `rgba(...)`, `hsl(...)`, `hsla(...)` calls |
| `LIT-FONT-SIZE` | `font_size` | CSS, SVG, TOML, JSON | `font-size` / `fontSize` with `px` or `pt` |
| `LIT-FONT-RUST` | `font_size` | Rust | `font_size` literals with `px` or `pt` |
| `LIT-SPACE-STYLE` | `spacing` | CSS, TOML, JSON | `padding*`, `margin*`, `*gap`, `spacing` with `px` or `pt` |
| `LIT-DURATION-STYLE` | `duration` | CSS, TOML, JSON, SVG | `transition*`, `animation*`, `duration` with `ms` or `s` |
| `LIT-DURATION-RUST` | `duration` | Rust | `Duration::from_millis(...)`, `from_secs*(...)` |

### Custom rule files (`lint-rules@1`)

Supply `--rules <file.toml>` to run a custom rule set:

```toml
format = "lint-rules@1"

[[rule]]
id = "LIT-COLOR-HEX"
kind = "color"            # color | font_size | spacing | duration
globs = ["**/*.css"]      # /-separated globs, no character classes or braces
regex = '(?:^|[^0-9A-Za-z_])(#(?:[0-9A-Fa-f]{8}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{3}))(?:[^0-9A-Za-z_]|$)'
value_group = 1           # capture group holding the literal text (0 = whole match)
```

All regular expressions run in a linear-time finite-automaton engine (no backtracking) with
bounded memory, so hostile inputs cannot stall CI. Invalid rule tables report
`LINT-RULES-SCHEMA`; invalid or oversized regular expressions report `LINT-REGEX`.

## 3. Token suggestions (`--package`)

When `--package <dir>` is given, `fid-design lint-literals` loads the package with
`fid-design-pkg`, resolves aliases and deprecations, and normalises both tokens and matched
literals to a common form:

- **Colours** to lowercase `#rrggbb` or `#rrggbbaa` (`#f0a` → `#ff00aa`,
  `from_rgb(0x1f, 0x6f, 0xeb)` → `#1f6feb`, `rgb(...)` / `hsl(...)` → `#rrggbb[aa]`).
- **Dimensions** to `px` (`1 pt = 4/3 px`, e.g. `12pt` → `16px`).
- **Durations** to `ms` (`0.12s` → `120ms`, `from_millis(120)` → `120ms`).

When a reported literal matches one token's resolved value, the finding suggests that token
name; when it matches several tokens, all of them are listed in sorted name order.

## 4. Intentional exceptions (allow markers and allowlist)

Two narrow escape hatches exist for genuinely unavoidable literals (such as vendored widget
bridges or fixed hardware/print constants):

1. **Inline marker:** place `design-allow: <reason>` on the same line as the literal, with a
   non-empty explanation of why a token cannot be used:

   ```css
   color: #1f6feb; /* design-allow: third-party chart widget fallback */
   ```

   A marker with no reason (for example `/* design-allow: */`) never suppresses findings and
   reports **`LIT-ALLOW-NOREASON`**.

2. **Allowlist file (`--allowlist <file>`):** list one `<path-or-glob>: <reason>` per line
   (`#` comments and blank lines are ignored):

   ```text
   # Paths exempt from literal-value linting.
   vendor/legacy.css: third-party widget scheduled for removal
   ```

   An entry without a reason reports **`LIT-ALLOW-NOREASON`**.

## 5. Wiring in consumer CI

Add one step to your consuming domain's CI workflow:

```sh
cargo run -q -p fid-design -- lint-literals \
  --root crates/my-ui/src \
  --package design \
  --json
```

Determinism is D0: directory walks are sorted, findings are sorted and JSON reports are
RFC 8785 canonical JSON, so repeated runs produce byte-identical output.
