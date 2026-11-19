//! Point records as integer coordinates plus typed attribute columns
//! (REQ-IO.CLOUD.LAS.HDR-003), and the exact inverse used by the writer
//! (REQ-IO.CLOUD.LAS.HDR-004).

use crate::bytes::{arr, f64_at, i32_at, u16_at, u8_at};
use crate::error::{LasError, LasField};
use crate::format::{PointFormat, WAVE_PACKET_LEN};
use crate::vlr::{ExtraKind, ExtraSpec};

/// The values of one extra-bytes attribute, `count` elements per point, point-major.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtraValues {
    /// Undocumented bytes.
    Raw(Vec<u8>),
    /// Unsigned 8-bit.
    U8(Vec<u8>),
    /// Signed 8-bit.
    I8(Vec<i8>),
    /// Unsigned 16-bit.
    U16(Vec<u16>),
    /// Signed 16-bit.
    I16(Vec<i16>),
    /// Unsigned 32-bit.
    U32(Vec<u32>),
    /// Signed 32-bit.
    I32(Vec<i32>),
    /// Unsigned 64-bit.
    U64(Vec<u64>),
    /// Signed 64-bit.
    I64(Vec<i64>),
    /// IEEE binary32 (bit-exact).
    F32(Vec<f32>),
    /// IEEE binary64 (bit-exact).
    F64(Vec<f64>),
}

impl ExtraValues {
    fn empty(kind: ExtraKind) -> ExtraValues {
        match kind {
            ExtraKind::Raw => ExtraValues::Raw(Vec::new()),
            ExtraKind::U8 => ExtraValues::U8(Vec::new()),
            ExtraKind::I8 => ExtraValues::I8(Vec::new()),
            ExtraKind::U16 => ExtraValues::U16(Vec::new()),
            ExtraKind::I16 => ExtraValues::I16(Vec::new()),
            ExtraKind::U32 => ExtraValues::U32(Vec::new()),
            ExtraKind::I32 => ExtraValues::I32(Vec::new()),
            ExtraKind::U64 => ExtraValues::U64(Vec::new()),
            ExtraKind::I64 => ExtraValues::I64(Vec::new()),
            ExtraKind::F32 => ExtraValues::F32(Vec::new()),
            ExtraKind::F64 => ExtraValues::F64(Vec::new()),
        }
    }

    /// Number of elements held.
    pub fn len(&self) -> usize {
        match self {
            ExtraValues::Raw(v) | ExtraValues::U8(v) => v.len(),
            ExtraValues::I8(v) => v.len(),
            ExtraValues::U16(v) => v.len(),
            ExtraValues::I16(v) => v.len(),
            ExtraValues::U32(v) => v.len(),
            ExtraValues::I32(v) => v.len(),
            ExtraValues::U64(v) => v.len(),
            ExtraValues::I64(v) => v.len(),
            ExtraValues::F32(v) => v.len(),
            ExtraValues::F64(v) => v.len(),
        }
    }

