---
id: D-016
title: Acquiring and sharing standards texts for DMT authoring and review
status: open
raised_by: GDT.RES split
blocks: [GDT.RES.ROWASME, GDT.RES.ROWISO]
decide_by: before the first DMT row is authored
updated: 2026-11-16
---

# D-016: Acquiring and sharing standards texts for DMT authoring and review

## Question

Who buys the ASME and ISO standards texts that DMT authors and reviewers need, under what licence, and how does the project record that each signed-off row was written and reviewed against a lawfully held copy of the cited edition?

## Context

D-009 settled who authors and reviews Dialect Mapping Table (DMT) rows (paid experts, then two-expert community sign-off, plus a partner). It did not settle access to the texts. Rows cite designator, edition and clause only and must never contain standards text (CONSTITUTION §5, AGENTS.md rule 5), but writing and checking them requires reading the text. Standards are typically sold as single-user licences that cannot be shared among community reviewers [unverified]. The P1a set needs ASME Y14.5-2018 and Y14.5.1-2019; P1b needs ISO 1101, ISO 5459 (2011 and 2024), ISO 2692, ISO 14405-1 (2016 and 2025) and ISO 14253-1. The concept docs treat the purchase as a steering-group budget item (doc 05 §2.6).

Blocks `GDT.RES.ROWASME` and `GDT.RES.ROWISO`; `UNC.ORS` case authoring likely needs the same answer.

## Options

| Option | For | Against |
|---|---|---|
| A. Project buys one licence per paid author; community reviewers must hold their own copy and attest to it at sign-off | Cheap; clear provenance | Raises the bar for volunteer reviewers |
| B. Project buys licences for every credentialed reviewer | Removes the barrier | Recurring cost grows with the reviewer pool |
| C. Rely on the university or consultancy partner (D-009 option C) holding institutional access | Low cost; credible | Depends on the partner; institutional terms may not cover open-project work [unverified] |

## Recommendation

A, with C as the source of reviewers where a partner exists. Add an "edition held" attestation field to the review record in `GDT.RES.ROWFLOW`.

## Decision

_Open._
