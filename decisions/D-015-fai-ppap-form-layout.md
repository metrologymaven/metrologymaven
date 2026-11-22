---
id: D-015
title: How closely AS9102 and PPAP form layouts may follow the published forms
status: accepted
raised_by: RPT.FORMS split
blocks: [RPT.FORMS.LAYOUT]
decide_by: before RPT.FORMS.LAYOUT is expanded
updated: 2026-11-22
---

# D-015: How closely AS9102 and PPAP form layouts may follow the published forms

## Question

May the project ship fixed AS9102 form 1–3 and PPAP dimensional-results layouts that place the same fields in the same order and grid as the forms published with SAE AS9102 and the AIAG PPAP manual, or must the layouts be original arrangements that only carry the same data?

## Context

RPT.FORMS must produce first-article and PPAP dimensional deliverables that customers and auditors compare field by field (REQ-RPT-003). The published forms belong to their standards bodies; their text and artwork are copyrighted, and the constitution forbids copying text, layout artwork or trade dress (CONSTITUTION §8, REQ-RPT.FORMS-008). Field *names and meanings* (part number, balloon number, requirement, result) are facts that can be paraphrased; whether a field-for-field grid in the same order is a protectable layout is a legal question [unverified]. Incumbent tools and spreadsheet templates commonly reproduce the forms closely [unverified].

The data model, form 3 row mapping, PPAP rows and finalisation (RPT.FORMS.HDR, ROWS, PPAP, FINAL) do not depend on the answer. Only the fixed visual templates (RPT.FORMS.LAYOUT) do.

## Options

| Option | For | Against |
|---|---|---|
| A. Original layouts: our own field labels in our own words, our own arrangement, citing the standard by designator and clause only; plus a machine-readable field map so customers can check completeness | Lowest copyright risk; clean room | Auditors used to the published grid may push back; customers may still ask for "the real form" |
| B. Field-for-field layout matching the published form's order and grid, with original wording and no copied artwork or logos | Familiar to auditors; matches incumbents | Possible infringement of layout; needs counsel review |
| C. A + user-supplied layout: users who hold a licence to the published forms import their own template into RPT.DESIGN; the project ships only A | No risk for the project; customers who need the exact form can have it | Extra work; the exact form is not available out of the box |
| D. Seek a licence or permission from SAE and AIAG to ship their form layouts | Best user experience | Cost, time, and licence terms may conflict with D-001 |

## Recommendation

**C**, with a counsel check of whether A's field order may follow the standards' clause order [unverified]. It keeps the shipped project clean and lets licensed users reproduce exact forms with the general designer. Revisit D if a standards body offers terms compatible with D-001.

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

**C**, with a counsel check of whether A's field order may follow the standards' clause order [unverified]. It keeps the shipped project clean and lets licensed users reproduce exact forms with the general designer. Revisit D if a standards body offers terms compatible with D-001.

**Condition:** the counsel review named in the recommendation must happen before the affected node is published or released.

**Amended 2026-11-22 by D-052:** Clean-room Option A/C is permanent (original report layouts and open data exports citing clause IDs only; counsel field-order review condition removed).
