# 2D PDF Drawing Ballooning & AI-Assisted FAI Extraction (`RPT.BALLOON.AUTO`)

## Why This Wedge Ships First

Most machine shops and Tier-2/Tier-3 aerospace, defense, and medical suppliers still receive **2D PDF engineering drawings** rather than MBD STEP AP242 with semantic PMI. Before a shop ever connects a CMM or trusts new scan-to-CAD software, it spends hours manually ballooning PDF drawings and re-typing tolerances into AS9102 Form 3 / PPAP spreadsheets.

`RPT.BALLOON.AUTO` turns Metrology Maven into an **immediate Day-1 shop-floor utility**:

```text
  Drop 2D PDF / TIFF drawing
        │
        ▼
  1. RPT.BALLOON.AUTO.TEXT   Bounded PDF vector text runs + sheet bboxes (0.01 mm) + display tiles
        │                    (zero JS/action execution; all strings typed UntrustedText, C6)
        ▼
  2. RPT.BALLOON.AUTO.PARSE  Deterministic parser: linear/dia/radius/angle dims, ± / bilateral /
        │                    limit tolerances, GD&T FCFs (⌖ ⏥ ⟂ ∥ ⌭ ⌓ ↗ ⌰), datums, notes
        │                    (exact authored decimal strings, never binary-rounded)
        ▼
  3. RPT.BALLOON.AUTO.PROPOSE  `report.propose_balloons@1` (W1 draft/agent branch)
        │                    Merges vector-parsed candidates + optional local/MCP vision-OCR
        │                    for scanned drawings -> batched Proposal Inbox queue (H2) with
        │                    cropped PDF snippet + bbox provenance + confidence word
        ▼
  4. Human Batch Review (H2) Confirm dialect (ASME/ISO), edit/accept/reject in one batched act
        │                    -> commits `report.add_char_balloon@1` (W2) atomically
        ▼
  5. RPT.BALLOON.AUTO.EXPORT  `report.export_balloon_pack@1` (W1 local export root)
                             One-command vector-overlaid Ballooned PDF + AS9102 Form 3 CSV/XLSX
```

## Key Architectural Guarantees

- **Zero Legal / Vendor Bottlenecks**: Uses only bounded pure-Rust / MIT-Apache PDF structure parsing, deterministic regex/grammar callout extraction, and optional user-configured local or MCP vision models for scanned raster sheets. No proprietary SDKs, no standards text stored or sent to external services (C5, §5), and clean-room original balloon styling (`UX.REPORT.BALLOON`).
- **Every Number Traceable to the Drawing**: Every proposed characteristic stores its exact sheet ID, integer 0.01 mm bounding box (`SheetRect`), raw untrusted source text, and authored decimal strings (`12.000 mm`, `+0.018/-0.000 mm`) without float round-tripping.
- **Agent-First, Human-Adopted (C5 / D-037 H2)**: `report.propose_balloons@1` writes only to an agent/draft branch (`W1`), never mutating the working document directly. A human reviews cropped snippets in the Proposal Inbox and adopts the batch in one action.
- **No Unnecessary Decision Gates**: All four leaves (`TEXT`, `PARSE`, `PROPOSE`, `EXPORT`) are `status: leaf`, `phase: P1a`, and buildable immediately on top of `IO.REG.PLUGIN.DRAW`, `RPT.BALLOON.SHEET`, `RPT.BALLOON.REG`, `RPT.BALLOON.CHAR`, and `RPT.FORMS.ROWS`.
