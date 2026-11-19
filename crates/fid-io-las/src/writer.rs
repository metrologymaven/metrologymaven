//! `LasWriter`: header, VLRs, point records and EVLRs, byte-exact
//! (REQ-IO.CLOUD.LAS.HDR-004).
//!
//! The writer takes every field from the caller's [`LasHeader`] except the layout fields,
//! which it computes from what it writes: header size, point data offset, number of VLRs,
//! and (by version) the waveform record offset, first EVLR offset and number of EVLRs. For
//! a file read by [`crate::LasReader`] these computed values equal the stored ones, so
//! reading and writing back reproduces every record byte for byte.

use crate::error::{LasError, LasField};
use crate::format::PointFormat;
use crate::header::LasHeader;
use crate::points::RawPointBatch;
use crate::reader::LasFile;
use crate::vlr::{RecordPayload, Vlr, VlrKind, EVLR_HEADER_LEN, VLR_HEADER_LEN};
use std::io::Write;

/// Writes a LAS file to a byte sink. The point count is the header's (the 64-bit count
/// for 1.4, the legacy count before); [`LasWriter::finish`] fails if a different number
/// of points was written.
#[derive(Debug)]
pub struct LasWriter<W: Write> {
    out: W,
    header: LasHeader,
    format: PointFormat,
    extra_width: usize,
    evlrs: Vec<Vlr>,
    declared: u64,
    written: u64,
    at: u64,
    buf: Vec<u8>,
}

fn bad(field: LasField) -> LasError {
    LasError::malformed(field)
}

impl<W: Write> LasWriter<W> {
    /// Computes the layout, then writes the header block, the VLRs and `vlr_padding`.
    /// `evlrs` are written by [`LasWriter::finish`]; version 1.3 allows at most one (the
    /// waveform data packet record), earlier versions none. An EVLR whose payload was not
    /// imported cannot be written (`malformed{evlr_payload}`).
    pub fn new(out: W, header: &LasHeader, vlrs: &[Vlr], vlr_padding: &[u8], evlrs: &[Vlr]) -> Result<Self, LasError> {
        let mut h = header.clone();
        let std_len = h
            .standard_len()
            .ok_or(LasError::LasVersionUnsupported { major: h.version_major, minor: h.version_minor })?;
        let format = h.format().ok_or_else(|| bad(LasField::PointDataFormat))?;
        let extra_width = h
            .point_data_record_length
            .checked_sub(format.base_len())
            .ok_or_else(|| bad(LasField::PointDataRecordLength))?;
        let declared = h.declared_points()?;

        h.header_size = u16::try_from(h.header_extra.len())
            .ok()
            .and_then(|e| std_len.checked_add(e))
            .ok_or_else(|| bad(LasField::HeaderSize))?;
        let mut offset = u64::from(h.header_size);
        for v in vlrs {
            let len = match &v.payload {
                RecordPayload::Loaded(b) => u16::try_from(b.len()).map_err(|_| bad(LasField::VlrLength))?,
                RecordPayload::NotImported { .. } => return Err(bad(LasField::VlrLength)),
            };
            offset += (VLR_HEADER_LEN + usize::from(len)) as u64;
        }
        offset += vlr_padding.len() as u64;
        h.offset_to_point_data = u32::try_from(offset).map_err(|_| bad(LasField::OffsetToPointData))?;
        h.number_of_vlrs = u32::try_from(vlrs.len()).map_err(|_| bad(LasField::NumberOfVlrs))?;

        let point_end = declared
            .checked_mul(u64::from(h.point_data_record_length))
            .and_then(|b| b.checked_add(offset))
            .ok_or_else(|| bad(LasField::PointCount))?;
        for e in evlrs {
            if matches!(e.payload, RecordPayload::NotImported { .. }) {
                return Err(bad(LasField::EvlrPayload));
            }
        }
        match h.version_minor {
            4 => {
                h.number_of_evlrs = u32::try_from(evlrs.len()).map_err(|_| bad(LasField::NumberOfEvlrs))?;
                if !evlrs.is_empty() {
                    h.start_of_first_evlr = point_end;
                }
                let mut at = point_end;
                h.start_of_waveform_data = 0;
                for e in evlrs {
                    if e.kind() == VlrKind::WaveformData {
                        h.start_of_waveform_data = at;
                        break;
                    }
                    at = at.saturating_add(EVLR_HEADER_LEN as u64).saturating_add(e.payload.len());
                }
            }
            3 => {
                h.start_of_waveform_data = match evlrs.len() {
                    0 => 0,
                    1 => point_end,
                    _ => return Err(bad(LasField::NumberOfEvlrs)),
                };
            }
            _ => {
                if !evlrs.is_empty() {
                    return Err(bad(LasField::NumberOfEvlrs));
                }
            }
        }

        let mut w = LasWriter {
            out,
            header: h,
            format,
            extra_width: usize::from(extra_width),
            evlrs: evlrs.to_vec(),
            declared,
            written: 0,
            at: 0,
            buf: Vec::new(),
        };
        let head = w.header.to_bytes();
        w.put(&head)?;
        for v in vlrs {
            let payload = v.payload.bytes().map(|b| b.as_bytes()).unwrap_or(&[]);
            let head = v.vlr_header(u16::try_from(payload.len()).unwrap_or(0));
            w.put(&head)?;
            w.put(payload)?;
        }
        w.put(vlr_padding)?;
        Ok(w)
    }

