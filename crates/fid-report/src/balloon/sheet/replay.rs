//! Exact replay of `RPT.BALLOON.SHEET.IMPORT` journal events (`AC-6`).
//!
//! Replay never reads the filesystem or generates fresh UUIDs: IDs come from the journal events
//! and source bytes come from the content-addressed blob backend (re-hashed on every read),
//! re-running `fid_io_draw::read_drawing_bytes` and confirming that every entity digest, sheet
//! ID, dimension and warning matches identically (`ReplayOutcome::Identical`).

use super::carry::{replay_carry_event, BalloonSink};
use super::command::AddDrawingResult;
use super::event::{canonical_digest, DrawingEvent};
use super::model::{compute_raster_bounds_cmm, DrawingRevision, DrawingSheet, ReaderWarning, SheetStatus};
use super::store::{digest_bytes, DrawingBlobBackend, DrawingStore};
use fid_io_draw::read_drawing_bytes;
use fid_replay::ReplayOutcome;
use fid_store::cas::BlobRef;
use std::collections::BTreeMap;

/// Outcome of an exact replay of drawing-sheet import, sizing and carry events (`AC-6`, `CARRY AC-4`).
#[derive(Debug, Clone)]
pub struct SheetReplayRun {
    pub outcome: ReplayOutcome,
    /// Index of the first event that failed exact replay, if any.
    pub first_mismatch: Option<usize>,
    /// Replayed store state up to the last identical event.
    pub store: DrawingStore,
}

/// Replays `events` in order against `blobs` without touching any external path or clock.
pub fn replay_exact(events: &[DrawingEvent], blobs: &dyn DrawingBlobBackend) -> SheetReplayRun {
    replay_exact_inner(events, blobs, None)
}

/// Replays `events` including [`DrawingEvent::CarryDrawingBalloon`] events against `blobs` and `sink`.
pub fn replay_exact_with_sink(
    events: &[DrawingEvent],
    blobs: &dyn DrawingBlobBackend,
    sink: &mut dyn BalloonSink,
) -> SheetReplayRun {
    replay_exact_inner(events, blobs, Some(sink))
}

fn replay_exact_inner(
    events: &[DrawingEvent],
    blobs: &dyn DrawingBlobBackend,
    mut sink: Option<&mut dyn BalloonSink>,
) -> SheetReplayRun {
    let mut store = DrawingStore::new();

    for (idx, ev) in events.iter().enumerate() {
        let step = match ev {
            DrawingEvent::CarryDrawingBalloon(carry) => match sink.as_deref_mut() {
                Some(s) => replay_carry_event(&mut store, s, carry),
                None => Err(ReplayOutcome::NotReplayable),
            },
            _ => replay_one(&mut store, blobs, ev),
        };
        match step {
            Ok(()) => {}
            Err(outcome) => {
                return SheetReplayRun { outcome, first_mismatch: Some(idx), store };
            }
        }
    }

    SheetReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, store }
}

