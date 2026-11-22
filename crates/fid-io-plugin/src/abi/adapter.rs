//! Adapters bridging [`FormatPluginReader`] and [`FormatPluginWriter`] into
//! `fid-io-reg`'s [`Registry`], [`Reader`], and [`Writer`] traits
//! (REQ-IO.REG.PLUGIN.ABI-004, REQ-IO.REG.PLUGIN.ABI-005, REQ-IO.REG.PLUGIN.ABI-006).

use crate::abi::{
    negotiate_version, AbiError, ContractVersion, DrawingPageRecord, FormatPluginReader, FormatPluginWriter, KvEntry,
    MeshRecord, PluginDescriptor, PluginError, PluginFormatSpec, Point3, PointsRecord, QifJsonRecord, ReadRequest,
    Record, TableRecord, WarningRecord, WriteEntity, WriteRequest,
};
use fid_canon::{to_canonical_bytes, CValue};
use fid_io_reg::env::export::{ByteSink, Writer};
use fid_io_reg::env::import::{ImportSource, Reader, StageSink};
use fid_io_reg::prov::LengthUnit;
use fid_io_reg::{
    Code, Direction, FormatDescriptor, FormatId, IoError, Matcher, Origin, Position, ReaderInfo, Registry,
    ReportBuilder, ReportPos, Signature, UntrustedText, VersionRange, VersionToken,
};
use std::collections::BTreeMap;
use std::io::{self, Read};

/// Element type ID staged for [`Record::Points`].
pub const ENTITY_TYPE_POINTS: &str = "plugin.points";
/// Element type ID staged for [`Record::Mesh`].
pub const ENTITY_TYPE_MESH: &str = "plugin.mesh";
/// Element type ID staged for [`Record::QifJson`].
pub const ENTITY_TYPE_QIF_JSON: &str = "plugin.qif_json";
/// Element type ID staged for [`Record::DrawingPage`].
pub const ENTITY_TYPE_DRAWING_PAGE: &str = "plugin.drawing_page";
/// Element type ID staged for [`Record::Table`].
pub const ENTITY_TYPE_TABLE: &str = "plugin.table";

/// Default signature priority for plugin-declared content signatures.
const DEFAULT_SIGNATURE_PRIORITY: u8 = 50;

/// Registers every format declared in `descriptor` into `registry` (`IO.REG.DETECT`)
/// under `origin` (`Origin::Plugin` for third-party plugins, or `Origin::FirstParty`
/// for contract readers shipped with the product; REQ-IO.REG.PLUGIN.ABI-005).
///
/// Validates the descriptor and performs contract version negotiation against
/// `host_version` before mutating `registry`, so a rejected descriptor leaves
/// `registry` unchanged.
pub fn register_descriptor(
    registry: &mut Registry,
    descriptor: &PluginDescriptor,
    origin: Origin,
    host_version: ContractVersion,
) -> Result<Vec<FormatDescriptor>, AbiError> {
    descriptor.validate()?;
    negotiate_version(host_version, descriptor.contract)?;

    let mut built = Vec::with_capacity(descriptor.formats.len());
    for spec in &descriptor.formats {
        let fd = to_format_descriptor(spec, origin)?;
        if registry.get(fd.id(), fd.direction()).is_some() {
            return Err(AbiError::DescriptorInvalid {
                field: format!("formats.{}", fd.id().as_str()),
                detail: "format already registered in registry".into(),
            });
        }
        built.push(fd);
    }

    for fd in &built {
        registry.register(fd.clone()).map_err(|e| AbiError::DescriptorInvalid {
            field: format!("formats.{}", fd.id().as_str()),
            detail: format!("registry rejected format: {e}"),
        })?;
    }

    Ok(built)
}

fn to_format_descriptor(spec: &PluginFormatSpec, origin: Origin) -> Result<FormatDescriptor, AbiError> {
    let versions =
        VersionRange::new(&spec.version_min, &spec.version_max).map_err(|e| AbiError::DescriptorInvalid {
            field: format!("formats.{}.versions", spec.format_id.as_str()),
            detail: format!("invalid version range: {e}"),
        })?;
    let mut signatures = Vec::with_capacity(spec.signatures.len());
    for sig in &spec.signatures {
        let s =
            Signature::new(sig.offset, Matcher::Bytes(sig.bytes.clone()), DEFAULT_SIGNATURE_PRIORITY).map_err(|e| {
                AbiError::DescriptorInvalid {
                    field: format!("formats.{}.signatures", spec.format_id.as_str()),
                    detail: format!("invalid signature: {e}"),
                }
            })?;
        signatures.push(s);
    }
    FormatDescriptor::new(
        spec.format_id.clone(),
        versions,
        signatures,
        spec.extensions.clone(),
        spec.options_schema.clone(),
        spec.phase,
        origin,
        spec.direction,
    )
    .map_err(|e| AbiError::DescriptorInvalid {
        field: format!("formats.{}", spec.format_id.as_str()),
        detail: format!("invalid format descriptor: {e}"),
    })
}

