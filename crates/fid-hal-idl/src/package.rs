//! Capability-package description format (`capability-package/v1`), validator and canonical
//! digest (spec node `HAL.IDL.GEN.FMT`).
//!
//! Every capability package in `fid-hal-idl` implements [`CapabilityPackage`] to emit a
//! language-neutral [`PackageDescription`]. Generators, discovery commands and conformance
//! suites read this description rather than Rust source.
//!
//! - **REQ-HAL.IDL.GEN.FMT-001:** Holds `manifest`, `operations`, `batch_schemas`, `semantics`
//!   and `profile` (plus optional internal `$defs`).
//! - **REQ-HAL.IDL.GEN.FMT-002:** Validation fails with `package_invalid` carrying an RFC 6901
//!   JSON pointer on duplicate operation names, unknown CONSTITUTION §3 side-effect classes,
//!   unknown `Device@1` error codes, missing/invalid data-plane column `EncodingDescriptor`s,
//!   or non-SemVer versions.
//! - **REQ-HAL.IDL.GEN.FMT-003:** [`PackageDescription::digest`] is SHA-256 over RFC 8785
//!   canonical JSON with `operations` sorted by `name`, `batch_schemas` sorted by `id`,
//!   `profile.items` sorted by `id`, `errors` sorted by wire code and `live_settings` sorted
//!   lexicographically (D0).
//! - **REQ-HAL.IDL.GEN.FMT-004:** Embedded type schemas are JSON Schema 2020-12 fragments
//!   restricted strictly to the allowlist: `type`, `properties`, `required`, `items`, `enum`,
//!   `minimum`, `maximum`, `maxLength`, internal `$ref` (`#/$defs/<name>`), and
//!   `additionalProperties: false`.
//! - **REQ-HAL.IDL.GEN.FMT-005:** No executable content and no external references other than
//!   digests of semantics notes and fixtures.

use crate::base::error::ErrorCode;
use crate::units::{EncodingDescriptor, SiUnit};
use fid_canon::CValue;
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Format tag of a `capability-package/v1` description.
pub const PACKAGE_SCHEMA_TAG: &str = "capability-package/v1";

/// Largest accepted package description JSON document (4 MiB).
pub const MAX_PACKAGE_BYTES: usize = 4 * 1024 * 1024;

/// Deepest accepted JSON nesting in a package description.
pub const MAX_PACKAGE_DEPTH: usize = 64;

/// Largest accepted semantics Markdown note (256 KiB).
pub const MAX_SEMANTICS_BYTES: usize = 256 * 1024;

/// Closed allowlist of JSON Schema 2020-12 keywords accepted inside type schemas (REQ-004).
pub const ALLOWED_SCHEMA_KEYWORDS: [&str; 10] = [
    "$ref",
    "additionalProperties",
    "enum",
    "items",
    "maxLength",
    "maximum",
    "minimum",
    "properties",
    "required",
    "type",
];

/// Closed allowlist of JSON Schema `"type"` scalar/container values.
pub const ALLOWED_SCHEMA_TYPES: [&str; 7] = ["array", "boolean", "integer", "null", "number", "object", "string"];

// ------------------------------------------------------------------ error type

/// Validation or parsing failure of a `capability-package/v1` description (`package_invalid`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageError {
    /// RFC 6901 JSON pointer to the offending node (e.g. `"/operations/1/name"`).
    pub pointer: String,
    /// Human-readable reason naming the violated rule or foreign keyword.
    pub reason: String,
}

impl PackageError {
    /// Creates a `package_invalid` error at `pointer`.
    pub fn invalid(pointer: impl Into<String>, reason: impl Into<String>) -> PackageError {
        PackageError { pointer: pointer.into(), reason: reason.into() }
    }

    /// Stable machine-readable error code (`"package_invalid"`).
    pub const fn code(&self) -> &'static str {
        "package_invalid"
    }
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "package_invalid{{{}}}: {}", self.pointer, self.reason)
    }
}

impl std::error::Error for PackageError {}

// ------------------------------------------------------------------ data structures

/// Manifest identifying one capability package and semantic version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub deprecated: bool,
    pub tier3_usable: bool,
}

/// One operation exposed by a capability package.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationDesc {
    pub name: String,
    pub class: SideEffect,
    /// JSON Schema 2020-12 fragment restricted to [`ALLOWED_SCHEMA_KEYWORDS`].
    pub request: CValue,
    /// JSON Schema 2020-12 fragment restricted to [`ALLOWED_SCHEMA_KEYWORDS`].
    pub response: CValue,
    /// Subset of `Device@1` error codes this operation may return.
    pub errors: Vec<ErrorCode>,
    /// Setting names that may be changed live without stopping acquisition.
    pub live_settings: Vec<String>,
}

/// One typed column in a data-plane acquisition batch schema.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchColumnDesc {
    pub name: String,
    pub unit: SiUnit,
    pub encoding: EncodingDescriptor,
}

/// One data-plane batch schema defined by a capability package.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchSchemaDesc {
    pub id: String,
    pub columns: Vec<BatchColumnDesc>,
    pub validity_mask: bool,
    pub seq_range: bool,
}

/// Project-authored Markdown semantics note and its SHA-256 digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticsNote {
    pub text: String,
    pub digest: Digest,
}

