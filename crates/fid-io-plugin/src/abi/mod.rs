//! Format-plugin ABI types, version negotiation, descriptor validation,
//! IDL artifact freshness/capability audit ([`codegen`]), and adapters to
//! `fid-io-reg`'s [`fid_io_reg::env::import::Reader`] and
//! [`fid_io_reg::env::export::Writer`] traits ([`adapter`]).

pub mod adapter;
pub mod codegen;

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_io_reg::{Direction, FormatId, Phase, ReaderId, UntrustedText, VersionRange, VersionToken};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;

/// The host's current format-plugin contract version (major 1, minor 0).
pub const HOST_CONTRACT_VERSION: ContractVersion = ContractVersion { major: 1, minor: 0 };

/// Largest descriptor JSON document [`PluginDescriptor::from_json`] accepts, in bytes.
pub const MAX_DESCRIPTOR_BYTES: usize = 256 * 1024;

/// Maximum number of format declarations in one [`PluginDescriptor`].
pub const MAX_FORMATS_PER_PLUGIN: usize = 64;

/// Maximum length of a single byte signature in a descriptor, in bytes.
pub const MAX_SIGNATURE_BYTES: usize = 64;

/// Major/minor contract version declared by the host or a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContractVersion {
    /// Breaking contract version. Must equal the host's major version.
    pub major: u32,
    /// Additive minor contract version. Host accepts `plugin.minor <= host.minor`.
    pub minor: u32,
}

impl ContractVersion {
    /// Constructs a contract version pair.
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

impl fmt::Display for ContractVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Errors returned by the format-plugin ABI layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiError {
    /// Plugin contract version is incompatible with the host's contract version
    /// (REQ-IO.REG.PLUGIN.ABI-002, REQ-IO.REG.PLUGIN.ABI-003).
    AbiMismatch { host: ContractVersion, plugin: ContractVersion },
    /// A committed generated ABI artifact (WIT or message schema) differs from
    /// what the single IDL source generates (REQ-IO.REG.PLUGIN.ABI-001).
    GeneratedOutOfDate { artifact: &'static str, detail: String },
    /// A [`PluginDescriptor`] failed schema or semantic validation.
    DescriptorInvalid { field: String, detail: String },
    /// The contract source or generated interface declares a forbidden
    /// capability (file system, network/socket, clock, or document)
    /// (REQ-IO.REG.PLUGIN.ABI-006).
    CapabilityViolation { detail: String },
}

impl AbiError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            AbiError::AbiMismatch { .. } => "plugin_abi_mismatch",
            AbiError::GeneratedOutOfDate { .. } => "abi_generated_out_of_date",
            AbiError::DescriptorInvalid { .. } => "plugin_descriptor_invalid",
            AbiError::CapabilityViolation { .. } => "capability_violation",
        }
    }
}

impl fmt::Display for AbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbiError::AbiMismatch { host, plugin } => {
                write!(f, "plugin_abi_mismatch: host contract {host} is incompatible with plugin contract {plugin}")
            }
            AbiError::GeneratedOutOfDate { artifact, detail } => {
                write!(f, "abi_generated_out_of_date ({artifact}): {detail}")
            }
            AbiError::DescriptorInvalid { field, detail } => {
                write!(f, "plugin_descriptor_invalid ({field}): {detail}")
            }
            AbiError::CapabilityViolation { detail } => {
                write!(f, "capability_violation: {detail}")
            }
        }
    }
}

impl std::error::Error for AbiError {}

/// Checks whether `host` can load a plugin declaring `plugin` contract version.
///
/// - Different major version -> `Err(AbiError::AbiMismatch)` naming both versions (REQ-002).
/// - Same major version and `plugin.minor <= host.minor` -> `Ok(())` (REQ-003).
/// - Same major version and `plugin.minor > host.minor` -> `Err(AbiError::AbiMismatch)`
///   naming both versions (REQ-003).
pub fn negotiate_version(host: ContractVersion, plugin: ContractVersion) -> Result<(), AbiError> {
    if plugin.major != host.major || plugin.minor > host.minor {
        return Err(AbiError::AbiMismatch { host, plugin });
    }
    Ok(())
}

