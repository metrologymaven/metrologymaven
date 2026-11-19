//! `LasReader`: validates the whole layout against the real file length, then streams
//! point records in batches.

use crate::bytes::{arr, u16_at, u64_at};
use crate::error::{CountUnit, LasError, LasField};
use crate::format::PointFormat;
use crate::header::{standard_header_len, LasHeader, HEADER_LEN_1_0, HEADER_LEN_1_4};
use crate::points::{ExtraValues, RawPointBatch};
use crate::vlr::{
    extra_layout, BoundedBytes, ExtraSpec, RecordPayload, Vlr, VlrKind, DEFAULT_MAX_PAYLOAD, EVLR_HEADER_LEN,
    VLR_HEADER_LEN,
};
use fid_io_reg::{LimitKind, Limits};
use std::io::{Read, Seek, SeekFrom};

/// Default number of points per batch (leaf Design).
pub const DEFAULT_BATCH_POINTS: usize = 65_536;
/// Bytes of point records read from the source at a time.
const CHUNK_BYTES: usize = 1 << 20;
/// Tracked bytes per point on top of the record (columns are wider than the record).
const COLUMN_OVERHEAD: u64 = 64;

/// Options of [`LasReader::open_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReaderOptions {
    /// Reader harness limits (IO.REG.LIMITS): file bytes, elements (VLRs, EVLRs and
    /// points), tracked allocation (kept payloads and one batch).
    pub limits: Limits,
    /// Bound of one VLR or EVLR payload, and of the bytes between the last VLR and the
    /// point data (default 64 MiB).
    pub max_record_payload: u64,
    /// Points per batch (default 65,536; at least 1).
    pub batch_points: usize,
}

impl Default for ReaderOptions {
    fn default() -> Self {
        ReaderOptions {
            limits: Limits::DEFAULT,
            max_record_payload: DEFAULT_MAX_PAYLOAD,
            batch_points: DEFAULT_BATCH_POINTS,
        }
    }
}

/// What the reader counted of waveform data without importing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WaveformSummary {
    /// Bytes of waveform data packets (EVLR payloads) skipped.
    pub data_bytes: u64,
    /// Points read so far whose wave packet descriptor index is not zero.
    pub points_with_packets: u64,
}

/// A LAS file read whole: everything [`LasReader`] keeps, plus all points in one batch.
#[derive(Debug, Clone, PartialEq)]
pub struct LasFile {
    /// The header as read.
    pub header: LasHeader,
    /// VLRs in file order.
    pub vlrs: Vec<Vlr>,
    /// Bytes between the last VLR and the point data, kept verbatim.
    pub vlr_padding: Vec<u8>,
    /// All points.
    pub points: RawPointBatch,
    /// EVLRs in file order (for 1.3, the waveform data packet record).
    pub evlrs: Vec<Vlr>,
    /// Waveform data counted, not imported.
    pub waveform: WaveformSummary,
}

/// Reads a LAS 1.0 to 1.4 file. [`LasReader::open`] validates every size, offset and
/// count against the file before any point is read; [`LasReader::next_batch`] streams
/// the points.
#[derive(Debug)]
pub struct LasReader<R> {
    inner: R,
    opts: ReaderOptions,
    header: LasHeader,
    format: PointFormat,
    record_len: usize,
    vlrs: Vec<Vlr>,
    vlr_padding: Vec<u8>,
    evlrs: Vec<Vlr>,
    extra: Vec<ExtraSpec>,
    declared: u64,
    read: u64,
    pos: u64,
    waveform: WaveformSummary,
    tracked: u64,
}

fn io_err(at: u64) -> impl Fn(std::io::Error) -> LasError {
    move |e| LasError::io(&e, at)
}

impl<R: Read + Seek> LasReader<R> {
    /// Opens a file with default options.
    pub fn open(inner: R) -> Result<Self, LasError> {
        Self::open_with(inner, ReaderOptions::default())
    }