impl SemanticsNote {
    /// Constructs a semantics note computing its exact SHA-256 digest over UTF-8 `text`.
    pub fn new(text: impl Into<String>) -> SemanticsNote {
        let text = text.into();
        let digest = sha256_str(&text);
        SemanticsNote { text, digest }
    }
}

/// One conformance requirement item in a package's conformance profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceItemDesc {
    pub id: String,
    pub summary: String,
    /// Optional SHA-256 digest of a conformance fixture (the only external reference allowed
    /// alongside semantics note digests, REQ-005).
    pub fixture_digest: Option<Digest>,
}

/// Conformance profile identifying required conformance items for the package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceProfileDesc {
    pub id: String,
    pub items: Vec<ConformanceItemDesc>,
}

/// Complete `capability-package/v1` description.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageDescription {
    pub manifest: PackageManifest,
    /// Optional internal reusable JSON Schema fragments targeted by `"$ref": "#/$defs/<name>"`.
    pub defs: BTreeMap<String, CValue>,
    pub operations: Vec<OperationDesc>,
    pub batch_schemas: Vec<BatchSchemaDesc>,
    pub semantics: SemanticsNote,
    pub profile: ConformanceProfileDesc,
}

/// Trait implemented by every capability package in `fid-hal-idl`.
pub trait CapabilityPackage {
    /// Returns the language-neutral `capability-package/v1` description of this package.
    fn description() -> PackageDescription;
}

/// Validates a [`CapabilityPackage`] implementation and returns its canonical digest.
pub fn validate_package<P: CapabilityPackage>() -> Result<Digest, PackageError> {
    let desc = P::description();
    desc.validate()?;
    desc.digest()
}

// ------------------------------------------------------------------ validation & canonical JSON

impl PackageDescription {
    /// Validates all structural, semantic, allowlist and security invariants of this description,
    /// returning `package_invalid { pointer, reason }` on the first violation.
    pub fn validate(&self) -> Result<(), PackageError> {
        validate_manifest(&self.manifest)?;

        for (def_name, schema) in &self.defs {
            let ptr = format!("/$defs/{}", escape_ptr(def_name));
            if !is_def_name(def_name) {
                return Err(PackageError::invalid(ptr, "invalid $defs entry name"));
            }
            validate_schema_fragment(schema, &ptr, &self.defs, 0)?;
        }
        check_ref_cycles(&self.defs)?;

        let mut op_names = BTreeSet::new();
        for (i, op) in self.operations.iter().enumerate() {
            let op_ptr = format!("/operations/{i}");
            let name_ptr = format!("{op_ptr}/name");
            if !is_snake_id(&op.name) {
                return Err(PackageError::invalid(name_ptr, format!("invalid operation name {:?}", op.name)));
            }
            if !op_names.insert(op.name.as_str()) {
                return Err(PackageError::invalid(name_ptr, format!("duplicate operation name {:?}", op.name)));
            }
            validate_schema_fragment(&op.request, &format!("{op_ptr}/request"), &self.defs, 0)?;
            validate_schema_fragment(&op.response, &format!("{op_ptr}/response"), &self.defs, 0)?;

            let mut seen_errs = BTreeSet::new();
            for (j, &ec) in op.errors.iter().enumerate() {
                if !seen_errs.insert(ec) {
                    return Err(PackageError::invalid(
                        format!("{op_ptr}/errors/{j}"),
                        format!("duplicate error code {}", ec.as_str()),
                    ));
                }
            }
            let mut seen_settings = BTreeSet::new();
            for (j, s) in op.live_settings.iter().enumerate() {
                let s_ptr = format!("{op_ptr}/live_settings/{j}");
                if !is_dotted_id(s) {
                    return Err(PackageError::invalid(s_ptr, format!("invalid setting name {s:?}")));
                }
                if !seen_settings.insert(s.as_str()) {
                    return Err(PackageError::invalid(s_ptr, format!("duplicate live setting {s:?}")));
                }
            }
        }

        let mut batch_ids = BTreeSet::new();
        for (i, bs) in self.batch_schemas.iter().enumerate() {
            let bs_ptr = format!("/batch_schemas/{i}");
            let id_ptr = format!("{bs_ptr}/id");
            if !is_dotted_id(&bs.id) {
                return Err(PackageError::invalid(id_ptr, format!("invalid batch schema id {:?}", bs.id)));
            }
            if !batch_ids.insert(bs.id.as_str()) {
                return Err(PackageError::invalid(id_ptr, format!("duplicate batch schema id {:?}", bs.id)));
            }
            if bs.columns.is_empty() {
                return Err(PackageError::invalid(
                    format!("{bs_ptr}/columns"),
                    "batch schema must have at least one column",
                ));
            }
            let mut col_names = BTreeSet::new();
            for (j, col) in bs.columns.iter().enumerate() {
                let col_ptr = format!("{bs_ptr}/columns/{j}");
                let cname_ptr = format!("{col_ptr}/name");
                if !is_snake_id(&col.name) {
                    return Err(PackageError::invalid(cname_ptr, format!("invalid column name {:?}", col.name)));
                }
                if !col_names.insert(col.name.as_str()) {
                    return Err(PackageError::invalid(cname_ptr, format!("duplicate column name {:?}", col.name)));
                }
                if col.encoding.unit() != col.unit {
                    return Err(PackageError::invalid(
                        format!("{col_ptr}/encoding/unit"),
                        format!(
                            "encoding unit {:?} does not match column unit {:?}",
                            col.encoding.unit().as_str(),
                            col.unit.as_str()
                        ),
                    ));
                }
            }
        }

        validate_semantics(&self.semantics)?;
        validate_profile(&self.profile)?;
        Ok(())
    }