/// Resource limits requested by a plugin in its descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestedCaps {
    /// Requested memory ceiling in bytes.
    pub max_memory_bytes: u64,
    /// Requested wall-clock ceiling in milliseconds.
    pub max_wall_ms: u64,
}

/// Content magic byte signature declared by a plugin format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteSignature {
    /// Byte offset from the start of the file.
    pub offset: u32,
    /// Exact byte sequence expected at `offset`.
    pub bytes: Vec<u8>,
}

/// One format supported by a plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginFormatSpec {
    /// Registered format ID (e.g. `toy-plugin`).
    pub format_id: FormatId,
    /// Import or export direction.
    pub direction: Direction,
    /// Lowest format version handled.
    pub version_min: String,
    /// Highest format version handled.
    pub version_max: String,
    /// Byte-level content signatures for `IO.REG.DETECT`.
    pub signatures: Vec<ByteSignature>,
    /// File extensions (lowercase, without leading dot).
    pub extensions: Vec<String>,
    /// JSON Schema object for the format's options.
    pub options_schema: CValue,
    /// Delivery phase tag for `IO.REG.DETECT`.
    pub phase: Phase,
}

/// Self-description returned by a plugin's `descriptor` entry point.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginDescriptor {
    /// Stable plugin identifier.
    pub plugin_id: ReaderId,
    /// Plugin implementation version token.
    pub version: VersionToken,
    /// Format-plugin contract version implemented by this plugin.
    pub contract: ContractVersion,
    /// Formats declared by this plugin.
    pub formats: Vec<PluginFormatSpec>,
    /// Requested memory and wall-time caps.
    pub requested_caps: RequestedCaps,
}