fn replay_one(
    store: &mut DrawingStore,
    blobs: &dyn DrawingBlobBackend,
    ev: &DrawingEvent,
) -> Result<(), ReplayOutcome> {
    match ev {
        DrawingEvent::AddDrawingBalloon(add) => {
            let super::event::AddDrawingEvent {
                drawing_rev,
                part_rev,
                drawing_number,
                revision_label,
                title,
                source_digest,
                source_blob_digest,
                sheet_ids,
                warnings,
                result_digest,
            } = add.as_ref();
            if store.has_revision_key(
                *part_rev,
                drawing_number.untrusted_value().as_str(),
                revision_label.untrusted_value().as_str(),
            ) {
                return Err(ReplayOutcome::Divergent);
            }
            let raw = blobs
                .get_blob(source_blob_digest)
                .map_err(|_| ReplayOutcome::Divergent)?
                .ok_or(ReplayOutcome::NotReplayable)?;
            if digest_bytes(&raw) != *source_digest {
                return Err(ReplayOutcome::Divergent);
            }
            let parsed = read_drawing_bytes(None, &raw).map_err(|_| ReplayOutcome::Divergent)?;
            if parsed.pages.len() != sheet_ids.len() {
                return Err(ReplayOutcome::Divergent);
            }
            let replayed_warnings: Vec<ReaderWarning> =
                parsed.warnings.into_iter().map(ReaderWarning::from_draw).collect();
            if replayed_warnings != *warnings {
                return Err(ReplayOutcome::Divergent);
            }

            let mut sheets = Vec::with_capacity(parsed.pages.len());
            for (page, &sid) in parsed.pages.into_iter().zip(sheet_ids.iter()) {
                let (status, width_cmm, height_cmm, image, px) = match page.raster {
                    Some(r) => {
                        let d = digest_bytes(&r.pixels);
                        // Verify raster blob exists in CAS and matches re-decoded pixel bytes.
                        let stored_pix = blobs
                            .get_blob(&d)
                            .map_err(|_| ReplayOutcome::Divergent)?
                            .ok_or(ReplayOutcome::NotReplayable)?;
                        if stored_pix != r.pixels {
                            return Err(ReplayOutcome::Divergent);
                        }
                        let st = if r.dpi.is_some() && page.width_cmm > 0 && page.height_cmm > 0 {
                            SheetStatus::Ready
                        } else {
                            SheetStatus::NeedsSize
                        };
                        (
                            st,
                            page.width_cmm,
                            page.height_cmm,
                            Some(BlobRef { digest: d, size: r.pixels.len() as u64 }),
                            Some((r.width, r.height)),
                        )
                    }
                    None => (SheetStatus::Ready, page.width_cmm, page.height_cmm, None, None),
                };
                sheets.push(DrawingSheet {
                    id: sid,
                    drawing_rev: *drawing_rev,
                    index: page.index,
                    width_cmm,
                    height_cmm,
                    status,
                    image,
                    px,
                });
            }

            let rev = DrawingRevision {
                id: *drawing_rev,
                part_rev: *part_rev,
                drawing_number: drawing_number.clone(),
                revision_label: revision_label.clone(),
                title: title.clone(),
                source_digest: *source_digest,
                sheets: sheet_ids.clone(),
                warnings: warnings.clone(),
                carried_from: BTreeMap::new(),
            };
            let res = AddDrawingResult {
                drawing_rev: rev.clone(),
                sheets: sheets.clone(),
                warnings: warnings.clone(),
                committed: true,
            };
            let got = canonical_digest(&res.to_value()).map_err(|_| ReplayOutcome::Divergent)?;
            if got != *result_digest {
                return Err(ReplayOutcome::Divergent);
            }
            store.insert_revision_and_sheets(rev, sheets, ev.clone());
            Ok(())
        }
        DrawingEvent::SetSizeDrawingBalloon { sheet, size, width_cmm, height_cmm, result_digest } => {
            let existing = store.sheet(*sheet).ok_or(ReplayOutcome::Divergent)?.clone();
            let px = existing.px.ok_or(ReplayOutcome::Divergent)?;
            let (w, h) = compute_raster_bounds_cmm(px, *size).map_err(|_| ReplayOutcome::Divergent)?;
            if w != *width_cmm || h != *height_cmm {
                return Err(ReplayOutcome::Divergent);
            }
            let mut updated = existing;
            updated.width_cmm = w;
            updated.height_cmm = h;
            updated.status = SheetStatus::Ready;
            let got = canonical_digest(&updated.to_value()).map_err(|_| ReplayOutcome::Divergent)?;
            if got != *result_digest {
                return Err(ReplayOutcome::Divergent);
            }
            store.update_sheet_bounds(*sheet, w, h, ev.clone()).map_err(|_| ReplayOutcome::Divergent)?;
            Ok(())
        }
        DrawingEvent::CarryDrawingBalloon(_) => Err(ReplayOutcome::NotReplayable),
    }
}