    /// Converts this validated description into its canonical [`CValue`] with `operations` sorted
    /// by `name`, `batch_schemas` sorted by `id`, `profile.items` sorted by `id`, `errors` sorted
    /// by wire code, and `live_settings` sorted lexicographically (REQ-003).
    pub fn canonical_value(&self) -> Result<CValue, PackageError> {
        self.validate()?;

        let mut m = BTreeMap::new();
        m.insert("schema".to_string(), CValue::Str(PACKAGE_SCHEMA_TAG.to_string()));
        m.insert(
            "manifest".to_string(),
            CValue::Obj(BTreeMap::from([
                ("deprecated".to_string(), CValue::Bool(self.manifest.deprecated)),
                ("name".to_string(), CValue::Str(self.manifest.name.clone())),
                ("tier3_usable".to_string(), CValue::Bool(self.manifest.tier3_usable)),
                ("version".to_string(), CValue::Str(self.manifest.version.clone())),
            ])),
        );

        if !self.defs.is_empty() {
            m.insert("$defs".to_string(), CValue::Obj(self.defs.clone()));
        }

        let mut ops: Vec<&OperationDesc> = self.operations.iter().collect();
        ops.sort_by(|a, b| a.name.cmp(&b.name));
        let ops_arr = ops
            .into_iter()
            .map(|op| {
                let mut errs: Vec<&str> = op.errors.iter().map(|e| e.as_str()).collect();
                errs.sort_unstable();
                let mut live = op.live_settings.clone();
                live.sort();
                CValue::Obj(BTreeMap::from([
                    ("class".to_string(), CValue::Str(op.class.as_str().to_string())),
                    ("errors".to_string(), CValue::Arr(errs.into_iter().map(|s| CValue::Str(s.to_string())).collect())),
                    ("live_settings".to_string(), CValue::Arr(live.into_iter().map(CValue::Str).collect())),
                    ("name".to_string(), CValue::Str(op.name.clone())),
                    ("request".to_string(), op.request.clone()),
                    ("response".to_string(), op.response.clone()),
                ]))
            })
            .collect();
        m.insert("operations".to_string(), CValue::Arr(ops_arr));

        let mut batches: Vec<&BatchSchemaDesc> = self.batch_schemas.iter().collect();
        batches.sort_by(|a, b| a.id.cmp(&b.id));
        let batches_arr = batches
            .into_iter()
            .map(|bs| {
                let cols = bs
                    .columns
                    .iter()
                    .map(|c| {
                        CValue::Obj(BTreeMap::from([
                            ("encoding".to_string(), c.encoding.to_value()),
                            ("name".to_string(), CValue::Str(c.name.clone())),
                            ("unit".to_string(), CValue::Str(c.unit.as_str().to_string())),
                        ]))
                    })
                    .collect();
                CValue::Obj(BTreeMap::from([
                    ("columns".to_string(), CValue::Arr(cols)),
                    ("id".to_string(), CValue::Str(bs.id.clone())),
                    ("seq_range".to_string(), CValue::Bool(bs.seq_range)),
                    ("validity_mask".to_string(), CValue::Bool(bs.validity_mask)),
                ]))
            })
            .collect();
        m.insert("batch_schemas".to_string(), CValue::Arr(batches_arr));

        m.insert(
            "semantics".to_string(),
            CValue::Obj(BTreeMap::from([
                ("digest".to_string(), CValue::Str(self.semantics.digest.as_str().to_string())),
                ("text".to_string(), CValue::Str(self.semantics.text.clone())),
            ])),
        );

        let mut items: Vec<&ConformanceItemDesc> = self.profile.items.iter().collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        let items_arr = items
            .into_iter()
            .map(|it| {
                let mut im = BTreeMap::from([
                    ("id".to_string(), CValue::Str(it.id.clone())),
                    ("summary".to_string(), CValue::Str(it.summary.clone())),
                ]);
                if let Some(d) = it.fixture_digest {
                    im.insert("fixture_digest".to_string(), CValue::Str(d.as_str().to_string()));
                }
                CValue::Obj(im)
            })
            .collect();
        m.insert(
            "profile".to_string(),
            CValue::Obj(BTreeMap::from([
                ("id".to_string(), CValue::Str(self.profile.id.clone())),
                ("items".to_string(), CValue::Arr(items_arr)),
            ])),
        );

        Ok(CValue::Obj(m))
    }

    /// Serialises the validated description to RFC 8785 canonical JSON bytes with sorted lists
    /// (REQ-003).
    pub fn canonical_json(&self) -> Result<Vec<u8>, PackageError> {
        let v = self.canonical_value()?;
        fid_canon::to_canonical_bytes(&v).map_err(|e| PackageError::invalid("", format!("canonical JSON error: {e}")))
    }