    /// True if no element is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn kind(&self) -> ExtraKind {
        match self {
            ExtraValues::Raw(_) => ExtraKind::Raw,
            ExtraValues::U8(_) => ExtraKind::U8,
            ExtraValues::I8(_) => ExtraKind::I8,
            ExtraValues::U16(_) => ExtraKind::U16,
            ExtraValues::I16(_) => ExtraKind::I16,
            ExtraValues::U32(_) => ExtraKind::U32,
            ExtraValues::I32(_) => ExtraKind::I32,
            ExtraValues::U64(_) => ExtraKind::U64,
            ExtraValues::I64(_) => ExtraKind::I64,
            ExtraValues::F32(_) => ExtraKind::F32,
            ExtraValues::F64(_) => ExtraKind::F64,
        }
    }

    /// Appends the elements stored in `b` (whole elements only).
    fn push_from(&mut self, b: &[u8]) {
        match self {
            ExtraValues::Raw(v) | ExtraValues::U8(v) => v.extend_from_slice(b),
            ExtraValues::I8(v) => v.extend(b.iter().map(|&x| x as i8)),
            ExtraValues::U16(v) => v.extend(b.chunks_exact(2).map(|c| u16::from_le_bytes(arr(c, 0)))),
            ExtraValues::I16(v) => v.extend(b.chunks_exact(2).map(|c| i16::from_le_bytes(arr(c, 0)))),
            ExtraValues::U32(v) => v.extend(b.chunks_exact(4).map(|c| u32::from_le_bytes(arr(c, 0)))),
            ExtraValues::I32(v) => v.extend(b.chunks_exact(4).map(|c| i32::from_le_bytes(arr(c, 0)))),
            ExtraValues::U64(v) => v.extend(b.chunks_exact(8).map(|c| u64::from_le_bytes(arr(c, 0)))),
            ExtraValues::I64(v) => v.extend(b.chunks_exact(8).map(|c| i64::from_le_bytes(arr(c, 0)))),
            ExtraValues::F32(v) => v.extend(b.chunks_exact(4).map(|c| f32::from_bits(u32::from_le_bytes(arr(c, 0))))),
            ExtraValues::F64(v) => v.extend(b.chunks_exact(8).map(|c| f64::from_bits(u64::from_le_bytes(arr(c, 0))))),
        }
    }

    /// Writes elements `from..from + n` to `out`; false if out of range.
    fn write_range(&self, from: usize, n: usize, out: &mut Vec<u8>) -> bool {
        fn put<T: Copy, const N: usize>(
            v: &[T],
            from: usize,
            n: usize,
            out: &mut Vec<u8>,
            f: fn(T) -> [u8; N],
        ) -> bool {
            match from.checked_add(n).and_then(|end| v.get(from..end)) {
                Some(s) => {
                    for &x in s {
                        out.extend_from_slice(&f(x));
                    }
                    true
                }
                None => false,
            }
        }
        match self {
            ExtraValues::Raw(v) | ExtraValues::U8(v) => put(v, from, n, out, |x| [x]),
            ExtraValues::I8(v) => put(v, from, n, out, i8::to_le_bytes),
            ExtraValues::U16(v) => put(v, from, n, out, u16::to_le_bytes),
            ExtraValues::I16(v) => put(v, from, n, out, i16::to_le_bytes),
            ExtraValues::U32(v) => put(v, from, n, out, u32::to_le_bytes),
            ExtraValues::I32(v) => put(v, from, n, out, i32::to_le_bytes),
            ExtraValues::U64(v) => put(v, from, n, out, u64::to_le_bytes),
            ExtraValues::I64(v) => put(v, from, n, out, i64::to_le_bytes),
            ExtraValues::F32(v) => put(v, from, n, out, |x: f32| x.to_bits().to_le_bytes()),
            ExtraValues::F64(v) => put(v, from, n, out, |x: f64| x.to_bits().to_le_bytes()),
        }
    }
}

/// One extra-bytes attribute column.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraColumn {
    /// Its description.
    pub spec: ExtraSpec,
    /// Its values.
    pub values: ExtraValues,
}

/// Typed attribute columns of a batch. A column is `Some` exactly when the batch's format
/// has the field. Bit fields are stored right-aligned in a byte.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttributeColumns {
    /// Pulse return magnitude.
    pub intensity: Vec<u16>,
    /// Return number (3 bits in formats 0 to 5, 4 bits in 6 to 10).
    pub return_number: Vec<u8>,
    /// Number of returns of the pulse (3 or 4 bits).
    pub number_of_returns: Vec<u8>,
    /// Scan direction flag (1 bit).
    pub scan_direction: Vec<u8>,
    /// Edge of flight line flag (1 bit).
    pub edge_of_flight_line: Vec<u8>,
    /// Classification (5 bits in formats 0 to 5, 8 bits in 6 to 10).
    pub classification: Vec<u8>,
    /// Classification flags (3 bits in formats 0 to 5, 4 bits in 6 to 10), lowest bit first.
    pub class_flags: Vec<u8>,
    /// Scanner channel (2 bits; formats 6 to 10).
    pub scanner_channel: Option<Vec<u8>>,
    /// Scan angle rank (formats 0 to 5).
    pub scan_angle_rank: Option<Vec<i8>>,
    /// Scan angle in its stored integer units (formats 6 to 10).
    pub scan_angle: Option<Vec<i16>>,
    /// User data.
    pub user_data: Vec<u8>,
    /// Point source ID.
    pub point_source_id: Vec<u16>,
    /// GPS time (bit-exact).
    pub gps_time: Option<Vec<f64>>,
    /// Red, green, blue.
    pub rgb: Option<Vec<[u16; 3]>>,
    /// Near infrared.
    pub nir: Option<Vec<u16>>,
    /// The wave packet group of each point, kept as stored: waveform data is counted,
    /// not imported.
    pub wave_packet: Option<Vec<[u8; WAVE_PACKET_LEN]>>,
    /// Extra-bytes attributes, in record order.
    pub extra: Vec<ExtraColumn>,
}