impl PluginDescriptor {
    /// Validates every field of this descriptor.
    pub fn validate(&self) -> Result<(), AbiError> {
        if self.requested_caps.max_memory_bytes == 0 {
            return Err(invalid("requested_caps.max_memory_bytes", "must be greater than zero"));
        }
        if self.requested_caps.max_wall_ms == 0 {
            return Err(invalid("requested_caps.max_wall_ms", "must be greater than zero"));
        }
        if self.formats.is_empty() {
            return Err(invalid("formats", "must declare at least one format"));
        }
        if self.formats.len() > MAX_FORMATS_PER_PLUGIN {
            return Err(invalid("formats", "exceeds maximum formats per plugin"));
        }
        let mut seen = BTreeSet::new();
        for (idx, spec) in self.formats.iter().enumerate() {
            let dir_key = match spec.direction {
                Direction::Import => "import",
                Direction::Export => "export",
            };
            if !seen.insert((spec.format_id.as_str().to_string(), dir_key)) {
                return Err(invalid(format!("formats[{idx}]"), "duplicate (format_id, direction) entry"));
            }
            if VersionRange::new(&spec.version_min, &spec.version_max).is_err() {
                return Err(invalid(format!("formats[{idx}].versions"), "invalid version range"));
            }
            if !matches!(spec.options_schema, CValue::Obj(_)) {
                return Err(invalid(format!("formats[{idx}].options_schema"), "options_schema must be a JSON object"));
            }
            for (sidx, sig) in spec.signatures.iter().enumerate() {
                if sig.bytes.is_empty() || sig.bytes.len() > MAX_SIGNATURE_BYTES {
                    return Err(invalid(
                        format!("formats[{idx}].signatures[{sidx}]"),
                        "signature bytes must be 1..=64 bytes",
                    ));
                }
            }
            for (eidx, ext) in spec.extensions.iter().enumerate() {
                if ext.is_empty()
                    || ext.starts_with('.')
                    || !ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                {
                    return Err(invalid(
                        format!("formats[{idx}].extensions[{eidx}]"),
                        "extension must be non-empty lowercase ASCII alphanumeric without leading dot",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Parses and validates a [`PluginDescriptor`] from JSON bytes.
    pub fn from_json(bytes: &[u8]) -> Result<Self, AbiError> {
        if bytes.len() > MAX_DESCRIPTOR_BYTES {
            return Err(invalid("descriptor", "exceeds MAX_DESCRIPTOR_BYTES"));
        }
        let val = parse(bytes).map_err(|e| invalid("descriptor", format!("invalid JSON: {e}")))?;
        Self::from_value(&val)
    }

    /// Parses and validates a [`PluginDescriptor`] from a [`CValue`].
    pub fn from_value(val: &CValue) -> Result<Self, AbiError> {
        let obj = as_obj(val, "descriptor")?;
        let plugin_id_str = req_str(obj, "plugin_id")?;
        let plugin_id =
            ReaderId::new(plugin_id_str).map_err(|_| invalid("plugin_id", "invalid plugin/reader identifier"))?;
        let version_str = req_str(obj, "version")?;
        let version = VersionToken::new(version_str).map_err(|_| invalid("version", "invalid version token"))?;

        let contract_obj = as_obj(req_field(obj, "contract")?, "contract")?;
        let major = req_u32(contract_obj, "major", "contract.major")?;
        let minor = req_u32(contract_obj, "minor", "contract.minor")?;
        let contract = ContractVersion { major, minor };

        let caps_obj = as_obj(req_field(obj, "requested_caps")?, "requested_caps")?;
        let max_memory_bytes = req_u64(caps_obj, "max_memory_bytes", "requested_caps.max_memory_bytes")?;
        let max_wall_ms = req_u64(caps_obj, "max_wall_ms", "requested_caps.max_wall_ms")?;
        let requested_caps = RequestedCaps { max_memory_bytes, max_wall_ms };

        let formats_arr = as_arr(req_field(obj, "formats")?, "formats")?;
        if formats_arr.len() > MAX_FORMATS_PER_PLUGIN {
            return Err(invalid("formats", "exceeds maximum formats per plugin"));
        }
        let mut formats = Vec::with_capacity(formats_arr.len());
        for (idx, item) in formats_arr.iter().enumerate() {
            let fpath = format!("formats[{idx}]");
            let fobj = as_obj(item, &fpath)?;
            let format_id = FormatId::new(req_str(fobj, "format_id")?)
                .map_err(|_| invalid(format!("{fpath}.format_id"), "invalid format_id"))?;
            let direction = match req_str(fobj, "direction")? {
                "import" => Direction::Import,
                "export" => Direction::Export,
                _ => return Err(invalid(format!("{fpath}.direction"), "expected 'import' or 'export'")),
            };
            let version_min = req_str(fobj, "version_min")?.to_string();
            let version_max = req_str(fobj, "version_max")?.to_string();

            let sigs_arr = as_arr(req_field(fobj, "signatures")?, &format!("{fpath}.signatures"))?;
            let mut signatures = Vec::with_capacity(sigs_arr.len());
            for (sidx, sitem) in sigs_arr.iter().enumerate() {
                let spath = format!("{fpath}.signatures[{sidx}]");
                let sobj = as_obj(sitem, &spath)?;
                let offset = req_u32(sobj, "offset", &format!("{spath}.offset"))?;
                let hex_or_bytes = req_field(sobj, "bytes")?;
                let bytes = parse_sig_bytes(hex_or_bytes, &format!("{spath}.bytes"))?;
                signatures.push(ByteSignature { offset, bytes });
            }

            let exts_arr = as_arr(req_field(fobj, "extensions")?, &format!("{fpath}.extensions"))?;
            let mut extensions = Vec::with_capacity(exts_arr.len());
            for (eidx, eitem) in exts_arr.iter().enumerate() {
                match eitem {
                    CValue::Str(s) => extensions.push(s.clone()),
                    _ => return Err(invalid(format!("{fpath}.extensions[{eidx}]"), "expected string")),
                }
            }

            let options_schema = req_field(fobj, "options_schema")?.clone();
            let phase_num = req_u32(fobj, "phase", &format!("{fpath}.phase"))?;
            let phase_u8 =
                u8::try_from(phase_num).map_err(|_| invalid(format!("{fpath}.phase"), "phase out of range"))?;
            let phase = Phase::new(phase_u8).ok_or_else(|| invalid(format!("{fpath}.phase"), "invalid phase"))?;

            formats.push(PluginFormatSpec {
                format_id,
                direction,
                version_min,
                version_max,
                signatures,
                extensions,
                options_schema,
                phase,
            });
        }

        let desc = Self { plugin_id, version, contract, formats, requested_caps };
        desc.validate()?;
        Ok(desc)
    }

    /// Converts this descriptor into a canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        let mut contract = BTreeMap::new();
        contract.insert("major".into(), CValue::Num(f64::from(self.contract.major)));
        contract.insert("minor".into(), CValue::Num(f64::from(self.contract.minor)));

        let mut caps = BTreeMap::new();
        caps.insert("max_memory_bytes".into(), CValue::U64(self.requested_caps.max_memory_bytes));
        caps.insert("max_wall_ms".into(), CValue::U64(self.requested_caps.max_wall_ms));

        let formats = self
            .formats
            .iter()
            .map(|f| {
                let mut m = BTreeMap::new();
                m.insert("format_id".into(), CValue::Str(f.format_id.as_str().to_string()));
                m.insert(
                    "direction".into(),
                    CValue::Str(match f.direction {
                        Direction::Import => "import".into(),
                        Direction::Export => "export".into(),
                    }),
                );
                m.insert("version_min".into(), CValue::Str(f.version_min.clone()));
                m.insert("version_max".into(), CValue::Str(f.version_max.clone()));
                m.insert(
                    "signatures".into(),
                    CValue::Arr(
                        f.signatures
                            .iter()
                            .map(|s| {
                                let mut sm = BTreeMap::new();
                                sm.insert("offset".into(), CValue::Num(f64::from(s.offset)));
                                sm.insert(
                                    "bytes".into(),
                                    CValue::Arr(s.bytes.iter().map(|b| CValue::Num(f64::from(*b))).collect()),
                                );
                                CValue::Obj(sm)
                            })
                            .collect(),
                    ),
                );
                m.insert(
                    "extensions".into(),
                    CValue::Arr(f.extensions.iter().map(|e| CValue::Str(e.clone())).collect()),
                );
                m.insert("options_schema".into(), f.options_schema.clone());
                m.insert("phase".into(), CValue::Num(f64::from(f.phase.number())));
                CValue::Obj(m)
            })
            .collect();

        let mut root = BTreeMap::new();
        root.insert("contract".into(), CValue::Obj(contract));
        root.insert("formats".into(), CValue::Arr(formats));
        root.insert("plugin_id".into(), CValue::Str(self.plugin_id.as_str().to_string()));
        root.insert("requested_caps".into(), CValue::Obj(caps));
        root.insert("version".into(), CValue::Str(self.version.as_str().to_string()));
        CValue::Obj(root)
    }

    /// Serializes this descriptor as RFC 8785 canonical JSON bytes.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, AbiError> {
        self.validate()?;
        to_canonical_bytes(&self.to_value()).map_err(|e| invalid("descriptor", format!("canonicalization failed: {e}")))
    }
}

/// A 3D point coordinate in the neutral record model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Triangle vertex indices into a [`MeshRecord`]'s `vertices` array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriIndices {
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

/// Untrusted key/value metadata entry returned by a plugin (CONSTITUTION C6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvEntry {
    pub key: UntrustedText,
    pub value: UntrustedText,
}

impl KvEntry {
    /// Constructs a [`KvEntry`] wrapping raw plugin strings as [`UntrustedText`].
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: UntrustedText::from_string(key.into()), value: UntrustedText::from_string(value.into()) }
    }
}

/// Point-set neutral record emitted by a plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct PointsRecord {
    pub name: UntrustedText,
    pub points: Vec<Point3>,
    pub unit: Option<UntrustedText>,
    pub attributes: Vec<KvEntry>,
}