    fn put(&mut self, b: &[u8]) -> Result<(), LasError> {
        self.out.write_all(b).map_err(|e| LasError::io(&e, self.at))?;
        self.at += b.len() as u64;
        Ok(())
    }

    /// The header as written (layout fields computed).
    pub fn header(&self) -> &LasHeader {
        &self.header
    }

    /// Writes the batch's points. The batch's format and extra columns must match the
    /// header (`malformed{point_data_format}`, `malformed{column}`), each bit field must
    /// fit (`malformed{value_range}`), and the total may not pass the header's count
    /// (`malformed{point_count}`). Nothing of the batch is written if any check fails.
    pub fn write_batch(&mut self, batch: &RawPointBatch) -> Result<(), LasError> {
        if batch.format != self.format {
            return Err(bad(LasField::PointDataFormat));
        }
        batch.check_shape()?;
        if batch.extra_width() != self.extra_width {
            return Err(bad(LasField::Column));
        }
        let n = batch.len() as u64;
        if !matches!(self.written.checked_add(n), Some(t) if t <= self.declared) {
            return Err(bad(LasField::PointCount));
        }
        let rec = usize::from(self.header.point_data_record_length);
        let mut buf = std::mem::take(&mut self.buf);
        // First pass: every value must encode, so a bad value leaves the output untouched.
        for p in 0..batch.len() {
            buf.clear();
            batch.encode_record(p, &mut buf)?;
        }
        let step = ((1usize << 20) / rec.max(1)).max(1);
        let mut i = 0;
        while i < batch.len() {
            let end = i.saturating_add(step).min(batch.len());
            buf.clear();
            for p in i..end {
                batch.encode_record(p, &mut buf)?;
            }
            self.put(&buf)?;
            i = end;
        }
        self.buf = buf;
        self.written += n;
        Ok(())
    }

    /// Checks the point count, writes the EVLRs and flushes; returns the sink.
    pub fn finish(mut self) -> Result<W, LasError> {
        if self.written != self.declared {
            return Err(bad(LasField::PointCount));
        }
        let evlrs = std::mem::take(&mut self.evlrs);
        for e in &evlrs {
            let payload = e.payload.bytes().map(|b| b.as_bytes()).unwrap_or(&[]);
            let head = e.evlr_header(payload.len() as u64);
            self.put(&head)?;
            self.put(payload)?;
        }
        self.out.flush().map_err(|e| LasError::io(&e, self.at))?;
        Ok(self.out)
    }
}

/// Writes a whole [`LasFile`] (as returned by [`crate::read_slice`]) to bytes.
pub fn write_file(file: &LasFile) -> Result<Vec<u8>, LasError> {
    let mut w = LasWriter::new(Vec::new(), &file.header, &file.vlrs, &file.vlr_padding, &file.evlrs)?;
    w.write_batch(&file.points)?;
    w.finish()
}