/// A batch of points: integer coordinates and attribute columns (leaf Design).
#[derive(Debug, Clone, PartialEq)]
pub struct RawPointBatch {
    /// The record format.
    pub format: PointFormat,
    /// Integer x as stored.
    pub xi: Vec<i32>,
    /// Integer y as stored.
    pub yi: Vec<i32>,
    /// Integer z as stored.
    pub zi: Vec<i32>,
    /// Attributes.
    pub attrs: AttributeColumns,
}

impl RawPointBatch {
    /// An empty batch shaped for `format` and the extra attributes `extra`, with room for
    /// `capacity` points.
    pub fn with_capacity(format: PointFormat, extra: &[ExtraSpec], capacity: usize) -> RawPointBatch {
        fn col<T>(capacity: usize, present: bool) -> Option<Vec<T>> {
            present.then(|| Vec::with_capacity(capacity))
        }
        let ext = format.is_extended();
        RawPointBatch {
            format,
            xi: Vec::with_capacity(capacity),
            yi: Vec::with_capacity(capacity),
            zi: Vec::with_capacity(capacity),
            attrs: AttributeColumns {
                intensity: Vec::with_capacity(capacity),
                return_number: Vec::with_capacity(capacity),
                number_of_returns: Vec::with_capacity(capacity),
                scan_direction: Vec::with_capacity(capacity),
                edge_of_flight_line: Vec::with_capacity(capacity),
                classification: Vec::with_capacity(capacity),
                class_flags: Vec::with_capacity(capacity),
                scanner_channel: col(capacity, ext),
                scan_angle_rank: col(capacity, !ext),
                scan_angle: col(capacity, ext),
                user_data: Vec::with_capacity(capacity),
                point_source_id: Vec::with_capacity(capacity),
                gps_time: col(capacity, format.gps_at().is_some()),
                rgb: col(capacity, format.rgb_at().is_some()),
                nir: col(capacity, format.nir_at().is_some()),
                wave_packet: col(capacity, format.wave_at().is_some()),
                extra: extra
                    .iter()
                    .map(|spec| ExtraColumn { spec: spec.clone(), values: ExtraValues::empty(spec.kind) })
                    .collect(),
            },
        }
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        self.xi.len()
    }

    /// True if the batch holds no point.
    pub fn is_empty(&self) -> bool {
        self.xi.is_empty()
    }