/// Adapts a [`FormatPluginReader`] into `fid-io-reg`'s [`Reader`] trait.
///
/// The adapter enforces contract version negotiation on construction, passes
/// only the bounded byte stream and validated options into `read` (REQ-006),
/// keeps every string returned by the plugin typed as [`UntrustedText`]
/// (REQ-004, CONSTITUTION C6), and stages each neutral [`Record`] into
/// [`StageSink`].
#[derive(Debug)]
pub struct PluginReaderAdapter<P> {
    plugin: P,
    format: FormatId,
    info: ReaderInfo,
    format_version: VersionToken,
    origin: Origin,
}

impl<P: FormatPluginReader> PluginReaderAdapter<P> {
    /// Builds an adapter for `format_id` declared by `plugin`, verifying
    /// descriptor validity and contract version compatibility against `host_version`.
    pub fn new(
        plugin: P,
        format_id: FormatId,
        origin: Origin,
        host_version: ContractVersion,
    ) -> Result<Self, AbiError> {
        let desc = plugin.descriptor();
        desc.validate()?;
        negotiate_version(host_version, desc.contract)?;

        let spec = desc
            .formats
            .iter()
            .find(|f| f.format_id == format_id && f.direction == Direction::Import)
            .ok_or_else(|| AbiError::DescriptorInvalid {
                field: "formats".into(),
                detail: format!("plugin '{}' does not declare import format '{}'", desc.plugin_id.as_str(), format_id),
            })?;

        let format_version = VersionToken::new(&spec.version_max)
            .or_else(|_| VersionToken::new(&spec.version_min))
            .or_else(|_| VersionToken::new("1"))
            .map_err(|_| AbiError::DescriptorInvalid {
                field: "formats.version_max".into(),
                detail: "unable to derive valid format version token".into(),
            })?;

        let info = ReaderInfo { id: desc.plugin_id.clone(), version: desc.version.clone() };
        Ok(Self { plugin, format: format_id, info, format_version, origin })
    }

    /// The origin this adapter was constructed with (`Origin::Plugin` or `Origin::FirstParty`).
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Reference to the wrapped plugin.
    pub fn plugin(&self) -> &P {
        &self.plugin
    }
}

/// Bridges [`ImportSource`] into `std::io::Read` while preserving any
/// `IoError` (such as `LimitExceeded` or cancellation) so a plugin cannot mask
/// harness limits.
struct BoundedStreamReader<'a, 's> {
    src: &'a mut ImportSource<'s>,
    err: Option<IoError>,
}

impl Read for BoundedStreamReader<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.err.is_some() {
            return Err(io::Error::other("bounded stream already failed"));
        }
        match self.src.read_bounded(buf) {
            Ok(n) => Ok(n),
            Err(e) => {
                self.err = Some(e);
                Err(io::Error::other("bounded stream limit or io error"))
            }
        }
    }
}

impl<P: FormatPluginReader> Reader for PluginReaderAdapter<P> {
    fn format(&self) -> &FormatId {
        &self.format
    }

    fn info(&self) -> &ReaderInfo {
        &self.info
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        self.plugin.validate_options(&self.format, given)
    }

    fn read(&self, opts: &CValue, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
        sink.declare_format_version(self.format_version.clone());
        sink.declare("plugin_id", UntrustedText::new(self.info.id.as_str()));
        sink.declare("plugin_origin", UntrustedText::new(self.origin.as_str()));

        let mut stream = BoundedStreamReader { src: sink.source(), err: None };
        let result = {
            let mut req = ReadRequest { format_id: &self.format, options: opts, stream: &mut stream };
            self.plugin.read(&mut req)
        };
        if let Some(source_err) = stream.err {
            return Err(source_err);
        }

        let records = result.map_err(plugin_err_to_io)?;
        for rec in &records {
            stage_record(rec, sink, report)?;
        }
        Ok(())
    }
}

/// Adapts a [`FormatPluginWriter`] into `fid-io-reg`'s [`Writer`] trait.
#[derive(Debug)]
pub struct PluginWriterAdapter<P> {
    plugin: P,
    format: FormatId,
    version: VersionToken,
}

