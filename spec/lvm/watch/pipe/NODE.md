---
id: LVM.WATCH.PIPE
title: Live update pipeline, decimation and stale state
level: 3
parent: LVM.WATCH
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.WATCH-002, REQ-LVM.WATCH-003]
covers: []
depends_on: [LVM.WATCH.WINDOW, HAL.LIVE, HAL.ACQ]
cli: [fid build read-watch, fid build set-stale-timeout]
mcp: [build_read_watch, build_set_stale_timeout]
journal: [build.set_stale_timeout@1]
updated: 2026-11-16
---

# LVM.WATCH.PIPE: Live update pipeline, decimation and stale state

## Purpose

Moves live samples from a HAL stream through the kernel to every subscribed watch window, vector group, live relationship and guide, fast enough for building and fixturing work. It decimates high-rate streams to display rate without losing the latest sample, and marks data stale when the stream stops. Live values are observed (determinism N) and never become values of record unless captured.

## Scope

**In:**
- Subscription of watch consumers to a station's live stream
- Per-sample kernel computation of readouts and deviations
- Decimation to display rate that always keeps the newest sample
- Latency measurement and the latency budget
- Stale timeout and stale state
- `build.read_watch@1` live read query

**Out** (and which node owns it instead):
- Device streams and back-pressure (`HAL.ACQ`, `HAL.LIVE`)
- Readout definitions (`LVM.WATCH.WINDOW`)
- Committing readings (`LVM.WATCH.CAPTURE`)
- Drawing (`VIZ`)

## Requirements

- **REQ-LVM.WATCH.PIPE-001** While a live instrument is streaming, the system shall compute every subscribed readout on the CPU kernel and publish it within 50 ms of the sample's arrival [unverified], for instrument rates up to 1 kHz.
- **REQ-LVM.WATCH.PIPE-002** When samples arrive faster than the display rate, the system shall decimate them and the published value shall always be computed from the newest sample.
- **REQ-LVM.WATCH.PIPE-003** If no sample has arrived within the stale timeout set by `build.set_stale_timeout@1` (default 1 s) [unverified], then every readout of that stream shall switch to a stale state and shall not present the last value as current.
- **REQ-LVM.WATCH.PIPE-004** The pipeline shall publish, with each readout, the sample timestamp, the publish timestamp and a stale flag, so consumers such as `LVM.GUIDE` can enforce their own rules.
- **REQ-LVM.WATCH.PIPE-005** The pipeline shall record its measured end-to-end latency percentiles per session as diagnostics.

## Interfaces

- **Offers:** `build.read_watch@1` (R0, determinism N); `build.set_stale_timeout@1` (W2); a deviation stream of `LiveReadout { value, sample_time, publish_time, stale }` for `LVM.WATCH.VECTOR`, `LVM.WATCH.LIVEREL` and `LVM.GUIDE`.
- **Consumes:** `HAL.LIVE` and `HAL.ACQ` streams; `LVM.WATCH.WINDOW` readout function.

## Decomposition hints

SUB (subscriptions), DECIM (decimation and newest-sample rule), STALE (timeouts), LAT (latency diagnostics).

## Open questions

- The 50 ms and 1 kHz figures need measurement on reference hardware.

## Children

_None yet: status `open`._
