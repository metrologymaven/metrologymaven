---
id: VIZ.CAPTURE
title: Screenshots, view track and video
level: 2
parent: VIZ
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-VIZ-004]
covers: [GOM.VIZ.07, GOM.VIZ.08]
depends_on: [CORE, VIZ.RENDER, VIZ.VIEW]
cli: [fid capture *]
mcp: [capture_*]
journal: [capture.*]
updated: 2026-11-16
---

# VIZ.CAPTURE: Screenshots, view track and video

## Purpose

The presentation recording layer (CONSTITUTION §6, layer 2): screenshots as commands, the roughly 10 Hz view track of camera, visibility and highlight keyframes, viewport video capture in hash-chained segments on the journal clock, and scripted camera animations (turntables, view-to-view flights) used for recordings and stills. CORE explicitly delegates the presentation track and video to VIZ. It is separate because it owns its own storage format, timing contract and recording governance.

## Scope

**In:**
- Screenshot command (viewport or window, resolution, background, with or without UI)
- View track: keyframe sampling of ViewState at about 10 Hz on the journal clock
- Viewport video capture by frame readback and encoding, in content-addressed segments attached by journal entries
- Full-window capture through OS APIs (later phase)
- Turntable and camera-path animations, rendered live or headlessly
- Re-rendered replays from the view track
- Recording governance: always-visible recording indicator, org policy to mandate or forbid, panel redaction, local-only storage by default
- Agent requests to start a recording as proposals

**Out** (and which node owns it instead):
- The journal and its clock (`CORE`)
- Off-screen rendering (`VIZ.RENDER`)
- ViewState and NamedViews (`VIZ.VIEW`)
- The timeline UI panel host (`VIZ.SHELL`)
- Control-channel screenshot transport (`AUTO`; it calls these commands)
- Signing-view evidence (`TRUST`)

## Requirements

- **REQ-VIZ.CAPTURE-001** Taking a screenshot shall be a registry command available through GUI, CLI and MCP, and shall be journaled with the image digest.
- **REQ-VIZ.CAPTURE-002** Every screenshot, view-track keyframe and video frame shall carry a timestamp from the journal's monotonic clock.
- **REQ-VIZ.CAPTURE-003** While recording, the system shall sample the view track at 10 Hz ± 1 Hz [unverified].
- **REQ-VIZ.CAPTURE-004** Video shall be stored in content-addressed segments of about 10 s, each attached to the journal by an entry carrying its digest.
- **REQ-VIZ.CAPTURE-005** If a stored segment's content does not match the digest in its journal entry, then verification shall report the segment as tampered.
- **REQ-VIZ.CAPTURE-006** While recording, a recording indicator shall be visible in every window of the app and shall not be suppressible by any theme, skin or agent.
- **REQ-VIZ.CAPTURE-007** When an agent asks to start a recording, the system shall create a proposal for a human to accept, and shall not start recording on the agent's call alone.
- **REQ-VIZ.CAPTURE-008** If org policy forbids recording, then every capture command except screenshots allowed by that policy shall fail with a policy error and journal nothing.
- **REQ-VIZ.CAPTURE-009** Recorded media shall be stored locally and shall leave the machine only through an explicit egress (X1) command.
- **REQ-VIZ.CAPTURE-010** When a turntable or camera-path animation is rendered headlessly twice from the same inputs, the frame sequences shall be identical within the VIZ.RENDER golden-image tolerance.
- **REQ-VIZ.CAPTURE-011** Viewport video capture at 1920×1080 and 30 fps shall reduce viewport frame rate by no more than 10% on the reference profile [unverified].

## Interfaces

- **Offers:** `capture.take_screenshot@1`, `capture.start_recording@1`, `capture.stop_recording@1`, `capture.attach_segment@1`, `capture.render_animation@1`, `capture.export_media@1` (X1) (names indicative; final at leaf); the view-track format; recorded-media types consumed by OUTR content and support bundles.
- **Consumes:** CORE journal clock and append; VIZ.RENDER off-screen frames and readback; VIZ.VIEW ViewState; VIZ.SHELL recording indicator and timeline host.

## Decomposition hints

SHOT (screenshots), TRACK (view track), VIDEO (readback, encode, segments), OSCAP (full-window capture), ANIM (turntable and camera paths), REPLAY (re-rendered replay), POLICY (governance and redaction).

## Split rationale

The dividing principle is **one kind of recorded medium per child, with governance in its own child**:
- SHOT (stills), TRACK (view keyframes), VIDEO (viewport pixels), OSCAP (full-window pixels, later phase) and ANIM (scripted camera stills) each own one medium and its storage;
- REC owns the session, indicator, policy and agent rule that every medium obeys;
- EXPORT is the only path from the project store to a folder.

Alternatives considered:
- **Split by host surface** (GUI capture vs headless capture). Rejected: the same commands serve both; headless is a property of VIZ.RENDER.
- **REC folded into VIDEO.** Rejected: screenshots and the view track also obey the policy, so governance must sit beside them, not inside one medium.

## Open questions

- Video codec choice (licence-compatible, hardware-encodable) is open; it may need a decision once D-001 sets the licence allowlist.
- Full-window OS capture depends on PLAT's per-OS permissions; tagged as a later phase within this subsystem.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.CAPTURE.SHOT | Screenshot command (leaf) | 001 002 | GOM.VIZ.07 |
| VIZ.CAPTURE.REC | Recording sessions, indicator and governance | 006 007 008 009 | none |
| VIZ.CAPTURE.TRACK | View track and re-rendered replay | 002 003 | none |
| VIZ.CAPTURE.VIDEO | Viewport video capture in hash-chained segments | 002 004 005 011 | none |
| VIZ.CAPTURE.ANIM | Turntable and camera-path animations (leaf) | 010 | GOM.VIZ.08 |
| VIZ.CAPTURE.EXPORT | Media export to an approved local root (leaf) | 009 | none |
| VIZ.CAPTURE.OSCAP | Full-window capture through OS APIs | 002 | none |