impl<P: FormatPluginWriter> PluginWriterAdapter<P> {
    /// Builds a writer adapter for `format_id` declared by `plugin`, verifying
    /// descriptor validity and contract version compatibility against `host_version`.
    pub fn new(plugin: P, format_id: FormatId, host_version: ContractVersion) -> Result<Self, AbiError> {
        let desc = plugin.descriptor();
        desc.validate()?;
        negotiate_version(host_version, desc.contract)?;

        let has_export = desc.formats.iter().any(|f| f.format_id == format_id && f.direction == Direction::Export);
        if !has_export {
            return Err(AbiError::DescriptorInvalid {
                field: "formats".into(),
                detail: format!("plugin '{}' does not declare export format '{}'", desc.plugin_id.as_str(), format_id),
            });
        }

        let version = desc.version.clone();
        Ok(Self { plugin, format: format_id, version })
    }

    /// Reference to the wrapped plugin.
    pub fn plugin(&self) -> &P {
        &self.plugin
    }
}

impl<P: FormatPluginWriter> Writer for PluginWriterAdapter<P> {
    type Source = [WriteEntity];

    fn format(&self) -> &FormatId {
        &self.format
    }

    fn version(&self) -> &VersionToken {
        &self.version
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        self.plugin.validate_options(&self.format, given)
    }

    fn write(
        &self,
        src: &Self::Source,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        _report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        let req = WriteRequest { format_id: &self.format, options: opts, entities: src };
        let bytes = self.plugin.write(&req).map_err(plugin_err_to_io)?;
        sink.write_bytes(&bytes)
    }
}

// ---------------------------------------------------------------------------
// Record staging helpers

fn plugin_err_to_io(err: PluginError) -> IoError {
    let at = Position::byte(err.byte_offset.unwrap_or(0));
    if err.code == "truncated_file" {
        IoError::TruncatedFile { at }
    } else {
        IoError::Malformed { at, reason: "plugin reader error" }
    }
}

fn stage_record(rec: &Record, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
    match rec {
        Record::Points(r) => stage_points(r, sink, report),
        Record::Mesh(r) => stage_mesh(r, sink, report),
        Record::QifJson(r) => stage_qif_json(r, sink, report),
        Record::DrawingPage(r) => stage_drawing_page(r, sink, report),
        Record::Table(r) => stage_table(r, sink, report),
        Record::Warning(w) => stage_warning(w, report),
    }
}

fn declare_unit_if_known(unit: Option<&UntrustedText>, sink: &mut StageSink<'_, '_>) {
    if let Some(u) = unit {
        if let Some(lu) = LengthUnit::ALL.into_iter().find(|x| UntrustedText::new(x.symbol()) == *u) {
            sink.declare_unit(lu);
        }
    }
}

fn encode_point(p: Point3) -> Result<CValue, IoError> {
    if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() {
        return Err(IoError::Malformed { at: Position::byte(0), reason: "non-finite point coordinate" });
    }
    Ok(CValue::Arr(vec![CValue::F64Bits(p.x), CValue::F64Bits(p.y), CValue::F64Bits(p.z)]))
}

fn untrusted_cstr(t: &UntrustedText) -> CValue {
    CValue::Str(t.as_display().to_string())
}

fn encode_kv(entries: &[KvEntry]) -> CValue {
    let items = entries
        .iter()
        .map(|kv| {
            let mut m = BTreeMap::new();
            m.insert("key".into(), untrusted_cstr(&kv.key));
            m.insert("value".into(), untrusted_cstr(&kv.value));
            CValue::Obj(m)
        })
        .collect();
    CValue::Arr(items)
}

fn encode_payload(map: BTreeMap<String, CValue>) -> Result<Vec<u8>, IoError> {
    to_canonical_bytes(&CValue::Obj(map))
        .map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "record payload not canonicalizable" })
}

fn stage_points(r: &PointsRecord, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
    declare_unit_if_known(r.unit.as_ref(), sink);
    let mut pts = Vec::with_capacity(r.points.len());
    for &p in &r.points {
        pts.push(encode_point(p)?);
    }
    let mut m = BTreeMap::new();
    m.insert("attributes".into(), encode_kv(&r.attributes));
    m.insert("name".into(), untrusted_cstr(&r.name));
    m.insert("points".into(), CValue::Arr(pts));
    m.insert("unit".into(), r.unit.as_ref().map_or(CValue::Null, untrusted_cstr));
    let bytes = encode_payload(m)?;
    sink.add_entity(ENTITY_TYPE_POINTS, &r.name, &bytes, Position::byte(0), report)
}

