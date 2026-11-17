---
id: D-013
title: Copy detection against incumbents' public text
status: accepted
raised_by: OUTR.BRAND split
blocks: [OUTR.BRAND.COPY]
decide_by: before the first public website release
updated: 2026-11-17
---

# D-013: Copy detection against incumbents' public text

## Question

Should the outreach lint detect passages copied from incumbents' public material (REQ-OUTR.BRAND-005), and if so, may the project keep a reference set built from that material?

## Context

REQ-OUTR.BRAND-005 asks the claim lint to flag 15 or more consecutive words matching incumbents' public material. That needs a reference set of incumbent text (brochures, web pages, manuals). Storing or distributing that text could itself raise copyright questions, and CONSTITUTION §8 (clean room) forbids copying incumbents' strings into the project. OUTR.BRAND.COPY cannot be specified without a choice. The rest of the lint (OUTR.BRAND.LINT) does not depend on it.

## Options

| Option | For | Against |
|---|---|---|
| A. Keep a private reference set of incumbent text, outside the public repository, run by maintainers only | Strongest detection | Holding copies of third-party text; not reproducible by contributors |
| B. Keep only hashed word shingles (for example 15-word windows, SHA-256), built once by a maintainer and published without the text | Contributors can run it; no text is redistributed | Building it still needs a temporary copy; legal status of published hashes [unverified]; misses paraphrase |
| C. Drop REQ-OUTR.BRAND-005; rely on the clean-room rule and the human review in OUTR.BRAND.REVIEW (add a "copied text" checklist item) | No third-party text handled at all; simplest | Detection depends on reviewers noticing |

## Recommendation

C now, revisit B after counsel review if copied text is ever found in project material. The human review already has a "copied text" checklist item (OUTR.BRAND.REVIEW). Evidence on the legal status of hashed shingles is [unverified].

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

C now, revisit B after counsel review if copied text is ever found in project material. The human review already has a "copied text" checklist item (OUTR.BRAND.REVIEW). Evidence on the legal status of hashed shingles is [unverified].

**Condition:** the counsel review named in the recommendation must happen before the affected node is published or released.
