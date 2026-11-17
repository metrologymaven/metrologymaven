---
id: LVM.GUIDE.ALARM
title: Alarm rules, alarm state and acknowledgement
level: 3
parent: LVM.GUIDE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.GUIDE-003, REQ-LVM.GUIDE-004, REQ-LVM.GUIDE-005, REQ-LVM.GUIDE-006]
covers: [SA.BLD.05]
depends_on: [LVM.GUIDE.DIRECT, LVM.WATCH.PIPE, VIZ.SHELL]
cli: [fid build set-alarm-rule, fid build ack-alarm, fid build silence-alarm, fid build list-alarms]
mcp: [build_set_alarm_rule, build_ack_alarm, build_silence_alarm, build_list_alarms]
journal: [build.set_alarm_rule@1, build.ack_alarm@1, build.silence_alarm@1]
updated: 2026-11-16
---

# LVM.GUIDE.ALARM: Alarm rules, alarm state and acknowledgement

## Purpose

Raises visual and audible alarms when a watched target leaves its tolerance band, when live data goes stale, or when a target drifts after set-in, and records every acknowledgement and silencing. Per CONSTITUTION §3.1 (D-007), the live sound or visual alarm is a UI effect, not a command; acknowledging or silencing it is a journaled W2 command.

## Scope

**In:**
- Alarm rules: out-of-tolerance, stale data, drift after set-in; per target or per session, with hysteresis and debounce settings
- Alarm state machine: inactive, active, acknowledged, silenced, cleared
- Alarm presentation contract for VIZ: severity, text, shape and sound pattern, so alarms are distinguishable without colour
- Acknowledge and silence commands with user and reason
- Latency budget from deviation update to alarm raised

**Out** (and which node owns it instead):
- Producing deviations and stale flags (`LVM.WATCH.PIPE`, `LVM.GUIDE.DIRECT`)
- Drawing and playing sound (`VIZ.SHELL`; UI effect, not a command)
- Set-in state used by the drift rule (`LVM.GUIDE.SESSION`)

## Requirements

- **REQ-LVM.GUIDE.ALARM-001** When a guidance update changes a target's overall state from `in` to `out`, the system shall raise an out-of-tolerance alarm within 100 ms of the deviation update [unverified].
- **REQ-LVM.GUIDE.ALARM-002** If a target's live data goes stale while the target is active, then the system shall raise a stale-data alarm and the target shall show `unknown`, never `in`.
- **REQ-LVM.GUIDE.ALARM-003** Each alarm kind shall have a distinct shape, text label and sound pattern in the presentation contract, so no alarm relies on colour alone.
- **REQ-LVM.GUIDE.ALARM-004** When `build.ack_alarm@1` or `build.silence_alarm@1` runs, the system shall journal the alarm ID, the principal and the optional reason; silencing shall require a reason.
- **REQ-LVM.GUIDE.ALARM-005** If an agent principal acknowledges or silences an alarm, then the event shall be written to the agent branch and the live alarm shall stay active until a human acknowledges it.
- **REQ-LVM.GUIDE.ALARM-006** Raising and clearing alarms shall not be journaled as commands; the alarm history shall be a derived log of rule evaluations kept with the session diagnostics.
- **REQ-LVM.GUIDE.ALARM-007** When `build.set_alarm_rule@1` runs, the system shall store the rule with its thresholds, hysteresis and debounce, and journal one event.

## Interfaces

- **Offers:** `build.set_alarm_rule@1`, `build.ack_alarm@1`, `build.silence_alarm@1` (W2), `build.list_alarms@1` (R0); alarm presentation events for VIZ.
- **Consumes:** `LVM.GUIDE.DIRECT` states; `LVM.WATCH.PIPE` stale flags; `LVM.GUIDE.SESSION` set-in records; `VIZ.SHELL` accessibility baseline and sound output.

## Decomposition hints

RULES (rule definitions and evaluation), STATE (alarm state machine and latency), ACK (acknowledge and silence commands), PRESENT (presentation contract for VIZ).

## Open questions

- Resolved by CONSTITUTION §3.1 (D-007): alarm sound and visuals are UI effects, not D1 device I/O; acknowledgement is W2.
- Default hysteresis and debounce values need a large-volume metrology reviewer.
- Should silenced alarms re-arm after a time limit?

## Children

_None yet: status `open`._