    /// Opens a file: reads the header, every VLR and EVLR, and checks each declared size,
    /// offset and count against the file length (REQ-IO.CLOUD.LAS.HDR-001, -002, -005).
    pub fn open_with(mut inner: R, opts: ReaderOptions) -> Result<Self, LasError> {
        let file_len = inner.seek(SeekFrom::End(0)).map_err(io_err(0))?;
        let limits = opts.limits;
        if file_len > limits.bytes() {
            return Err(LasError::LimitExceeded { limit: LimitKind::Bytes, value: limits.bytes(), at: limits.bytes() });
        }
        let mut reader = LasReader {
            inner,
            opts,
            header: LasHeader::default(),
            format: PointFormat::FORMAT_6,
            record_len: 0,
            vlrs: Vec::new(),
            vlr_padding: Vec::new(),
            evlrs: Vec::new(),
            extra: Vec::new(),
            declared: 0,
            read: 0,
            pos: 0,
            waveform: WaveformSummary::default(),
            tracked: 0,
        };

        // Header block: the signature and version decide how long it is.
        let prefix_len = file_len.min(u64::from(HEADER_LEN_1_4));
        let mut prefix = vec![0u8; usize::from(prefix_len as u16)];
        reader.read_at(0, &mut prefix)?;
        if prefix.get(..4) != Some(&b"LASF"[..]) {
            return Err(LasError::malformed(LasField::Signature));
        }
        if prefix.len() < 26 {
            return Err(LasError::TruncatedFile {
                declared: u64::from(HEADER_LEN_1_0),
                actual: file_len,
                unit: CountUnit::Bytes,
            });
        }
        let mut header = LasHeader::parse(&prefix)?;
        let std_len = standard_header_len(header.version_major, header.version_minor).unwrap_or(HEADER_LEN_1_4);

        // Header size and point data offset against the file.
        let header_size = u64::from(header.header_size);
        if header.header_size < std_len || header_size > file_len {
            return Err(LasError::malformed(LasField::HeaderSize));
        }
        let data_start = u64::from(header.offset_to_point_data);
        if data_start < header_size || data_start > file_len {
            return Err(LasError::malformed(LasField::OffsetToPointData));
        }
        let format = header.format().ok_or_else(|| LasError::malformed(LasField::PointDataFormat))?;
        if header.point_data_record_length < format.base_len() {
            return Err(LasError::malformed(LasField::PointDataRecordLength));
        }
        let record_len = u64::from(header.point_data_record_length);
        let declared = header.declared_points()?;

        // Where the point data ends: the first EVLR, or the end of the file.
        let minor = header.version_minor;
        let (evlr_start, evlr_count) = if minor >= 4 && header.number_of_evlrs > 0 {
            let s = header.start_of_first_evlr;
            if s < data_start || s > file_len {
                return Err(LasError::malformed(LasField::StartOfFirstEvlr));
            }
            (Some(s), u64::from(header.number_of_evlrs))
        } else if minor == 3 && header.start_of_waveform_data != 0 {
            let s = header.start_of_waveform_data;
            if s < data_start || s > file_len {
                return Err(LasError::malformed(LasField::StartOfWaveformData));
            }
            (Some(s), 1)
        } else {
            (None, 0)
        };
        let data_end = evlr_start.unwrap_or(file_len);
        if minor >= 4 && header.start_of_waveform_data != 0 {
            let s = header.start_of_waveform_data;
            if s < data_end || s >= file_len {
                return Err(LasError::malformed(LasField::StartOfWaveformData));
            }
        }
        let data_bytes = data_end - data_start;
        if declared.checked_mul(record_len) != Some(data_bytes) {
            return Err(LasError::TruncatedFile {
                declared,
                actual: data_bytes / record_len.max(1),
                unit: CountUnit::Points,
            });
        }
        let elements = declared.saturating_add(u64::from(header.number_of_vlrs)).saturating_add(evlr_count);
        if elements > limits.elements() {
            return Err(LasError::LimitExceeded {
                limit: LimitKind::Elements,
                value: limits.elements(),
                at: data_start,
            });
        }

        // User bytes after the version's header block.
        let extra_len = header_size - u64::from(std_len);
        header.header_extra = reader.read_vec(u64::from(std_len), extra_len)?;

        // VLRs: each header and payload must end at or before the point data.
        let mut pos = header_size;
        for _ in 0..header.number_of_vlrs {
            let header_end = pos.saturating_add(VLR_HEADER_LEN as u64);
            if header_end > data_start {
                return Err(LasError::malformed(LasField::NumberOfVlrs));
            }
            let mut raw = [0u8; VLR_HEADER_LEN];
            reader.read_at(pos, &mut raw)?;
            let len = u64::from(u16_at(&raw, 20));
            if header_end.saturating_add(len) > data_start {
                return Err(LasError::malformed(LasField::VlrLength));
            }
            let payload = reader.read_payload(header_end, len)?;
            reader.vlrs.push(Vlr {
                reserved: u16_at(&raw, 0),
                user_id: arr(&raw, 2),
                record_id: u16_at(&raw, 18),
                description: arr(&raw, 22),
                payload: RecordPayload::Loaded(payload),
            });
            pos = header_end + len;
        }
        let padding = reader.read_payload(pos, data_start - pos)?;
        reader.vlr_padding = padding.as_bytes().to_vec();

        // EVLRs: each header and payload must end at or before the end of the file.
        let mut pos = data_end;
        for _ in 0..evlr_count {
            let header_end = pos.saturating_add(EVLR_HEADER_LEN as u64);
            if header_end > file_len {
                return Err(LasError::malformed(LasField::NumberOfEvlrs));
            }
            let mut raw = [0u8; EVLR_HEADER_LEN];
            reader.read_at(pos, &mut raw)?;
            let len = u64_at(&raw, 20);
            if len > file_len - header_end {
                return Err(LasError::malformed(LasField::EvlrLength));
            }
            let mut evlr = Vlr {
                reserved: u16_at(&raw, 0),
                user_id: arr(&raw, 2),
                record_id: u16_at(&raw, 18),
                description: arr(&raw, 28),
                payload: RecordPayload::NotImported { len },
            };
            if evlr.kind() == VlrKind::WaveformData {
                reader.waveform.data_bytes = reader.waveform.data_bytes.saturating_add(len);
            } else {
                evlr.payload = RecordPayload::Loaded(reader.read_payload(header_end, len)?);
            }
            reader.evlrs.push(evlr);
            pos = header_end + len;
        }

        let extra_bytes = usize::from(header.point_data_record_length - format.base_len());
        reader.extra = extra_layout(&reader.vlrs, &reader.evlrs, extra_bytes)?;
        reader.header = header;
        reader.format = format;
        reader.record_len = usize::from(reader.header.point_data_record_length);
        reader.declared = declared;
        reader.pos = data_start;
        Ok(reader)
    }