    /// Computes the canonical `sha256:` digest over RFC 8785 canonical JSON with sorted lists
    /// (REQ-003, AC-1, AC-6).
    pub fn digest(&self) -> Result<Digest, PackageError> {
        let bytes = self.canonical_json()?;
        Ok(Digest::from_bytes(fid_canon::digest_bytes(&bytes).0))
    }

    /// Parses and strictly validates a `capability-package/v1` JSON document, reporting
    /// `package_invalid { pointer, reason }` on any schema, semantic, allowlist or security error.
    pub fn from_json(bytes: &[u8]) -> Result<PackageDescription, PackageError> {
        let val = fid_canon::parse_with_limits(bytes, MAX_PACKAGE_BYTES, MAX_PACKAGE_DEPTH)
            .map_err(|e| PackageError::invalid("", format!("invalid JSON: {e}")))?;
        Self::from_value(&val)
    }

    /// Parses and strictly validates a [`CValue`] into a [`PackageDescription`].
    pub fn from_value(val: &CValue) -> Result<PackageDescription, PackageError> {
        let root = req_obj(val, "")?;
        check_allowed_keys(
            root,
            &["$defs", "batch_schemas", "manifest", "operations", "profile", "schema", "semantics"],
            "",
        )?;

        let schema_tag = req_str(root, "schema", "")?;
        if schema_tag != PACKAGE_SCHEMA_TAG {
            return Err(PackageError::invalid(
                "/schema",
                format!("expected schema {:?}, found {:?}", PACKAGE_SCHEMA_TAG, schema_tag),
            ));
        }

        // Parse optional $defs first so internal $ref checks in operations resolve against them.
        let mut defs = BTreeMap::new();
        if let Some(defs_v) = root.get("$defs") {
            let defs_obj = req_obj(defs_v, "/$defs")?;
            for (k, v) in defs_obj {
                let ptr = format!("/$defs/{}", escape_ptr(k));
                if !is_def_name(k) {
                    return Err(PackageError::invalid(ptr, "invalid $defs entry name"));
                }
                defs.insert(k.clone(), v.clone());
            }
            for (k, v) in &defs {
                let ptr = format!("/$defs/{}", escape_ptr(k));
                validate_schema_fragment(v, &ptr, &defs, 0)?;
            }
            check_ref_cycles(&defs)?;
        }

        // Manifest
        let manifest_obj = req_obj(req_field(root, "manifest", "")?, "/manifest")?;
        check_allowed_keys(manifest_obj, &["deprecated", "name", "tier3_usable", "version"], "/manifest")?;
        let manifest = PackageManifest {
            name: req_str(manifest_obj, "name", "/manifest")?.to_string(),
            version: req_str(manifest_obj, "version", "/manifest")?.to_string(),
            deprecated: req_bool(manifest_obj, "deprecated", "/manifest")?,
            tier3_usable: req_bool(manifest_obj, "tier3_usable", "/manifest")?,
        };
        validate_manifest(&manifest)?;

        // Operations
        let ops_arr = req_arr(req_field(root, "operations", "")?, "/operations")?;
        let mut operations = Vec::with_capacity(ops_arr.len());
        let mut seen_ops = BTreeSet::new();
        for (i, item) in ops_arr.iter().enumerate() {
            let op_ptr = format!("/operations/{i}");
            let om = req_obj(item, &op_ptr)?;
            check_allowed_keys(om, &["class", "errors", "live_settings", "name", "request", "response"], &op_ptr)?;

            let name = req_str(om, "name", &op_ptr)?.to_string();
            let name_ptr = format!("{op_ptr}/name");
            if !is_snake_id(&name) {
                return Err(PackageError::invalid(name_ptr, format!("invalid operation name {name:?}")));
            }
            if !seen_ops.insert(name.clone()) {
                return Err(PackageError::invalid(name_ptr, format!("duplicate operation name {name:?}")));
            }

            let class_str = req_str(om, "class", &op_ptr)?;
            let class = SideEffect::parse(class_str).map_err(|_| {
                PackageError::invalid(format!("{op_ptr}/class"), format!("unknown side-effect class {class_str:?}"))
            })?;

            let req_v = req_field(om, "request", &op_ptr)?;
            validate_schema_fragment(req_v, &format!("{op_ptr}/request"), &defs, 0)?;

            let resp_v = req_field(om, "response", &op_ptr)?;
            validate_schema_fragment(resp_v, &format!("{op_ptr}/response"), &defs, 0)?;

            let errs_arr = req_arr(req_field(om, "errors", &op_ptr)?, &format!("{op_ptr}/errors"))?;
            let mut errors = Vec::with_capacity(errs_arr.len());
            let mut seen_errs = BTreeSet::new();
            for (j, ev) in errs_arr.iter().enumerate() {
                let e_ptr = format!("{op_ptr}/errors/{j}");
                let CValue::Str(code_s) = ev else {
                    return Err(PackageError::invalid(e_ptr, "error code must be a string"));
                };
                let ec = ErrorCode::parse(code_s)
                    .map_err(|_| PackageError::invalid(&e_ptr, format!("unknown Device@1 error code {code_s:?}")))?;
                if !seen_errs.insert(ec) {
                    return Err(PackageError::invalid(e_ptr, format!("duplicate error code {code_s:?}")));
                }
                errors.push(ec);
            }

            let mut live_settings = Vec::new();
            if let Some(ls_v) = om.get("live_settings") {
                let ls_arr = req_arr(ls_v, &format!("{op_ptr}/live_settings"))?;
                let mut seen_ls = BTreeSet::new();
                for (j, sv) in ls_arr.iter().enumerate() {
                    let s_ptr = format!("{op_ptr}/live_settings/{j}");
                    let CValue::Str(s) = sv else {
                        return Err(PackageError::invalid(s_ptr, "live setting name must be a string"));
                    };
                    if !is_dotted_id(s) {
                        return Err(PackageError::invalid(s_ptr, format!("invalid live setting name {s:?}")));
                    }
                    if !seen_ls.insert(s.clone()) {
                        return Err(PackageError::invalid(s_ptr, format!("duplicate live setting {s:?}")));
                    }
                    live_settings.push(s.clone());
                }
            }

            operations.push(OperationDesc {
                name,
                class,
                request: req_v.clone(),
                response: resp_v.clone(),
                errors,
                live_settings,
            });
        }

        // Batch schemas
        let batches_arr = req_arr(req_field(root, "batch_schemas", "")?, "/batch_schemas")?;
        let mut batch_schemas = Vec::with_capacity(batches_arr.len());
        let mut seen_batches = BTreeSet::new();
        for (i, item) in batches_arr.iter().enumerate() {
            let bs_ptr = format!("/batch_schemas/{i}");
            let bm = req_obj(item, &bs_ptr)?;
            check_allowed_keys(bm, &["columns", "id", "seq_range", "validity_mask"], &bs_ptr)?;

            let id = req_str(bm, "id", &bs_ptr)?.to_string();
            let id_ptr = format!("{bs_ptr}/id");
            if !is_dotted_id(&id) {
                return Err(PackageError::invalid(id_ptr, format!("invalid batch schema id {id:?}")));
            }
            if !seen_batches.insert(id.clone()) {
                return Err(PackageError::invalid(id_ptr, format!("duplicate batch schema id {id:?}")));
            }

            let cols_arr = req_arr(req_field(bm, "columns", &bs_ptr)?, &format!("{bs_ptr}/columns"))?;
            if cols_arr.is_empty() {
                return Err(PackageError::invalid(
                    format!("{bs_ptr}/columns"),
                    "batch schema must have at least one column",
                ));
            }
            let mut columns = Vec::with_capacity(cols_arr.len());
            let mut seen_cols = BTreeSet::new();
            for (j, cv) in cols_arr.iter().enumerate() {
                let col_ptr = format!("{bs_ptr}/columns/{j}");
                let cm = req_obj(cv, &col_ptr)?;
                check_allowed_keys(cm, &["encoding", "name", "unit"], &col_ptr)?;

                let col_name = req_str(cm, "name", &col_ptr)?.to_string();
                let cname_ptr = format!("{col_ptr}/name");
                if !is_snake_id(&col_name) {
                    return Err(PackageError::invalid(cname_ptr, format!("invalid column name {col_name:?}")));
                }
                if !seen_cols.insert(col_name.clone()) {
                    return Err(PackageError::invalid(cname_ptr, format!("duplicate column name {col_name:?}")));
                }

                let unit_s = req_str(cm, "unit", &col_ptr)?;
                let unit = SiUnit::parse(unit_s)
                    .map_err(|e| PackageError::invalid(format!("{col_ptr}/unit"), e.to_string()))?;

                let enc_ptr = format!("{col_ptr}/encoding");
                let enc_v = cm
                    .get("encoding")
                    .ok_or_else(|| PackageError::invalid(&enc_ptr, "missing required EncodingDescriptor"))?;
                let encoding = EncodingDescriptor::from_value(enc_v.clone())
                    .map_err(|e| PackageError::invalid(&enc_ptr, e.to_string()))?;
                if encoding.unit() != unit {
                    return Err(PackageError::invalid(
                        format!("{enc_ptr}/unit"),
                        format!(
                            "encoding unit {:?} does not match column unit {:?}",
                            encoding.unit().as_str(),
                            unit.as_str()
                        ),
                    ));
                }

                columns.push(BatchColumnDesc { name: col_name, unit, encoding });
            }

            let validity_mask = req_bool(bm, "validity_mask", &bs_ptr)?;
            let seq_range = req_bool(bm, "seq_range", &bs_ptr)?;
            batch_schemas.push(BatchSchemaDesc { id, columns, validity_mask, seq_range });
        }

        // Semantics note
        let sem_obj = req_obj(req_field(root, "semantics", "")?, "/semantics")?;
        check_allowed_keys(sem_obj, &["digest", "text"], "/semantics")?;
        let sem_text = req_str(sem_obj, "text", "/semantics")?.to_string();
        let sem_digest_s = req_str(sem_obj, "digest", "/semantics")?;
        let sem_digest = Digest::parse(sem_digest_s)
            .map_err(|e| PackageError::invalid("/semantics/digest", format!("invalid digest: {e}")))?;
        let semantics = SemanticsNote { text: sem_text, digest: sem_digest };
        validate_semantics(&semantics)?;

        // Conformance profile
        let prof_obj = req_obj(req_field(root, "profile", "")?, "/profile")?;
        check_allowed_keys(prof_obj, &["id", "items"], "/profile")?;
        let prof_id = req_str(prof_obj, "id", "/profile")?.to_string();
        let items_arr = req_arr(req_field(prof_obj, "items", "/profile")?, "/profile/items")?;
        let mut items = Vec::with_capacity(items_arr.len());
        for (i, iv) in items_arr.iter().enumerate() {
            let ip = format!("/profile/items/{i}");
            let im = req_obj(iv, &ip)?;
            check_allowed_keys(im, &["fixture_digest", "id", "summary"], &ip)?;
            let id = req_str(im, "id", &ip)?.to_string();
            let summary = req_str(im, "summary", &ip)?.to_string();
            let fixture_digest = match im.get("fixture_digest") {
                None | Some(CValue::Null) => None,
                Some(CValue::Str(s)) => {
                    let d = Digest::parse(s).map_err(|e| {
                        PackageError::invalid(format!("{ip}/fixture_digest"), format!("invalid fixture digest: {e}"))
                    })?;
                    Some(d)
                }
                Some(_) => {
                    return Err(PackageError::invalid(
                        format!("{ip}/fixture_digest"),
                        "fixture_digest must be a sha256 string",
                    ));
                }
            };
            items.push(ConformanceItemDesc { id, summary, fixture_digest });
        }
        let profile = ConformanceProfileDesc { id: prof_id, items };
        validate_profile(&profile)?;

        let desc = PackageDescription { manifest, defs, operations, batch_schemas, semantics, profile };
        desc.validate()?;
        Ok(desc)
    }
}

