# Public framing and outreach checklist for coding agents

Use this checklist whenever a PR touches public-facing copy (`site/`, `README.md`,
`CONTRIBUTING.md`, docs portal templates, release notes, or announcement drafts under `OUTR`).
It aligns public copy with `brand/guidelines/name-and-logo.md` section 7, `agent/claim-phrases.txt`,
and D-036 (open/paid boundary and honest research framing).

## Rules for running agents

1. **Lead with workflows and open standards, not incumbent trademarks.**
   - Describe what Metrology Maven does in terms of the user's job and open standards
     (QIF 3.0 / ISO 23952, STEP AP242, DMIS, I++ DME, ASME Y14.5, ISO GPS).
   - Never lead a hero, app card, README summary or announcement with *"Replaces [Vendor Product]"*.
   - Per `brand/guidelines/name-and-logo.md` section 7, refer to third-party products by name in
     plain text only when needed to state a concrete fact (such as a file format read or an internal
     coverage table row), and never with comparison claims unless backed by a linked benchmark
     (`OUTR.BRAND.LINT` rule CL002).
2. **Use the full brand name.**
   - Write **Metrology Maven** in full, and full app names (**Metrology Maven Report**,
     **Metrology Maven Inspect**, **Metrology Maven Probe**, **Metrology Maven Network**).
     Never shorten the product name to "Maven" (D-039).
   - Technical identifiers (`fid`, `fid-*`, MCP tool names) stay in code formatting.
3. **Avoid unbacked capability and compliance claims.**
   - Do not use phrases in `agent/claim-phrases.txt` (for example `our agent`, `agents can`,
     `program a part`, `autonomously`, `success rate`) in public copy without a matching
     `<!-- fid-claim: <id> -->` entry in `agent/claims.json` (`AGENT.BOARD.CLAIMS`).
   - Do not use compliance/certification words (`certified`, `compliant`, `validated`, `traceable`)
     without both a standard designator + edition year and a benchmark/evidence link
     (`OUTR.BRAND.LINT` rule CL001).
4. **Honest pre-alpha and hiring status.**
   - Never list future roles as active open job vacancies or post them to external job boards
     when hiring is not open within 30-90 days. Frame planned roles transparently as a
     **Future Roles & Talent Network** roadmap with a lightweight contact/notify path.
5. **Preserve the open/commercial boundary (D-036 rule 3).**
   - All four open apps, the deterministic math kernel, QIF/DMIS/I++ support, CLI/MCP surfaces,
     and `fid-verify` stay free forever.
   - Commercial offerings stay strictly in regulated dossiers, support/LTS, hosted verification,
     instrument integration testing, and on-premises enterprise add-ons (such as private
     machine-adapter fine-tuning and fleet drift monitoring).
6. **Keep clean-room discipline.**
   - Never paste standards text (ISO, ASME, VDA) or vendor manuals (`AGENTS.md` rule 5, D-013).
   - Do not promote the project on vendor-owned support forums.

## Follow-up spec-tree PRs (require human P1/P2 review)

Changes to L0/L1 spec nodes are kept separate from website/docs PRs (`AGENTS.md` working rule 6)
and need human review under D-037 P2:

- **`spec/NODE.md` (L0):** Reword the `## Purpose` lead sentence from *"It replaces scan inspection
  (...)"* to positive open-standards workflow framing matching `README.md` and `site/build.py`.
- **`spec/cmm/NODE.md` (L1):** Reword `## Purpose` line 25 (*"Replaces CMM programming suites:"*)
  to *"Provides open-standard CMM programming, simulation and execution:"*.
- **`spec/outr/` announcement drafts (`OUTR.LAUNCH.ANN`, `OUTR.SITE.PAGES`):** When expanding or
  building those leaves, enforce the rules above in generated templates and test fixtures.

## Verification commands for framing PRs

```bash
python site/build.py
# Verify zero third-party resource origins (mirrors .github/workflows/site.yml):
grep -rEo '(src|href)="https?://[^"]+' site/dist --include='*.html' --include='*.css' \
  | grep -vE '="https://(metrologymaven\.io|github\.com/metrologymaven)' \
  | grep -E '(src)=|stylesheet' || echo "OK: zero third-party origins"
python tools/scope.py check
```