/// Triangle-mesh neutral record emitted by a plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshRecord {
    pub name: UntrustedText,
    pub vertices: Vec<Point3>,
    pub triangles: Vec<TriIndices>,
    pub unit: Option<UntrustedText>,
    pub attributes: Vec<KvEntry>,
}

/// Lossless QIF-JSON fragment neutral record emitted by a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QifJsonRecord {
    pub name: UntrustedText,
    pub fragment_json: UntrustedText,
}

/// Drawing page neutral record emitted by a plugin (used by `IO.REG.PLUGIN.DRAW`).
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingPageRecord {
    pub name: UntrustedText,
    pub page_index: u32,
    pub width_mm: f64,
    pub height_mm: f64,
    pub dpi: u32,
    pub media_type: UntrustedText,
    pub image_bytes: Vec<u8>,
    pub metadata: Vec<KvEntry>,
}

/// Tabular data neutral record emitted by a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRecord {
    pub name: UntrustedText,
    pub columns: Vec<UntrustedText>,
    pub rows: Vec<Vec<UntrustedText>>,
}

/// Non-fatal diagnostic warning emitted by a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarningRecord {
    pub code: UntrustedText,
    pub detail: UntrustedText,
    pub byte_offset: Option<u64>,
}

/// Neutral record stream item returned by a format plugin's `read` entry point.
///
/// Every string contained in every variant is typed as [`UntrustedText`]
/// (CONSTITUTION C6, REQ-IO.REG.PLUGIN.ABI-004).
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Points(PointsRecord),
    Mesh(MeshRecord),
    QifJson(QifJsonRecord),
    DrawingPage(DrawingPageRecord),
    Table(TableRecord),
    Warning(WarningRecord),
}