/// Convenience helper: parses and validates raw `capability-package/v1` JSON bytes and returns its
/// canonical [`Digest`].
pub fn validate_and_digest_json(bytes: &[u8]) -> Result<Digest, PackageError> {
    let desc = PackageDescription::from_json(bytes)?;
    desc.digest()
}

// ------------------------------------------------------------------ validation helpers

fn validate_manifest(m: &PackageManifest) -> Result<(), PackageError> {
    if !is_package_name(&m.name) {
        return Err(PackageError::invalid("/manifest/name", format!("invalid package name {:?}", m.name)));
    }
    if !is_semver(&m.version) {
        return Err(PackageError::invalid(
            "/manifest/version",
            format!("version {:?} is not a valid SemVer 2.0.0 string", m.version),
        ));
    }
    Ok(())
}

fn validate_semantics(s: &SemanticsNote) -> Result<(), PackageError> {
    if s.text.trim().is_empty() {
        return Err(PackageError::invalid("/semantics/text", "semantics note text must not be empty"));
    }
    if s.text.len() > MAX_SEMANTICS_BYTES {
        return Err(PackageError::invalid(
            "/semantics/text",
            format!("semantics note exceeds {MAX_SEMANTICS_BYTES} bytes"),
        ));
    }
    check_no_executable_or_external(&s.text, "/semantics/text")?;
    let expected = sha256_str(&s.text);
    if s.digest != expected {
        return Err(PackageError::invalid(
            "/semantics/digest",
            format!("semantics digest mismatch: expected {expected}, got {}", s.digest),
        ));
    }
    Ok(())
}

