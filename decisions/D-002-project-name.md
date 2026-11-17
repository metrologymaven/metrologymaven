---
id: D-002
title: Project and product names
status: accepted
raised_by: founder
blocks: []
decide_by: before first public push
updated: 2026-11-16
---

# D-002: Project and product names

## Question

What is the public name of the project, and of each focused app?

## Context

"Fiducial" is a placeholder with a likely class 9/42 conflict (Fiducial Informatique), and it is probably descriptive (Design Baseline, Codename section). Every identifier in this tree that includes the codename (`fid-*` crates, `fid` CLI, MCP names) is internal and gets one global rename before first publication.

The Craft family's pattern is a family brand plus a function suffix (PhotoCraft, FilmCraft). A metrology family could follow the same pattern, but it must not reuse "Craft".

## Options

| Option | For | Against |
|---|---|---|
| A. A coined family name + function suffixes (e.g. `<Name> Inspect`, `<Name> Probe`, `<Name> Network`) | Easy to protect; scales to many apps | Needs a naming exercise and a knockout search |
| B. Keep "Fiducial" | Already used everywhere internally | Conflict and descriptiveness risk |

## Recommendation

A. Shortlist 5–10 coined names and get a counsel knockout search. Don't publish before then.

## Decision

**Accepted as a process, 2026-11-16, founder.** Shortlist 5-10 coined names, run a counsel knockout search, publish nothing before then. The name itself is still to be chosen; `fid`/Fiducial stays the internal working name, with one global rename at first publication.
