---
id: D-039
title: Metrology Maven name, public monorepo and website
status: accepted
raised_by: founder
blocks: []
decide_by: before first public push
updated: 2026-11-18
---

# D-039: Metrology Maven name, public monorepo and website

## Question

D-002 left the public name open. What is the name, where does the project live publicly, and what happens to the `fid` identifiers?

## Context

- The founder's company and the existing domain `metrologymaven.io` (in use since 2026-08 for a consultancy site) already carry the name **Metrology Maven**.
- D-002 planned one global rename of every `fid`/Fiducial identifier at first publication. "Fiducial" was rejected as a *brand* (a likely class 9/42 conflict). A fiducial is also a generic metrology term (a reference mark), so command and crate names using it are not brand use.
- "Maven" alone is the name of a well-known software build tool (Apache Maven). The full two-word name must always be used for the product.

## Decision

**Accepted 2026-11-17, founder.**

1. **Name.** The project, the product family and the steward company are **Metrology Maven**. The apps are *Metrology Maven Inspect*, *Metrology Maven Report*, *Metrology Maven Probe* and *Metrology Maven Network* (D-004). The brand is never shortened to "Maven". A counsel knockout search and trademark filing are still recommended and tracked with the company formation, not in this tree.
2. **Identifiers stay.** The CLI stays `fid`, crates stay `fid-*` and MCP tool names are unchanged. This replaces the D-002 global-rename step and the placeholder-name gate in ECO.REL.PKG.PLAN only for the `fid` prefix. User-visible product strings use the brand.
3. **One public monorepo** at `github.com/metrologymaven/metrologymaven` holds the spec tree (`spec/`, `seeds/`, `decisions/`, `tools/`), the code (`crates/`, Cargo workspace) and the website (`site/`). The earlier private repository `metrologymaven-site` is renamed `metrologymaven-site-legacy` and archived, not deleted.
4. **Website.** `metrologymaven.io` becomes the project website (OUTR.SITE), replacing the consultancy site. It is built statically by `site/build.py` from the spec tree and served by the existing Cloudflare Worker `metrologymaven-site` as static assets, so the domain routes stay attached. Old consultancy URLs redirect to the home page.
5. **Disclosure hold (D-036 rule 5).** The founder decides when the repository and website go public. Going public is a public disclosure of everything in this tree. That ends the option of patents in most countries outside the US and starts the one-year US grace period. Publishing therefore also acts as a defensive publication (B4, D-001).
6. **Launched 2026-11-18 at the founder's direction:** the repository went public and metrologymaven.io now serves the project site. The disclosure hold is lifted from that date.

## Consequences

- README, site and product strings say "Metrology Maven"; the internal word "Fiducial" is retired from prose.
- OUTR.BRAND inherits the "never just 'Maven'" rule for its vocabulary lint.