fn validate_profile(p: &ConformanceProfileDesc) -> Result<(), PackageError> {
    if !is_profile_id(&p.id) {
        return Err(PackageError::invalid("/profile/id", format!("invalid conformance profile id {:?}", p.id)));
    }
    let mut seen = BTreeSet::new();
    for (i, item) in p.items.iter().enumerate() {
        let ip = format!("/profile/items/{i}");
        if !is_profile_id(&item.id) {
            return Err(PackageError::invalid(format!("{ip}/id"), format!("invalid profile item id {:?}", item.id)));
        }
        if !seen.insert(item.id.as_str()) {
            return Err(PackageError::invalid(format!("{ip}/id"), format!("duplicate profile item id {:?}", item.id)));
        }
        if item.summary.trim().is_empty() || item.summary.len() > 2048 {
            return Err(PackageError::invalid(format!("{ip}/summary"), "profile item summary must be 1..=2048 bytes"));
        }
        check_no_executable_or_external(&item.summary, &format!("{ip}/summary"))?;
    }
    Ok(())
}

fn validate_schema_fragment(
    val: &CValue,
    ptr: &str,
    defs: &BTreeMap<String, CValue>,
    depth: usize,
) -> Result<(), PackageError> {
    if depth > 32 {
        return Err(PackageError::invalid(ptr, "schema nesting exceeds depth 32"));
    }
    let obj = req_obj(val, ptr)?;

    for key in obj.keys() {
        if !ALLOWED_SCHEMA_KEYWORDS.contains(&key.as_str()) {
            return Err(PackageError::invalid(
                format!("{ptr}/{}", escape_ptr(key)),
                format!("disallowed JSON Schema keyword {key:?}"),
            ));
        }
    }

    if let Some(ref_v) = obj.get("$ref") {
        let ref_ptr = format!("{ptr}/$ref");
        let CValue::Str(r) = ref_v else {
            return Err(PackageError::invalid(ref_ptr, "$ref must be a string"));
        };
        let Some(target) = r.strip_prefix("#/$defs/") else {
            return Err(PackageError::invalid(
                ref_ptr,
                format!("external or non-$defs $ref {r:?} is forbidden; only internal \"#/$defs/<name>\" is allowed"),
            ));
        };
        if !is_def_name(target) || !defs.contains_key(target) {
            return Err(PackageError::invalid(ref_ptr, format!("unresolved internal $ref {r:?}")));
        }
    }

    if let Some(t_v) = obj.get("type") {
        let t_ptr = format!("{ptr}/type");
        let CValue::Str(t) = t_v else {
            return Err(PackageError::invalid(t_ptr, "type must be a string"));
        };
        if !ALLOWED_SCHEMA_TYPES.contains(&t.as_str()) {
            return Err(PackageError::invalid(t_ptr, format!("unsupported schema type {t:?}")));
        }
    }

    if let Some(ap_v) = obj.get("additionalProperties") {
        if *ap_v != CValue::Bool(false) {
            return Err(PackageError::invalid(
                format!("{ptr}/additionalProperties"),
                "additionalProperties must be false",
            ));
        }
    }

    if let Some(props_v) = obj.get("properties") {
        let props_ptr = format!("{ptr}/properties");
        let props = req_obj(props_v, &props_ptr)?;
        for (k, sub) in props {
            if k.is_empty() || k.len() > 128 || k.contains('\0') {
                return Err(PackageError::invalid(format!("{props_ptr}/{}", escape_ptr(k)), "invalid property name"));
            }
            validate_schema_fragment(sub, &format!("{props_ptr}/{}", escape_ptr(k)), defs, depth + 1)?;
        }
    }

    if let Some(req_v) = obj.get("required") {
        let req_ptr = format!("{ptr}/required");
        let arr = req_arr(req_v, &req_ptr)?;
        let mut seen = BTreeSet::new();
        for (i, item) in arr.iter().enumerate() {
            let ip = format!("{req_ptr}/{i}");
            let CValue::Str(s) = item else {
                return Err(PackageError::invalid(ip, "required entry must be a string"));
            };
            if s.is_empty() || !seen.insert(s.as_str()) {
                return Err(PackageError::invalid(ip, format!("empty or duplicate required field {s:?}")));
            }
        }
    }

    if let Some(items_v) = obj.get("items") {
        validate_schema_fragment(items_v, &format!("{ptr}/items"), defs, depth + 1)?;
    }

    if let Some(enum_v) = obj.get("enum") {
        let e_ptr = format!("{ptr}/enum");
        let arr = req_arr(enum_v, &e_ptr)?;
        if arr.is_empty() {
            return Err(PackageError::invalid(e_ptr, "enum must not be empty"));
        }
        for (i, ev) in arr.iter().enumerate() {
            let ep = format!("{e_ptr}/{i}");
            match ev {
                CValue::Str(s) => check_no_executable_or_external(s, &ep)?,
                CValue::Bool(_) | CValue::Null | CValue::I64(_) | CValue::U64(_) | CValue::Num(_) => {}
                _ => {
                    return Err(PackageError::invalid(ep, "enum entries must be JSON scalars"));
                }
            }
        }
    }

    let min_val = match obj.get("minimum") {
        None => None,
        Some(v) => Some(as_finite_f64(v, &format!("{ptr}/minimum"))?),
    };
    let max_val = match obj.get("maximum") {
        None => None,
        Some(v) => Some(as_finite_f64(v, &format!("{ptr}/maximum"))?),
    };
    if let (Some(lo), Some(hi)) = (min_val, max_val) {
        if lo > hi {
            return Err(PackageError::invalid(format!("{ptr}/minimum"), "minimum exceeds maximum"));
        }
    }

    if let Some(ml_v) = obj.get("maxLength") {
        let ml_ptr = format!("{ptr}/maxLength");
        let ok = match ml_v {
            CValue::U64(_) => true,
            CValue::I64(n) => *n >= 0,
            CValue::Num(x) => x.is_finite() && *x >= 0.0 && x.fract() == 0.0,
            _ => false,
        };
        if !ok {
            return Err(PackageError::invalid(ml_ptr, "maxLength must be a non-negative integer"));
        }
    }

    Ok(())
}