    /// Reads exactly `buf.len()` bytes at `at`.
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), LasError> {
        self.inner.seek(SeekFrom::Start(at)).map_err(io_err(at))?;
        self.inner.read_exact(buf).map_err(io_err(at))
    }

    /// Reads a payload of `len` bytes at `at` (already checked to lie inside the file),
    /// bounded by the per-record bound and the tracked allocation limit.
    fn read_payload(&mut self, at: u64, len: u64) -> Result<BoundedBytes, LasError> {
        if len > self.opts.max_record_payload {
            return Err(LasError::LimitExceeded { limit: LimitKind::Alloc, value: self.opts.max_record_payload, at });
        }
        let bytes = self.read_vec(at, len)?;
        BoundedBytes::new(bytes, self.opts.max_record_payload)
    }

    /// Reads `len` bytes at `at` into a new vector, charging them to the tracked
    /// allocation limit first.
    fn read_vec(&mut self, at: u64, len: u64) -> Result<Vec<u8>, LasError> {
        self.charge(len, at)?;
        let n = usize::try_from(len).map_err(|_| LasError::LimitExceeded {
            limit: LimitKind::Alloc,
            value: self.opts.limits.alloc(),
            at,
        })?;
        let mut v = vec![0u8; n];
        self.read_at(at, &mut v)?;
        Ok(v)
    }

    /// Charges `n` bytes of kept data to the tracked allocation limit.
    fn charge(&mut self, n: u64, at: u64) -> Result<(), LasError> {
        let alloc = self.opts.limits.alloc();
        match self.tracked.checked_add(n) {
            Some(t) if t <= alloc => {
                self.tracked = t;
                Ok(())
            }
            _ => Err(LasError::LimitExceeded { limit: LimitKind::Alloc, value: alloc, at }),
        }
    }

    /// The header as read.
    pub fn header(&self) -> &LasHeader {
        &self.header
    }

    /// The point format.
    pub fn format(&self) -> PointFormat {
        self.format
    }

    /// The VLRs in file order.
    pub fn vlrs(&self) -> &[Vlr] {
        &self.vlrs
    }

    /// Bytes between the last VLR and the point data.
    pub fn vlr_padding(&self) -> &[u8] {
        &self.vlr_padding
    }

    /// The EVLRs in file order.
    pub fn evlrs(&self) -> &[Vlr] {
        &self.evlrs
    }

    /// The extra-bytes attributes of each record.
    pub fn extra_layout(&self) -> &[ExtraSpec] {
        &self.extra
    }

    /// The number of points the header declares (and the data holds).
    pub fn point_count(&self) -> u64 {
        self.declared
    }

    /// Points not yet returned by [`LasReader::next_batch`].
    pub fn remaining(&self) -> u64 {
        self.declared - self.read
    }

    /// Waveform data counted so far.
    pub fn waveform(&self) -> WaveformSummary {
        self.waveform
    }

    /// The next batch of up to [`ReaderOptions::batch_points`] points, or `None` when all
    /// points have been read.
    pub fn next_batch(&mut self) -> Result<Option<RawPointBatch>, LasError> {
        let batch = self.opts.batch_points.max(1) as u64;
        self.read_points(batch)
    }

    fn read_points(&mut self, max_points: u64) -> Result<Option<RawPointBatch>, LasError> {
        let n = self.remaining().min(max_points);
        if n == 0 {
            return Ok(None);
        }
        let rec = self.record_len as u64;
        let need = n.saturating_mul(rec.saturating_add(COLUMN_OVERHEAD));
        let alloc = self.opts.limits.alloc();
        let too_big = LasError::LimitExceeded { limit: LimitKind::Alloc, value: alloc, at: self.pos };
        if self.tracked.saturating_add(need) > alloc {
            return Err(too_big);
        }
        let count = usize::try_from(n).map_err(|_| too_big.clone())?;
        let mut out = RawPointBatch::with_capacity(self.format, &self.extra, 0);
        reserve(&mut out, count, &self.extra).map_err(|_| too_big)?;
        let per_chunk = (CHUNK_BYTES / self.record_len.max(1)).max(1);
        let mut buf = Vec::new();
        let mut left = count;
        while left > 0 {
            let k = left.min(per_chunk);
            buf.resize(k * self.record_len, 0);
            let at = self.pos;
            self.read_at(at, &mut buf)?;
            for r in buf.chunks_exact(self.record_len) {
                out.push_record(r);
            }
            self.pos = at + buf.len() as u64;
            left -= k;
        }
        self.read += n;
        self.waveform.points_with_packets += out.points_with_wave_packets();
        Ok(Some(out))
    }

    /// Reads every remaining point into one batch, and returns the whole file.
    pub fn into_file(mut self) -> Result<LasFile, LasError> {
        let points = match self.read_points(self.remaining())? {
            Some(b) => b,
            None => RawPointBatch::with_capacity(self.format, &self.extra, 0),
        };
        Ok(LasFile {
            header: self.header,
            vlrs: self.vlrs,
            vlr_padding: self.vlr_padding,
            points,
            evlrs: self.evlrs,
            waveform: self.waveform,
        })
    }
}

