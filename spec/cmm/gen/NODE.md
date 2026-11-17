---
id: CMM.GEN
title: Program generation from CAD, PMI, templates and DMIS
level: 2
parent: CMM
kind: subsystem
status: split
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM-005]
covers: [PCD.CAD.03, PW.CMM.02]
depends_on: [AUTO, IO, GEO, GDT, CMM.STRAT, CMM.PATH]
cli: [fid cmm generate-*, fid cmm convert-*, fid cmm *-template*]
mcp: [cmm_generate_*, cmm_convert_*, cmm_*_template*]
journal: [cmm.generate_*@*, cmm.convert_*@*, cmm.*_template*@*]
updated: 2026-11-16
---

# CMM.GEN: Program generation from CAD, PMI, templates and DMIS

## Purpose

Produces **CMM measurement programs as Command Programs** from four sources: selected CAD features, imported PMI (characteristics), parameterised templates, and imported DMIS. It assembles features, strategies, alignments and paths into an ordered measurement sequence, and stops at a draft: every generated program is a draft that only a human releases. It is a separate unit because it is the programmer-facing composition step; it owns sequencing and provenance, while strategies, paths and the program language live elsewhere.

## Scope

**In:**
- CAD-based feature generation: from selected CAD faces and edges, create nominal measurement features with default strategies
- PMI-based generation: from imported characteristics and datums, select the features each characteristic needs and create them
- Measurement sequence generation: order of features, alignment steps (manual pre-alignment, then DCC re-alignment), tip changes and evaluation steps, emitted as a Command Program
- Program templates: parameterised prismatic programs (part families) instantiated from parameters
- DMIS-to-Command-Program conversion: map a parsed DMIS program (from `IO`) to Command Program steps, with a conversion report listing every unsupported or approximated statement
- Provenance of generated programs: source (CAD, PMI, template, DMIS, agent), generator version and inputs, agent-authored flag

**Out** (and which node owns it instead):
- DMIS parsing and DMIS export (owned by `IO`)
- Command Program language, interpreter, editor model and release lifecycle (owned by `AUTO`)
- Hit layouts and scan strategies (owned by `CMM.STRAT`)
- Moves, clearance planes and path optimisation (owned by `CMM.PATH`)
- Alignment solvers (owned by `ALN`)
- PMI import and characteristic creation for evaluation (owned by `IO` and `GDT`)

## Requirements

- **REQ-CMM.GEN-001** When the user selects CAD faces or edges, the system shall create one nominal measurement feature per recognised primitive with a default strategy, and shall report each selection it could not classify.
- **REQ-CMM.GEN-002** When generating from PMI, the system shall create the measurement features required by each selected characteristic and its datums, and shall list characteristics it could not support with a reason.
- **REQ-CMM.GEN-003** The system shall emit every generated measurement sequence as a Command Program in draft state, never as a released program.
- **REQ-CMM.GEN-004** The system shall tag every generated program with its source kind (CAD, PMI, template, DMIS or agent), generator version and input digests, and shall set the agent-authored flag when an agent principal issued or edited any step.
- **REQ-CMM.GEN-005** Every generated sequence shall begin with an alignment block (manual pre-alignment points recorded as observations, then a DCC re-alignment) unless the user selects an existing alignment.
- **REQ-CMM.GEN-006** The system shall instantiate a program template from a typed parameter set, and shall reject parameters outside each parameter's declared range.
- **REQ-CMM.GEN-007** When converting a parsed DMIS 5.3 program, the system shall produce a Command Program plus a conversion report that lists, by DMIS line number, every statement that was dropped, approximated or mapped one-to-one.
- **REQ-CMM.GEN-008** Generation shall be deterministic (class D0): the same inputs and generator version shall produce a byte-identical Command Program.

## Interfaces

- **Offers:** commands such as `cmm.generate_features_from_cad@1`, `cmm.generate_program_from_pmi@1`, `cmm.instantiate_template@1`, `cmm.convert_dmis@1` (all W2, writing a draft; agents write to an agent branch); data types GeneratedProgramProvenance, DmisConversionReport.
- **Consumes:** `IO` parsed DMIS and PMI; `GEO` CAD bodies and faces; `GDT` characteristic model; `AUTO` Command Program format; `CMM.STRAT` default strategies; `CMM.PATH` path planning for the generated sequence.
- **Journal:** `cmm.generate_*`, `cmm.convert_*`, `cmm.*_template*`.

## Decomposition hints

CAD (features from CAD selection), PMI (features from characteristics), SEQ (sequence assembly and alignment blocks), TMPL (templates and part families), DMIS (DMIS conversion and report), PROV (provenance tagging).

## Open questions

- **Owner assigned (level-2 review, accepted 2026-11-16):** CMM.GEN owns exporting a Command Program back to DMIS (seed PCD.PRG.10, export half). IO.DMIS supplies the writer. Include it when this node is expanded.
- Phasing: the concept docs put DMIS import and templated prismatic programs in P2, and auto-path from PMI in P3 (Design Baseline §8). Child nodes for PMI-driven generation may need `phase: P3`; changing a phase needs a decision.
- Which DMIS statements map to Command Program steps is a domain-expert task; the conversion report makes gaps visible rather than guessing.

## Split rationale

The split follows the **sources of a program** only at the edge, and funnels them through one assembler. CAD, PMI, TMPL and DMIS each turn their source into features or steps; SEQ is the single assembler that produces the draft Command Program (alignment block, grouping, evaluation steps), so every source yields the same program shape; PROV is the shared provenance and agent-authored computation that all of them call, kept small because D-008 makes the flag a release gate. DMIS export (assigned to `CMM.GEN` in the level-2 review) sits with DMIS import in `CMM.GEN.DMIS`, because both directions share the mapping table and report model.

Alternatives considered:
- **No separate SEQ** (each source assembles its own program). Rejected: four assemblers would drift apart and the alignment-block rule would be enforced four times.
- **DMIS export as its own child.** Rejected: it shares the mapping table and report with import, and has no parent requirement of its own; it becomes a grandchild instead.

Phase: `CMM.GEN.PMI` is tagged P3 and DMIS export will be `later`; both are proposed for ratification in D-018.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [CMM.GEN.CAD](cad/NODE.md) | Measurement features from CAD selection | 001 008 | PCD.CAD.03 |
| [CMM.GEN.PMI](pmi/NODE.md) | Measurement features and programs from PMI | 002 | |
| [CMM.GEN.SEQ](seq/NODE.md) | Measurement sequence assembly as a draft Command Program | 003 005 008 | PW.CMM.02 |
| [CMM.GEN.TMPL](tmpl/NODE.md) | Parameterised program templates and part families | 006 008 | |
| [CMM.GEN.DMIS](dmis/NODE.md) | DMIS program conversion to and from Command Programs | 007 008 | |
| [CMM.GEN.PROV](prov/NODE.md) | Generated-program provenance and agent-authored flag | 004 | |
