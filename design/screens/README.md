# {{FAMILY_NAME}} Screen Inventory, ID Convention and Wireframe Citations

Spec node: `UX.SCREENS.INV` (REQ-UX.SCREENS.INV-001 .. -006; satisfies REQ-UX.SCREENS-001 .. -003, -008, -009).
Licence: CC-BY-4.0.

This directory holds the register of every primary screen across {{FAMILY_NAME}} (`inventory.json`),
the wireframe and mockup approval records (`approvals/`), and the per-app screen drawings
(`screens/<app>/<name>/`).

## 1. Identifier convention

- **Screen ID:** `SCR.<APP>.<NAME>`
  - `<APP>` is one of:
    - `SHELL` (`app = "shell"`) — shared application shell
    - `SETUP` (`app = "setup"`) — Shop Setup guided onboarding
    - `INSP` (`app = "inspect"`) — Inspect app
    - `PROBE` (`app = "probe"`) — Probe app
    - `NET` (`app = "network"`) — Network app
    - `RPT` (`app = "report"`) — Report app
    - `KIOSK` (`app = "kiosk"`) — Shop-floor operator kiosk
    - `STN` (`app = "station"`) — Approval Station
  - `<NAME>` is 2 to 12 uppercase ASCII letters or digits starting with an uppercase letter
    (for example `SCR.RPT.FAIFORM`, `SCR.SHELL.START`, `SCR.KIOSK.LOGIN`).
  - FLOW journey maps (`acceptance/journeys/persona/per-*.json`) reference these IDs in their
    step `screen` field.
- **Wireframe ID:** `WF.<APP>.<NAME>@<rev>` where `<rev>` is a positive integer starting at 1
  (for example `WF.RPT.FAIFORM@2`).
- **Mockup ID:** `MK.<APP>.<NAME>@<rev>` where `<rev>` is a positive integer starting at 1
  (for example `MK.RPT.FAIFORM@1`).
- **Drawing files:** `screens/<app>/<name>/wf-<rev>.{svg,pdf,png}` for wireframes and
  `screens/<app>/<name>/mk-<rev>.<theme>.{png,pdf}` for themed mockups.

## 2. How implementation leaves cite wireframes and mockups

Every implementation leaf that builds a primary screen must cite the approved wireframe ID
(and mockup ID before GA) that it builds against (REQ-UX-003):

1. **In `## Design` (current convention):** add a line starting with `Wireframes:` naming one
   or more `WF.<APP>.<NAME>@<rev>` or `MK.<APP>.<NAME>@<rev>` IDs:

   ```markdown
   - **Wireframes:** WF.RPT.FAIFORM@2
   ```

2. **In YAML frontmatter (once tooling adds the field):** list the same IDs in a single-line
   frontmatter list (no commas inside items):

   ```yaml
   wireframes: [WF.RPT.FAIFORM@2]
   ```

`fid-design screens-gate` reads both forms. Any leaf that cites an unapproved `WF` or `MK` ID
fails with `SCREEN-GATE-UNAPPROVED`; any claimed leaf (`claimed_by` set) whose `## Design`
names a `SCR.<APP>.<NAME>` ID without citing a `WF.<APP>.<NAME>@<rev>` fails with
`SCREEN-GATE-UNCITED`.

## 3. Approval records (`screens/approvals/*.json`)

When a human reviewer approves a wireframe or mockup, they commit a `screen-approval@1` record
under `design/screens/approvals/` storing:

- `format`: `"screen-approval@1"`
- `pr`: positive PR number
- `screen`: `SCR.<APP>.<NAME>`
- `artefact`: `WF.<APP>.<NAME>@<rev>` or `MK.<APP>.<NAME>@<rev>`
- `journey_digests`: map from each traced journey `map_id` to its current RFC 8785 canonical
  JSON SHA-256 digest (`sha256:<64 lowercase hex>`)
- `approver`: `{ "name": "<GitHub login>", "principal": "human" }`
- `date`: `YYYY-MM-DD`

If a traced journey map later changes, its RFC 8785 SHA-256 digest no longer matches
`journey_digests`, and `fid-design screens-check` flags `SCREEN-APPROVAL-STALE` until a fresh
human review approves the updated screen.

## 4. Running the checks

```sh
# Check inventory, journey traces, approvals and staleness
fid-design screens-check --root design --journeys acceptance/journeys/persona --json

# Check GA mockup completeness for one app
fid-design screens-check --root design --journeys acceptance/journeys/persona --ga report --json

# Check spec-tree wireframe/mockup citations
fid-design screens-gate --spec spec --root design --json
```