fn check_ref_cycles(defs: &BTreeMap<String, CValue>) -> Result<(), PackageError> {
    // 0 = unvisited, 1 = visiting, 2 = done
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    for name in defs.keys() {
        dfs_ref_cycle(name, defs, &mut state)?;
    }
    Ok(())
}

fn dfs_ref_cycle<'a>(
    name: &'a str,
    defs: &'a BTreeMap<String, CValue>,
    state: &mut BTreeMap<&'a str, u8>,
) -> Result<(), PackageError> {
    match state.get(name).copied().unwrap_or(0) {
        2 => return Ok(()),
        1 => {
            return Err(PackageError::invalid(
                format!("/$defs/{}", escape_ptr(name)),
                format!("cyclic $ref through $defs/{name}"),
            ));
        }
        _ => {}
    }
    state.insert(name, 1);
    if let Some(schema) = defs.get(name) {
        let mut refs = Vec::new();
        collect_refs(schema, &mut refs);
        for target in refs {
            dfs_ref_cycle(target, defs, state)?;
        }
    }
    state.insert(name, 2);
    Ok(())
}

fn collect_refs<'a>(v: &'a CValue, out: &mut Vec<&'a str>) {
    if let CValue::Obj(m) = v {
        if let Some(CValue::Str(r)) = m.get("$ref") {
            if let Some(target) = r.strip_prefix("#/$defs/") {
                out.push(target);
            }
        }
        if let Some(CValue::Obj(props)) = m.get("properties") {
            for sub in props.values() {
                collect_refs(sub, out);
            }
        }
        if let Some(items) = m.get("items") {
            collect_refs(items, out);
        }
    }
}