impl Record {
    /// Collects references to every string carried by this record, all typed as
    /// [`UntrustedText`].
    pub fn untrusted_strings(&self) -> Vec<&UntrustedText> {
        let mut out = Vec::new();
        match self {
            Record::Points(r) => {
                out.push(&r.name);
                if let Some(u) = &r.unit {
                    out.push(u);
                }
                for kv in &r.attributes {
                    out.push(&kv.key);
                    out.push(&kv.value);
                }
            }
            Record::Mesh(r) => {
                out.push(&r.name);
                if let Some(u) = &r.unit {
                    out.push(u);
                }
                for kv in &r.attributes {
                    out.push(&kv.key);
                    out.push(&kv.value);
                }
            }
            Record::QifJson(r) => {
                out.push(&r.name);
                out.push(&r.fragment_json);
            }
            Record::DrawingPage(r) => {
                out.push(&r.name);
                out.push(&r.media_type);
                for kv in &r.metadata {
                    out.push(&kv.key);
                    out.push(&kv.value);
                }
            }
            Record::Table(r) => {
                out.push(&r.name);
                for c in &r.columns {
                    out.push(c);
                }
                for row in &r.rows {
                    for cell in row {
                        out.push(cell);
                    }
                }
            }
            Record::Warning(w) => {
                out.push(&w.code);
                out.push(&w.detail);
            }
        }
        out
    }
}

/// Input to a format plugin's `read` entry point (REQ-IO.REG.PLUGIN.ABI-006).
///
/// Carries only the format ID, validated options, and a bounded byte stream —
/// no file system, network, clock, or document handle.
pub struct ReadRequest<'a> {
    pub format_id: &'a FormatId,
    pub options: &'a CValue,
    pub stream: &'a mut dyn Read,
}

impl fmt::Debug for ReadRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadRequest")
            .field("format_id", &self.format_id)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// One neutral entity passed to a format plugin's `write` entry point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteEntity {
    pub type_id: String,
    pub name: UntrustedText,
    pub payload: Vec<u8>,
}

/// Input to a format plugin's `write` entry point.
#[derive(Debug)]
pub struct WriteRequest<'a> {
    pub format_id: &'a FormatId,
    pub options: &'a CValue,
    pub entities: &'a [WriteEntity],
}

/// Error returned by a format plugin's `read` or `write` entry point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginError {
    pub code: String,
    pub detail: UntrustedText,
    pub byte_offset: Option<u64>,
}

impl PluginError {
    /// Creates a malformed-input error with an untrusted detail string.
    pub fn malformed(detail: impl Into<String>, byte_offset: Option<u64>) -> Self {
        Self { code: "malformed".into(), detail: UntrustedText::from_string(detail.into()), byte_offset }
    }

    /// Creates a truncated-file error with an untrusted detail string.
    pub fn truncated(detail: impl Into<String>, byte_offset: Option<u64>) -> Self {
        Self { code: "truncated_file".into(), detail: UntrustedText::from_string(detail.into()), byte_offset }
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.byte_offset {
            Some(at) => write!(f, "{} at byte {at}: {}", self.code, self.detail.as_display()),
            None => write!(f, "{}: {}", self.code, self.detail.as_display()),
        }
    }
}

impl std::error::Error for PluginError {}

/// Contract implemented by every in-process, first-party, WASM-bridged or
/// native-bridged format-plugin reader.
pub trait FormatPluginReader {
    /// Returns the plugin's descriptor.
    fn descriptor(&self) -> &PluginDescriptor;

