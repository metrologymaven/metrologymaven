---
id: CMM.PROBE.QUAL
title: Probe qualification reduction, records and templates
level: 3
parent: CMM.PROBE
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.PROBE-006, REQ-CMM.PROBE-007, REQ-CMM.PROBE-010]
covers: [PCD.PRB.03]
depends_on: [KRN, AUTO, CMM.PROBE.MODEL, CMM.PROBE.HEAD, CMM.PROBE.VALID]
cli: [fid cmm qualify-probe, fid cmm *-probe-reference-sphere, fid cmm generate-probe-qualification]
mcp: [cmm_qualify_probe, cmm_*_probe_reference_sphere, cmm_generate_probe_qualification]
journal: [cmm.qualify_probe@1, cmm.*_probe_reference_sphere@*, cmm.generate_probe_qualification@*]
updated: 2026-11-16
---

# CMM.PROBE.QUAL: Probe qualification reduction, records and templates

## Purpose

Turns hits measured on a reference sphere into qualified tips. It owns the reference sphere definition, the reduction of supplied hits to each tip's offset vector, effective radius and form residual through the kernel sphere fit, the pass or fail decision against the user's residual limit, the qualification record, and the qualification program templates that produce a Command Program (so the motion itself only ever runs through the Motion Executive). It is a separate module because it is the only place where measured data changes a probe's state of record.

## Scope

**In:**
- Reference sphere entity: nominal diameter, calibrated form value, pose (mount position and stem direction), certificate reference
- Reduction of supplied hits per (tip, head angle) to offset, effective radius and form residual, via the `KRN` least-squares sphere fit
- Reference-tip and sphere-location step (locating the sphere centre with a reference tip before qualifying the others)
- Residual-limit check and the qualification record (one per tip and angle)
- Qualification program templates per head kind (fixed, indexing, continuous) emitting a draft Command Program of sphere hits for each tip and angle
- Issuing a validity period on pass, using the policy and function owned by `CMM.PROBE.VALID`

**Out** (and which node owns it instead):
- Sphere fit algorithm (owned by `KRN.LSQ`)
- Running the qualification program on a machine (owned by `CMM.EXEC`)
- Validity evaluation and status queries (owned by `CMM.PROBE.VALID`)
- Probing uncertainty from qualification residuals (owned by `UNC`)
- Command Program format and release (owned by `AUTO`)

## Requirements

- **REQ-CMM.PROBE.QUAL-001** The system shall model a reference sphere with nominal diameter, calibrated form value, pose and certificate reference, and shall reject a non-positive or non-finite diameter.
- **REQ-CMM.PROBE.QUAL-002** When hits for one tip at one head angle are supplied, the system shall reject the set with `QUAL_TOO_FEW_HITS` if it has fewer than five hits or if the hits lie within one plane to 1e-9 m.
- **REQ-CMM.PROBE.QUAL-003** When a valid hit set is supplied, the system shall fit a least-squares sphere through the kernel and compute effective tip radius as fitted radius minus reference sphere radius, tip offset as fitted centre minus located sphere centre, and form residual as the peak-to-valley of radial residuals.
- **REQ-CMM.PROBE.QUAL-004** If the form residual exceeds the user-configured limit for that configuration, then the system shall record the tip as `UNQUALIFIED` with the residual and shall issue no validity period.
- **REQ-CMM.PROBE.QUAL-005** When a tip passes, the system shall journal one qualification record holding probe configuration digest, tip ID, head angle, reference sphere ID, hit-set digest, offset, effective radius, residual, part-independent temperature reading if supplied, time, kernel algorithm ID and the validity period.
- **REQ-CMM.PROBE.QUAL-006** The system shall provide a qualification template for each head kind that generates a draft Command Program visiting every requested (tip, angle) pair with a configurable hit pattern, and the template shall produce no motion itself.
- **REQ-CMM.PROBE.QUAL-007** Reduction shall be deterministic (class D0 or D1, matching the kernel sphere fit class) so that replaying a qualification reproduces the record.

## Interfaces

- **Offers:** `cmm.define_probe_reference_sphere@1` (W2), `cmm.qualify_probe@1` (W2, reduction only, no motion), `cmm.generate_probe_qualification@1` (W2, writes a draft Command Program; agents write to an agent branch); data types ReferenceSphere, QualificationRecord, QualificationTemplate.
- **Consumes:** `KRN` LS sphere fit; `CMM.PROBE.MODEL` nominal offsets; `CMM.PROBE.HEAD` angle sets; `CMM.PROBE.VALID` validity policy and function; `AUTO` Command Program format.
- **Journal:** `cmm.qualify_probe@1`, `cmm.define_probe_reference_sphere@1`, `cmm.generate_probe_qualification@1`.

## Decomposition hints

SPHERE (reference sphere entity), REDUCE (hit reduction and record), LOCATE (reference tip and sphere location), TMPL (qualification program templates per head kind).

## Open questions

- Whether qualification on the controller side (server-owned compensation, as the run ticket's `compensation_owner: server` suggests) or client side is the default per controller type [unverified]. Server-owned compensation may mean this module only records controller-reported results.
- Default qualification form-residual limit needs a domain expert; no default is set here.
- Peak-to-valley versus RMS as the residual measure is a domain choice; peak-to-valley is used here as the stricter reading [unverified].

## Children

_None yet: status `open`._