fn check_no_executable_or_external(s: &str, ptr: &str) -> Result<(), PackageError> {
    if s.contains('\0') {
        return Err(PackageError::invalid(ptr, "NUL byte is forbidden"));
    }
    let lower = s.to_ascii_lowercase();
    for forbidden in ["<script", "javascript:", "data:text/", "data:application/", "http://", "https://", "file://"] {
        if lower.contains(forbidden) {
            return Err(PackageError::invalid(
                ptr,
                format!("forbidden executable or external reference token {forbidden:?}"),
            ));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ identifier & semver checks

/// Strict SemVer 2.0.0 validation: `MAJOR.MINOR.PATCH[-prerelease][+build]`.
pub fn is_semver(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    let (core_and_pre, build) = match s.split_once('+') {
        Some((l, r)) => (l, Some(r)),
        None => (s, None),
    };
    let (core, pre) = match core_and_pre.split_once('-') {
        Some((l, r)) => (l, Some(r)),
        None => (core_and_pre, None),
    };

    let mut nums = core.split('.');
    let (Some(maj), Some(min), Some(pat), None) = (nums.next(), nums.next(), nums.next(), nums.next()) else {
        return false;
    };
    if !is_semver_num(maj) || !is_semver_num(min) || !is_semver_num(pat) {
        return false;
    }
    if let Some(p) = pre {
        if p.is_empty() {
            return false;
        }
        for id in p.split('.') {
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return false;
            }
            if id.bytes().all(|b| b.is_ascii_digit()) && !is_semver_num(id) {
                return false;
            }
        }
    }
    if let Some(b) = build {
        if b.is_empty() {
            return false;
        }
        for id in b.split('.') {
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return false;
            }
        }
    }
    true
}

fn is_semver_num(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s.len() == 1 || !s.starts_with('0'))
}

fn is_package_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.contains("..")
        && s.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn is_def_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn is_snake_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn is_dotted_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.contains("..")
        && s.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

/// A profile or profile-item ID: `[A-Za-z0-9][A-Za-z0-9._/-]*`, at most 64 bytes, no `..`,
/// optionally ending in one `@<major>` suffix (as in `Device-core@1`, HAL.IDL.BASE.DESC).
fn is_profile_id(s: &str) -> bool {
    let base = match s.split_once('@') {
        Some((base, major)) if is_semver_num(major) => base,
        Some(_) => return false,
        None => s,
    };
    !base.is_empty()
        && s.len() <= 64
        && !base.contains("..")
        && base.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && base.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
}

// ------------------------------------------------------------------ JSON helpers

fn escape_ptr(seg: &str) -> String {
    seg.replace('~', "~0").replace('/', "~1")
}

fn sha256_str(s: &str) -> Digest {
    Digest::from_bytes(fid_canon::digest_bytes(s.as_bytes()).0)
}

fn req_obj<'a>(v: &'a CValue, ptr: &str) -> Result<&'a BTreeMap<String, CValue>, PackageError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(PackageError::invalid(ptr, "expected JSON object")),
    }
}

fn req_arr<'a>(v: &'a CValue, ptr: &str) -> Result<&'a [CValue], PackageError> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => Err(PackageError::invalid(ptr, "expected JSON array")),
    }
}

fn req_field<'a>(m: &'a BTreeMap<String, CValue>, key: &str, parent: &str) -> Result<&'a CValue, PackageError> {
    m.get(key).ok_or_else(|| {
        PackageError::invalid(format!("{parent}/{}", escape_ptr(key)), format!("missing required field {key:?}"))
    })
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, key: &str, parent: &str) -> Result<&'a str, PackageError> {
    match req_field(m, key, parent)? {
        CValue::Str(s) => Ok(s.as_str()),
        _ => Err(PackageError::invalid(format!("{parent}/{}", escape_ptr(key)), format!("{key:?} must be a string"))),
    }
}

fn req_bool(m: &BTreeMap<String, CValue>, key: &str, parent: &str) -> Result<bool, PackageError> {
    match req_field(m, key, parent)? {
        CValue::Bool(b) => Ok(*b),
        _ => Err(PackageError::invalid(format!("{parent}/{}", escape_ptr(key)), format!("{key:?} must be a boolean"))),
    }
}

fn check_allowed_keys(m: &BTreeMap<String, CValue>, allowed: &[&str], parent: &str) -> Result<(), PackageError> {
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(PackageError::invalid(format!("{parent}/{}", escape_ptr(k)), format!("unknown field {k:?}")));
        }
    }
    Ok(())
}

fn as_finite_f64(v: &CValue, ptr: &str) -> Result<f64, PackageError> {
    let x = match v {
        CValue::Num(n) => *n,
        CValue::I64(n) => *n as f64,
        CValue::U64(n) => *n as f64,
        _ => return Err(PackageError::invalid(ptr, "expected finite JSON number")),
    };
    if !x.is_finite() {
        return Err(PackageError::invalid(ptr, "number must be finite"));
    }
    Ok(x)
}