    /// The attribute names present, in a fixed order; extra attributes are
    /// `extra:<name>:<type>[<count>]` with the name escaped for display.
    pub fn attribute_names(&self) -> Vec<String> {
        let a = &self.attrs;
        let mut names: Vec<String> = [
            "intensity",
            "return_number",
            "number_of_returns",
            "scan_direction",
            "edge_of_flight_line",
            "classification",
            "class_flags",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let optional = [
            ("scanner_channel", a.scanner_channel.is_some()),
            ("scan_angle_rank", a.scan_angle_rank.is_some()),
            ("scan_angle", a.scan_angle.is_some()),
            ("user_data", true),
            ("point_source_id", true),
            ("gps_time", a.gps_time.is_some()),
            ("rgb", a.rgb.is_some()),
            ("nir", a.nir.is_some()),
            ("wave_packet", a.wave_packet.is_some()),
        ];
        names.extend(optional.iter().filter(|(_, present)| *present).map(|(n, _)| n.to_string()));
        for c in &a.extra {
            names.push(format!("extra:{}:{}[{}]", c.spec.name.as_display(), c.spec.kind.as_str(), c.spec.count));
        }
        names
    }

    /// Number of points whose wave packet descriptor index is not zero.
    pub fn points_with_wave_packets(&self) -> u64 {
        match &self.attrs.wave_packet {
            Some(w) => w.iter().filter(|p| p.first().copied().unwrap_or(0) != 0).count() as u64,
            None => 0,
        }
    }

    /// Appends one record. `rec` holds at least the format's size plus the extra widths.
    pub(crate) fn push_record(&mut self, rec: &[u8]) {
        let f = self.format;
        let a = &mut self.attrs;
        self.xi.push(i32_at(rec, 0));
        self.yi.push(i32_at(rec, 4));
        self.zi.push(i32_at(rec, 8));
        a.intensity.push(u16_at(rec, 12));
        let b14 = u8_at(rec, 14);
        let b15 = u8_at(rec, 15);
        if f.is_extended() {
            a.return_number.push(b14 & 0x0F);
            a.number_of_returns.push(b14 >> 4);
            a.class_flags.push(b15 & 0x0F);
            if let Some(c) = a.scanner_channel.as_mut() {
                c.push((b15 >> 4) & 0x03);
            }
            a.scan_direction.push((b15 >> 6) & 1);
            a.edge_of_flight_line.push(b15 >> 7);
            a.classification.push(u8_at(rec, 16));
            a.user_data.push(u8_at(rec, 17));
            if let Some(c) = a.scan_angle.as_mut() {
                c.push(i16::from_le_bytes(arr(rec, 18)));
            }
            a.point_source_id.push(u16_at(rec, 20));
        } else {
            a.return_number.push(b14 & 0x07);
            a.number_of_returns.push((b14 >> 3) & 0x07);
            a.scan_direction.push((b14 >> 6) & 1);
            a.edge_of_flight_line.push(b14 >> 7);
            a.classification.push(b15 & 0x1F);
            a.class_flags.push(b15 >> 5);
            if let Some(c) = a.scan_angle_rank.as_mut() {
                c.push(u8_at(rec, 16) as i8);
            }
            a.user_data.push(u8_at(rec, 17));
            a.point_source_id.push(u16_at(rec, 18));
        }
        if let (Some(c), Some(at)) = (a.gps_time.as_mut(), f.gps_at()) {
            c.push(f64_at(rec, at));
        }
        if let (Some(c), Some(at)) = (a.rgb.as_mut(), f.rgb_at()) {
            c.push([u16_at(rec, at), u16_at(rec, at + 2), u16_at(rec, at + 4)]);
        }
        if let (Some(c), Some(at)) = (a.nir.as_mut(), f.nir_at()) {
            c.push(u16_at(rec, at));
        }
        if let (Some(c), Some(at)) = (a.wave_packet.as_mut(), f.wave_at()) {
            c.push(arr(rec, at));
        }
        let mut at = usize::from(f.base_len());
        for col in &mut a.extra {
            let w = col.spec.width();
            col.values.push_from(rec.get(at..at.saturating_add(w)).unwrap_or(&[]));
            at = at.saturating_add(w);
        }
    }

    /// Checks that every column matches the format and holds one entry per point.
    pub(crate) fn check_shape(&self) -> Result<(), LasError> {
        let n = self.len();
        let a = &self.attrs;
        let f = self.format;
        let bad = || LasError::malformed(LasField::Column);
        let lens = [
            self.yi.len(),
            self.zi.len(),
            a.intensity.len(),
            a.return_number.len(),
            a.number_of_returns.len(),
            a.scan_direction.len(),
            a.edge_of_flight_line.len(),
            a.classification.len(),
            a.class_flags.len(),
            a.user_data.len(),
            a.point_source_id.len(),
        ];
        if lens.iter().any(|&l| l != n) {
            return Err(bad());
        }
        fn opt<T>(c: &Option<Vec<T>>, want: bool, n: usize) -> bool {
            match c {
                Some(v) => want && v.len() == n,
                None => !want,
            }
        }
        let ext = f.is_extended();
        let ok = opt(&a.scanner_channel, ext, n)
            && opt(&a.scan_angle_rank, !ext, n)
            && opt(&a.scan_angle, ext, n)
            && opt(&a.gps_time, f.gps_at().is_some(), n)
            && opt(&a.rgb, f.rgb_at().is_some(), n)
            && opt(&a.nir, f.nir_at().is_some(), n)
            && opt(&a.wave_packet, f.wave_at().is_some(), n);
        if !ok {
            return Err(bad());
        }
        for c in &a.extra {
            if c.values.kind() != c.spec.kind || Some(c.values.len()) != n.checked_mul(c.spec.count) {
                return Err(bad());
            }
        }
        Ok(())
    }

    /// Total extra bytes per record the batch's extra columns describe.
    pub(crate) fn extra_width(&self) -> usize {
        self.attrs.extra.iter().fold(0usize, |acc, c| acc.saturating_add(c.spec.width()))
    }

    /// Encodes point `i` (shape already checked) onto `out`.
    pub(crate) fn encode_record(&self, i: usize, out: &mut Vec<u8>) -> Result<(), LasError> {
        let f = self.format;
        let a = &self.attrs;
        let col = LasError::malformed(LasField::Column);
        let range = LasError::malformed(LasField::ValueRange);
        let get = |v: &Vec<u8>| v.get(i).copied().ok_or(col.clone());
        let fits = |v: u8, bits: u32| if u32::from(v) >> bits == 0 { Ok(v) } else { Err(range.clone()) };
        let start = out.len();
        for v in [&self.xi, &self.yi, &self.zi] {
            out.extend_from_slice(&v.get(i).copied().ok_or(col.clone())?.to_le_bytes());
        }
        out.extend_from_slice(&a.intensity.get(i).copied().ok_or(col.clone())?.to_le_bytes());
        let rn = get(&a.return_number)?;
        let nr = get(&a.number_of_returns)?;
        let sd = fits(get(&a.scan_direction)?, 1)?;
        let ef = fits(get(&a.edge_of_flight_line)?, 1)?;
        let class = get(&a.classification)?;
        let flags = get(&a.class_flags)?;
        let user = get(&a.user_data)?;
        let psid = a.point_source_id.get(i).copied().ok_or(col.clone())?;
        if f.is_extended() {
            let ch = a.scanner_channel.as_ref().and_then(|c| c.get(i)).copied().ok_or(col.clone())?;
            let angle = a.scan_angle.as_ref().and_then(|c| c.get(i)).copied().ok_or(col.clone())?;
            out.push(fits(rn, 4)? | (fits(nr, 4)? << 4));
            out.push(fits(flags, 4)? | (fits(ch, 2)? << 4) | (sd << 6) | (ef << 7));
            out.push(class);
            out.push(user);
            out.extend_from_slice(&angle.to_le_bytes());
            out.extend_from_slice(&psid.to_le_bytes());
        } else {
            let rank = a.scan_angle_rank.as_ref().and_then(|c| c.get(i)).copied().ok_or(col.clone())?;
            out.push(fits(rn, 3)? | (fits(nr, 3)? << 3) | (sd << 6) | (ef << 7));
            out.push(fits(class, 5)? | (fits(flags, 3)? << 5));
            out.extend_from_slice(&rank.to_le_bytes());
            out.push(user);
            out.extend_from_slice(&psid.to_le_bytes());
        }
        // Optional groups in their record order: GPS time, RGB, NIR, wave packet; the
        // offsets in `PointFormat` agree with this order for every format.
        if f.gps_at().is_some() {
            let t = a.gps_time.as_ref().and_then(|c| c.get(i)).copied().ok_or(col.clone())?;
            out.extend_from_slice(&t.to_bits().to_le_bytes());
        }
        if f.rgb_at().is_some() {
            let rgb = a.rgb.as_ref().and_then(|c| c.get(i)).copied().ok_or(col.clone())?;
            for v in rgb {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        if f.nir_at().is_some() {
            let v = a.nir.as_ref().and_then(|c| c.get(i)).copied().ok_or(col.clone())?;
            out.extend_from_slice(&v.to_le_bytes());
        }
        if f.wave_at().is_some() {
            let w = a.wave_packet.as_ref().and_then(|c| c.get(i)).copied().ok_or(col.clone())?;
            out.extend_from_slice(&w);
        }
        if out.len() - start != usize::from(f.base_len()) {
            return Err(col);
        }
        for c in &a.extra {
            let from = i.checked_mul(c.spec.count).ok_or(col.clone())?;
            if !c.values.write_range(from, c.spec.count, out) {
                return Err(col);
            }
        }
        Ok(())
    }
}