    /// Validates caller options for `format_id` and returns effective options.
    fn validate_options(&self, _format_id: &FormatId, given: &CValue) -> Result<CValue, &'static str> {
        match given {
            CValue::Obj(_) => Ok(given.clone()),
            _ => Err("options must be an object"),
        }
    }

    /// Reads untrusted bytes from `req.stream` using validated `req.options`
    /// and returns a deterministic list of neutral [`Record`]s.
    fn read(&self, req: &mut ReadRequest<'_>) -> Result<Vec<Record>, PluginError>;
}

/// Contract implemented by every in-process, first-party, WASM-bridged or
/// native-bridged format-plugin writer.
pub trait FormatPluginWriter {
    /// Returns the plugin's descriptor.
    fn descriptor(&self) -> &PluginDescriptor;

    /// Validates caller options for `format_id` and returns effective options.
    fn validate_options(&self, _format_id: &FormatId, given: &CValue) -> Result<CValue, &'static str> {
        match given {
            CValue::Obj(_) => Ok(given.clone()),
            _ => Err("options must be an object"),
        }
    }

    /// Serializes `req.entities` using validated `req.options` into output bytes.
    fn write(&self, req: &WriteRequest<'_>) -> Result<Vec<u8>, PluginError>;
}

// ---------------------------------------------------------------------------
// Internal JSON parsing helpers

fn invalid(field: impl Into<String>, detail: impl Into<String>) -> AbiError {
    AbiError::DescriptorInvalid { field: field.into(), detail: detail.into() }
}

fn as_obj<'a>(v: &'a CValue, field: &str) -> Result<&'a BTreeMap<String, CValue>, AbiError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(invalid(field, "expected JSON object")),
    }
}

fn as_arr<'a>(v: &'a CValue, field: &str) -> Result<&'a [CValue], AbiError> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => Err(invalid(field, "expected JSON array")),
    }
}

fn req_field<'a>(obj: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a CValue, AbiError> {
    obj.get(key).ok_or_else(|| invalid(key, "missing required field"))
}

fn req_str<'a>(obj: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, AbiError> {
    match req_field(obj, key)? {
        CValue::Str(s) => Ok(s.as_str()),
        _ => Err(invalid(key, "expected string")),
    }
}

fn req_u32(obj: &BTreeMap<String, CValue>, key: &str, path: &str) -> Result<u32, AbiError> {
    let n = req_u64(obj, key, path)?;
    u32::try_from(n).map_err(|_| invalid(path, "value out of u32 range"))
}

fn req_u64(obj: &BTreeMap<String, CValue>, key: &str, path: &str) -> Result<u64, AbiError> {
    match req_field(obj, key)? {
        CValue::U64(n) => Ok(*n),
        CValue::I64(n) => u64::try_from(*n).map_err(|_| invalid(path, "expected non-negative integer")),
        CValue::Num(f) => {
            if !f.is_finite() || *f < 0.0 || f.fract() != 0.0 || *f > 9_007_199_254_740_991.0 {
                return Err(invalid(path, "expected non-negative safe integer"));
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Ok(*f as u64)
        }
        CValue::Str(s) => s.parse::<u64>().map_err(|_| invalid(path, "expected unsigned integer")),
        _ => Err(invalid(path, "expected unsigned integer")),
    }
}

fn parse_sig_bytes(val: &CValue, path: &str) -> Result<Vec<u8>, AbiError> {
    match val {
        CValue::Arr(items) => {
            if items.is_empty() || items.len() > MAX_SIGNATURE_BYTES {
                return Err(invalid(path, "signature bytes must be 1..=64 bytes"));
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let b = match item {
                    CValue::Num(f) if f.is_finite() && *f >= 0.0 && *f <= 255.0 && f.fract() == 0.0 =>
                    {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        (*f as u8)
                    }
                    CValue::U64(n) => u8::try_from(*n).map_err(|_| invalid(path, "byte out of 0..=255"))?,
                    _ => return Err(invalid(path, "signature byte must be integer 0..=255")),
                };
                out.push(b);
            }
            Ok(out)
        }
        CValue::Str(s) => {
            if s.is_empty() || s.len() > MAX_SIGNATURE_BYTES {
                return Err(invalid(path, "signature string must be 1..=64 bytes"));
            }
            Ok(s.as_bytes().to_vec())
        }
        _ => Err(invalid(path, "expected byte array or ASCII string")),
    }
}