fn stage_mesh(r: &MeshRecord, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
    declare_unit_if_known(r.unit.as_ref(), sink);
    let mut verts = Vec::with_capacity(r.vertices.len());
    for &v in &r.vertices {
        verts.push(encode_point(v)?);
    }
    let vlen = u32::try_from(r.vertices.len()).unwrap_or(u32::MAX);
    let mut tris = Vec::with_capacity(r.triangles.len());
    for t in &r.triangles {
        if t.a >= vlen || t.b >= vlen || t.c >= vlen {
            return Err(IoError::Malformed { at: Position::byte(0), reason: "mesh triangle index out of bounds" });
        }
        tris.push(CValue::Arr(vec![
            CValue::Num(f64::from(t.a)),
            CValue::Num(f64::from(t.b)),
            CValue::Num(f64::from(t.c)),
        ]));
    }
    let mut m = BTreeMap::new();
    m.insert("attributes".into(), encode_kv(&r.attributes));
    m.insert("name".into(), untrusted_cstr(&r.name));
    m.insert("triangles".into(), CValue::Arr(tris));
    m.insert("unit".into(), r.unit.as_ref().map_or(CValue::Null, untrusted_cstr));
    m.insert("vertices".into(), CValue::Arr(verts));
    let bytes = encode_payload(m)?;
    sink.add_entity(ENTITY_TYPE_MESH, &r.name, &bytes, Position::byte(0), report)
}

fn stage_qif_json(r: &QifJsonRecord, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
    let mut m = BTreeMap::new();
    m.insert("fragment_json".into(), untrusted_cstr(&r.fragment_json));
    m.insert("name".into(), untrusted_cstr(&r.name));
    let bytes = encode_payload(m)?;
    sink.add_entity(ENTITY_TYPE_QIF_JSON, &r.name, &bytes, Position::byte(0), report)
}

fn stage_drawing_page(
    r: &DrawingPageRecord,
    sink: &mut StageSink<'_, '_>,
    report: &mut ReportBuilder,
) -> Result<(), IoError> {
    if !r.width_mm.is_finite() || !r.height_mm.is_finite() || r.width_mm <= 0.0 || r.height_mm <= 0.0 {
        return Err(IoError::Malformed { at: Position::byte(0), reason: "invalid drawing page dimensions" });
    }
    let mut hex = String::with_capacity(r.image_bytes.len() * 2);
    for b in &r.image_bytes {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
    }
    let mut m = BTreeMap::new();
    m.insert("dpi".into(), CValue::Num(f64::from(r.dpi)));
    m.insert("height_mm".into(), CValue::F64Bits(r.height_mm));
    m.insert("image_hex".into(), CValue::Str(hex));
    m.insert("media_type".into(), untrusted_cstr(&r.media_type));
    m.insert("metadata".into(), encode_kv(&r.metadata));
    m.insert("name".into(), untrusted_cstr(&r.name));
    m.insert("page_index".into(), CValue::Num(f64::from(r.page_index)));
    m.insert("width_mm".into(), CValue::F64Bits(r.width_mm));
    let bytes = encode_payload(m)?;
    sink.add_entity(ENTITY_TYPE_DRAWING_PAGE, &r.name, &bytes, Position::byte(0), report)
}

fn stage_table(r: &TableRecord, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
    let cols = r.columns.iter().map(untrusted_cstr).collect();
    let rows = r.rows.iter().map(|row| CValue::Arr(row.iter().map(untrusted_cstr).collect())).collect();
    let mut m = BTreeMap::new();
    m.insert("columns".into(), CValue::Arr(cols));
    m.insert("name".into(), untrusted_cstr(&r.name));
    m.insert("rows".into(), CValue::Arr(rows));
    let bytes = encode_payload(m)?;
    sink.add_entity(ENTITY_TYPE_TABLE, &r.name, &bytes, Position::byte(0), report)
}

fn stage_warning(w: &WarningRecord, report: &mut ReportBuilder) -> Result<(), IoError> {
    let code_text = w.code.as_display().to_string();
    let code = Code::new(&code_text)
        .or_else(|_| Code::new("plugin_warning"))
        .map_err(|_| IoError::Malformed { at: Position::byte(0), reason: "invalid fallback warning code" })?;
    let pos = w.byte_offset.map(ReportPos::byte);
    report.warn(&code, pos, &w.detail);
    Ok(())
}