/// Reserves room for `n` points in every column, failing instead of aborting when the
/// allocator refuses.
fn reserve(b: &mut RawPointBatch, n: usize, extra: &[ExtraSpec]) -> Result<(), std::collections::TryReserveError> {
    let a = &mut b.attrs;
    b.xi.try_reserve_exact(n)?;
    b.yi.try_reserve_exact(n)?;
    b.zi.try_reserve_exact(n)?;
    a.intensity.try_reserve_exact(n)?;
    for v in [
        &mut a.return_number,
        &mut a.number_of_returns,
        &mut a.scan_direction,
        &mut a.edge_of_flight_line,
        &mut a.classification,
        &mut a.class_flags,
        &mut a.user_data,
    ] {
        v.try_reserve_exact(n)?;
    }
    a.point_source_id.try_reserve_exact(n)?;
    if let Some(v) = a.scanner_channel.as_mut() {
        v.try_reserve_exact(n)?;
    }
    if let Some(v) = a.scan_angle_rank.as_mut() {
        v.try_reserve_exact(n)?;
    }
    if let Some(v) = a.scan_angle.as_mut() {
        v.try_reserve_exact(n)?;
    }
    if let Some(v) = a.gps_time.as_mut() {
        v.try_reserve_exact(n)?;
    }
    if let Some(v) = a.rgb.as_mut() {
        v.try_reserve_exact(n)?;
    }
    if let Some(v) = a.nir.as_mut() {
        v.try_reserve_exact(n)?;
    }
    if let Some(v) = a.wave_packet.as_mut() {
        v.try_reserve_exact(n)?;
    }
    for (c, spec) in a.extra.iter_mut().zip(extra) {
        let elems = n.saturating_mul(spec.count);
        match &mut c.values {
            ExtraValues::Raw(v) | ExtraValues::U8(v) => v.try_reserve_exact(elems)?,
            ExtraValues::I8(v) => v.try_reserve_exact(elems)?,
            ExtraValues::U16(v) => v.try_reserve_exact(elems)?,
            ExtraValues::I16(v) => v.try_reserve_exact(elems)?,
            ExtraValues::U32(v) => v.try_reserve_exact(elems)?,
            ExtraValues::I32(v) => v.try_reserve_exact(elems)?,
            ExtraValues::U64(v) => v.try_reserve_exact(elems)?,
            ExtraValues::I64(v) => v.try_reserve_exact(elems)?,
            ExtraValues::F32(v) => v.try_reserve_exact(elems)?,
            ExtraValues::F64(v) => v.try_reserve_exact(elems)?,
        }
    }
    Ok(())
}

/// Reads a whole LAS file from memory with `opts`.
pub fn read_slice(data: &[u8], opts: ReaderOptions) -> Result<LasFile, LasError> {
    LasReader::open_with(std::io::Cursor::new(data), opts)?.into_file()
}
