//! Integration tests covering AC-1..AC-5 and REQ-001..REQ-006 of spec node
//! `IO.REG.PLUGIN.ABI`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{CValue, Digest as CanonDigest};
use fid_io_plugin::abi::adapter::{
    register_descriptor, PluginReaderAdapter, PluginWriterAdapter, ENTITY_TYPE_DRAWING_PAGE, ENTITY_TYPE_MESH,
    ENTITY_TYPE_POINTS, ENTITY_TYPE_QIF_JSON, ENTITY_TYPE_TABLE,
};
use fid_io_plugin::abi::codegen::{
    audit_capabilities, check_generated_freshness, COMMITTED_IDL, COMMITTED_MSG_PROTOCOL, COMMITTED_WIT,
};
use fid_io_plugin::abi::{
    negotiate_version, AbiError, ContractVersion, DrawingPageRecord, FormatPluginReader, FormatPluginWriter, KvEntry,
    MeshRecord, PluginDescriptor, PluginError, Point3, PointsRecord, QifJsonRecord, ReadRequest, Record, TableRecord,
    TriIndices, WarningRecord, WriteEntity, WriteRequest, HOST_CONTRACT_VERSION,
};
use fid_io_reg::detect::list::{list_formats, list_formats_value, IMPORT_LIST_FORMATS};
use fid_io_reg::env::export::{ByteSink, CountingSink, Writer};
use fid_io_reg::env::import::{
    import_file_id, register_import_events, BlobSink, ImportEnv, ImportFile, ImportParams, ImportRun, MemoryBlobs,
};
use fid_io_reg::prov::{PrincipalCommit, PrincipalKind};
use fid_io_reg::{
    Direction, FormatId, LimitRequest, Origin, OriginFilter, Principal, Registry, ReportBuilder, RootAccess, RootId,
    RootSet, UntrustedText,
};
use fid_project::{ElementTypeDef, ElementTypeRegistry, Tree, Uuid};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/io/reg/plugin/abi/");

fn load_descriptor(name: &str) -> PluginDescriptor {
    let bytes = fs::read(format!("{FIXTURE_DIR}{name}")).unwrap();
    PluginDescriptor::from_json(&bytes).unwrap()
}

/// In-process test plugin implementing both [`FormatPluginReader`] and [`FormatPluginWriter`].
#[derive(Debug)]
struct ToyPlugin {
    descriptor: PluginDescriptor,
}

impl ToyPlugin {
    fn from_fixture(name: &str) -> Self {
        Self { descriptor: load_descriptor(name) }
    }
}

impl FormatPluginReader for ToyPlugin {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn read(&self, req: &mut ReadRequest<'_>) -> Result<Vec<Record>, PluginError> {
        let mut raw = Vec::new();
        req.stream
            .read_to_end(&mut raw)
            .map_err(|e| PluginError::malformed(format!("stream read error: {e}"), None))?;
        let text = String::from_utf8(raw).map_err(|_| PluginError::malformed("invalid UTF-8", Some(0)))?;
        let mut lines = text.lines();
        let header = lines.next().ok_or_else(|| PluginError::truncated("missing header", Some(0)))?;
        if header != "TOYPLG" {
            return Err(PluginError::malformed("bad magic header", Some(0)));
        }
        let mut current_unit: Option<UntrustedText> = None;
        let mut records = Vec::new();

        for line in lines {
            if let Some(u) = line.strip_prefix("UNIT ") {
                current_unit = Some(UntrustedText::new(u));
            } else if let Some(rest) = line.strip_prefix("POINTS ") {
                let (name, coords) =
                    rest.split_once(' ').ok_or_else(|| PluginError::malformed("bad POINTS line", None))?;
                let mut points = Vec::new();
                for triple in coords.split(';') {
                    let parts: Vec<f64> = triple
                        .split(' ')
                        .map(|s| s.parse::<f64>().map_err(|_| PluginError::malformed("bad float", None)))
                        .collect::<Result<_, _>>()?;
                    if parts.len() != 3 {
                        return Err(PluginError::malformed("point needs 3 coords", None));
                    }
                    points.push(Point3 { x: parts[0], y: parts[1], z: parts[2] });
                }
                records.push(Record::Points(PointsRecord {
                    name: UntrustedText::new(name),
                    points,
                    unit: current_unit.clone(),
                    attributes: vec![KvEntry::new("source_format", req.format_id.as_str())],
                }));
            } else if let Some(rest) = line.strip_prefix("TABLE ") {
                let (name, body) =
                    rest.split_once(' ').ok_or_else(|| PluginError::malformed("bad TABLE line", None))?;
                let (cols_s, rows_s) =
                    body.split_once('|').ok_or_else(|| PluginError::malformed("bad TABLE body", None))?;
                let columns = cols_s.split(',').map(UntrustedText::new).collect();
                let rows = rows_s.split(';').map(|r| r.split(',').map(UntrustedText::new).collect()).collect();
                records.push(Record::Table(TableRecord { name: UntrustedText::new(name), columns, rows }));
            } else if let Some(rest) = line.strip_prefix("WARN ") {
                let (code, detail) = rest.split_once(' ').unwrap_or((rest, ""));
                records.push(Record::Warning(WarningRecord {
                    code: UntrustedText::new(code),
                    detail: UntrustedText::new(detail),
                    byte_offset: Some(0),
                }));
            } else if !line.is_empty() {
                return Err(PluginError::malformed(format!("unexpected directive: {line}"), None));
            }
        }

        Ok(records)
    }
}

impl FormatPluginWriter for ToyPlugin {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn write(&self, req: &WriteRequest<'_>) -> Result<Vec<u8>, PluginError> {
        let mut out = String::from("TOYPLG\n");
        for ent in req.entities {
            out.push_str(&format!("ENTITY {} {} {}\n", ent.type_id, ent.name.as_display(), ent.payload.len()));
        }
        Ok(out.into_bytes())
    }
}

struct TmpDir {
    path: PathBuf,
}

impl TmpDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("fid-io-plugin-abi-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("in")).unwrap();
        Self { path }
    }

    fn write_in(&self, name: &str, bytes: &[u8]) {
        fs::write(self.path.join("in").join(name), bytes).unwrap();
    }

    fn roots(&self) -> RootSet {
        let mut rs = RootSet::new();
        rs.insert(RootId::new("in").unwrap(), RootAccess::Import, self.path.join("in"));
        rs
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn test_tree() -> (Tree, Uuid) {
    let mut reg = ElementTypeRegistry::new();
    let mut schema = BTreeMap::new();
    schema.insert("type".into(), CValue::Str("object".into()));
    let schema_val = CValue::Obj(schema);
    for (ty, cat) in [
        (ENTITY_TYPE_POINTS, "point_cloud"),
        (ENTITY_TYPE_MESH, "mesh"),
        (ENTITY_TYPE_QIF_JSON, "qif"),
        (ENTITY_TYPE_DRAWING_PAGE, "drawing"),
        (ENTITY_TYPE_TABLE, "table"),
    ] {
        reg.register(ElementTypeDef::new(ty, schema_val.clone(), cat).unwrap()).unwrap();
    }
    register_import_events(&mut reg, &[&import_file_id().unwrap()]).unwrap();
    let ns = Uuid::parse("01960000-0000-7000-8000-000000000001").unwrap();
    let tree = Tree::new(ns, Arc::new(reg)).unwrap();
    (tree, ns)
}

fn test_commit() -> PrincipalCommit {
    PrincipalCommit { kind: PrincipalKind::Agent, commit: CanonDigest([0x42; 32]), on_behalf_of_commit: None }
}

// ---------------------------------------------------------------------------
// AC-1: Single IDL source generates WIT & message schema; drift fails `abi_generated_out_of_date`

#[test]
fn ac1_generated_abi_freshness_and_drift_detection() {
    check_generated_freshness(COMMITTED_IDL, COMMITTED_WIT, COMMITTED_MSG_PROTOCOL).unwrap();

    let drifted_wit = format!("{COMMITTED_WIT}// hand edit\n");
    let err = check_generated_freshness(COMMITTED_IDL, &drifted_wit, COMMITTED_MSG_PROTOCOL).unwrap_err();
    assert_eq!(err.code(), "abi_generated_out_of_date");
    assert!(matches!(err, AbiError::GeneratedOutOfDate { artifact: "format-plugin.wit", .. }));

    let drifted_msg = COMMITTED_MSG_PROTOCOL.replace("\"tag\": 8", "\"tag\": 9");
    let err = check_generated_freshness(COMMITTED_IDL, COMMITTED_WIT, &drifted_msg).unwrap_err();
    assert_eq!(err.code(), "abi_generated_out_of_date");
    assert!(matches!(err, AbiError::GeneratedOutOfDate { artifact: "message-protocol.json", .. }));
}

// ---------------------------------------------------------------------------
// AC-2: Major version N+1 fails `plugin_abi_mismatch` naming both versions

#[test]
fn ac2_major_version_mismatch_fails() {
    let desc = load_descriptor("descriptor-major-mismatch.json");
    assert_eq!(desc.contract, ContractVersion::new(2, 0));

    let err = negotiate_version(HOST_CONTRACT_VERSION, desc.contract).unwrap_err();
    assert_eq!(err.code(), "plugin_abi_mismatch");
    let msg = err.to_string();
    assert!(msg.contains("1.0"), "expected host version in message: {msg}");
    assert!(msg.contains("2.0"), "expected plugin version in message: {msg}");

    let mut reg = Registry::new();
    let reg_err = register_descriptor(&mut reg, &desc, Origin::Plugin, HOST_CONTRACT_VERSION).unwrap_err();
    assert_eq!(reg_err.code(), "plugin_abi_mismatch");
    assert!(reg.is_empty(), "rejected plugin must not mutate registry");

    let plugin = ToyPlugin { descriptor: desc };
    let fid = FormatId::new("toy-plugin").unwrap();
    let adapter_err = PluginReaderAdapter::new(plugin, fid, Origin::Plugin, HOST_CONTRACT_VERSION).unwrap_err();
    assert_eq!(adapter_err.code(), "plugin_abi_mismatch");
}

// ---------------------------------------------------------------------------
// AC-3: Minor version negotiation (plugin minor <= host accepted; > host rejected)

#[test]
fn ac3_minor_version_negotiation() {
    let host_v1_1 = ContractVersion::new(1, 1);

    // Plugin minor 0 with host minor 1 -> loads
    let desc_v1_0 = load_descriptor("descriptor-v1-0.json");
    assert_eq!(desc_v1_0.contract, ContractVersion::new(1, 0));
    negotiate_version(host_v1_1, desc_v1_0.contract).unwrap();
    let mut reg = Registry::new();
    register_descriptor(&mut reg, &desc_v1_0, Origin::Plugin, host_v1_1).unwrap();
    let fid = FormatId::new("toy-plugin").unwrap();
    let _adapter =
        PluginReaderAdapter::new(ToyPlugin { descriptor: desc_v1_0 }, fid.clone(), Origin::Plugin, host_v1_1).unwrap();

    // Plugin minor 2 with host minor 1 -> plugin_abi_mismatch
    let desc_v1_2 = load_descriptor("descriptor-minor-too-high.json");
    assert_eq!(desc_v1_2.contract, ContractVersion::new(1, 2));
    let err = negotiate_version(host_v1_1, desc_v1_2.contract).unwrap_err();
    assert_eq!(err.code(), "plugin_abi_mismatch");
    let msg = err.to_string();
    assert!(msg.contains("1.1"), "expected host version in message: {msg}");
    assert!(msg.contains("1.2"), "expected plugin version in message: {msg}");

    let mut reg2 = Registry::new();
    let reg_err = register_descriptor(&mut reg2, &desc_v1_2, Origin::Plugin, host_v1_1).unwrap_err();
    assert_eq!(reg_err.code(), "plugin_abi_mismatch");
    let adapter_err =
        PluginReaderAdapter::new(ToyPlugin { descriptor: desc_v1_2 }, fid, Origin::Plugin, host_v1_1).unwrap_err();
    assert_eq!(adapter_err.code(), "plugin_abi_mismatch");
}

// ---------------------------------------------------------------------------
// AC-4: In-process reader registered, shown by import.list_formats@1, and runs
//       through import.file@1 with every string typed untrusted

#[test]
fn ac4_in_process_plugin_through_import_file_and_list_formats() {
    let plugin = ToyPlugin::from_fixture("descriptor-v1-0.json");
    let desc = FormatPluginReader::descriptor(&plugin).clone();
    let fid = FormatId::new("toy-plugin").unwrap();

    // Register with origin `plugin` (REQ-005).
    let mut reg = Registry::new();
    register_descriptor(&mut reg, &desc, Origin::Plugin, HOST_CONTRACT_VERSION).unwrap();

    // `import.list_formats@1` lists the format with origin `plugin`.
    let listed = list_formats(&reg, Direction::Import, OriginFilter::Any).unwrap();
    assert_eq!(listed.event.command.as_str(), IMPORT_LIST_FORMATS);
    let CValue::Arr(items) = list_formats_value(&reg, Direction::Import, OriginFilter::Plugin) else {
        panic!("expected array from list_formats_value");
    };
    assert_eq!(items.len(), 1);
    let CValue::Obj(first) = &items[0] else {
        panic!("expected descriptor object");
    };
    assert_eq!(first.get("id"), Some(&CValue::Str("toy-plugin".into())));
    assert_eq!(first.get("origin"), Some(&CValue::Str("plugin".into())));

    // First-party contract readers can register with origin `first_party` (REQ-005).
    let mut fp_reg = Registry::new();
    register_descriptor(&mut fp_reg, &desc, Origin::FirstParty, HOST_CONTRACT_VERSION).unwrap();
    let CValue::Arr(fp_items) = list_formats_value(&fp_reg, Direction::Import, OriginFilter::FirstParty) else {
        panic!("expected array");
    };
    assert_eq!(fp_items.len(), 1);

    // Run `import.file@1` with `PluginReaderAdapter` on `sample.tplg`.
    let tmp = TmpDir::new("ac4");
    let sample_bytes = fs::read(format!("{FIXTURE_DIR}sample.tplg")).unwrap();
    tmp.write_in("sample.tplg", &sample_bytes);
    let roots = tmp.roots();
    let (mut tree, ns) = test_tree();

    let adapter = PluginReaderAdapter::new(plugin, fid.clone(), Origin::Plugin, HOST_CONTRACT_VERSION).unwrap();
    assert_eq!(adapter.origin(), Origin::Plugin);

    // Also inspect records directly to confirm every returned string is `UntrustedText`.
    let mut cursor = std::io::Cursor::new(&sample_bytes);
    let empty_opts = CValue::Obj(BTreeMap::new());
    let mut req = ReadRequest { format_id: &fid, options: &empty_opts, stream: &mut cursor };
    let direct_records = adapter.plugin().read(&mut req).unwrap();
    assert_eq!(direct_records.len(), 3);
    let all_strings: Vec<&UntrustedText> = direct_records.iter().flat_map(Record::untrusted_strings).collect();
    assert!(all_strings.contains(&&UntrustedText::new("datum_plane")));
    assert!(all_strings.contains(&&UntrustedText::new("inspection_notes")));
    assert!(all_strings.contains(&&UntrustedText::new("hello")));
    assert!(all_strings.contains(&&UntrustedText::new("sample_note")));

    let mut cmd = ImportFile::new();
    cmd.add(Box::new(adapter)).unwrap();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, test_commit(), &tree, ns);
    let mut blobs = MemoryBlobs::default();
    let prepared =
        cmd.prepare(&env, &ImportParams::new("root://in/sample.tplg"), Some(&mut blobs as &mut dyn BlobSink)).unwrap();

    assert_eq!(prepared.report().warnings().len(), 1);
    assert_eq!(prepared.report().warnings()[0].code.as_str(), "sample_note");
    assert_eq!(prepared.event().entities.len(), 2);

    let outcome = prepared.commit(&mut tree).unwrap();
    let total_entities: u64 = outcome.report.entity_counts().values().sum();
    assert_eq!(total_entities, 2);

    // Untrusted string containing a forbidden control character is quarantined by `fid-io-reg`
    // (replaced by placeholder name + `name_forbidden_character` warning, CONSTITUTION C6).
    let hostile = b"TOYPLG\nUNIT mm\nPOINTS bad\x07name 1 2 3\n";
    tmp.write_in("hostile.tplg", hostile);
    let env2 = ImportEnv::new(&reg, &roots, Principal::Agent, test_commit(), &tree, ns);
    let prepared_hostile = cmd
        .prepare(&env2, &ImportParams::new("root://in/hostile.tplg"), Some(&mut blobs as &mut dyn BlobSink))
        .unwrap();
    assert!(
        prepared_hostile.report().warnings().iter().any(|w| w.code.as_str() == "name_forbidden_character"),
        "expected name_forbidden_character warning for untrusted control char"
    );

    // Bounded stream enforcement (REQ-006): byte limit below file length fails `limit_exceeded`.
    let mut tight_env = ImportEnv::new(&reg, &roots, Principal::Agent, test_commit(), &tree, ns);
    tight_env.limits = LimitRequest { bytes: Some(8), ..LimitRequest::default() };
    let lim_err = cmd.prepare(&tight_env, &ImportParams::new("root://in/sample.tplg"), None).unwrap_err();
    assert_eq!(lim_err.code(), "limit_exceeded");
}

// ---------------------------------------------------------------------------
// AC-5: Capability audit rejects file, socket/network, clock, or document caps

#[test]
fn ac5_capability_audit_forbids_fs_net_clock_document() {
    audit_capabilities(COMMITTED_IDL, COMMITTED_WIT, COMMITTED_MSG_PROTOCOL).unwrap();

    let wit_with_fs =
        COMMITTED_WIT.replace("world format-plugin {\n", "world format-plugin {\n    import wasi:filesystem/types;\n");
    let err = audit_capabilities(COMMITTED_IDL, &wit_with_fs, COMMITTED_MSG_PROTOCOL).unwrap_err();
    assert_eq!(err.code(), "capability_violation");

    let idl_with_socket = COMMITTED_IDL.replace("\"imports\": []", "\"imports\": [\"wasi:sockets/tcp\"]");
    let err = audit_capabilities(&idl_with_socket, COMMITTED_WIT, COMMITTED_MSG_PROTOCOL).unwrap_err();
    assert_eq!(err.code(), "capability_violation");

    let wit_with_clock = COMMITTED_WIT.replace("export plugin;", "export plugin;\n    // ok\n    clock-read: func();");
    let err = audit_capabilities(COMMITTED_IDL, &wit_with_clock, COMMITTED_MSG_PROTOCOL).unwrap_err();
    assert_eq!(err.code(), "capability_violation");

    let wit_with_doc = COMMITTED_WIT.replace("export plugin;", "export plugin;\n    document-handle: func();");
    let err = audit_capabilities(COMMITTED_IDL, &wit_with_doc, COMMITTED_MSG_PROTOCOL).unwrap_err();
    assert_eq!(err.code(), "capability_violation");
}

// ---------------------------------------------------------------------------
// Additional coverage: all 6 Record variants + Writer adapter + descriptor roundtrip

struct AllRecordsPlugin {
    descriptor: PluginDescriptor,
}

impl FormatPluginReader for AllRecordsPlugin {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn read(&self, req: &mut ReadRequest<'_>) -> Result<Vec<Record>, PluginError> {
        let mut sink_buf = Vec::new();
        req.stream.read_to_end(&mut sink_buf).map_err(|e| PluginError::malformed(e.to_string(), None))?;
        Ok(vec![
            Record::Points(PointsRecord {
                name: UntrustedText::new("pts_a"),
                points: vec![Point3 { x: 1.0, y: 2.0, z: 3.0 }],
                unit: Some(UntrustedText::new("mm")),
                attributes: vec![KvEntry::new("probe", "tp20")],
            }),
            Record::Mesh(MeshRecord {
                name: UntrustedText::new("mesh_a"),
                vertices: vec![
                    Point3 { x: 0.0, y: 0.0, z: 0.0 },
                    Point3 { x: 1.0, y: 0.0, z: 0.0 },
                    Point3 { x: 0.0, y: 1.0, z: 0.0 },
                ],
                triangles: vec![TriIndices { a: 0, b: 1, c: 2 }],
                unit: Some(UntrustedText::new("mm")),
                attributes: vec![],
            }),
            Record::QifJson(QifJsonRecord {
                name: UntrustedText::new("qif_a"),
                fragment_json: UntrustedText::new(r#"{"QIFDocument":{"version":"3.0"}}"#),
            }),
            Record::DrawingPage(DrawingPageRecord {
                name: UntrustedText::new("sheet_1"),
                page_index: 0,
                width_mm: 297.0,
                height_mm: 210.0,
                dpi: 300,
                media_type: UntrustedText::new("image/png"),
                image_bytes: vec![0x89, 0x50, 0x4e, 0x47],
                metadata: vec![KvEntry::new("sheet", "1/1")],
            }),
            Record::Table(TableRecord {
                name: UntrustedText::new("bom_1"),
                columns: vec![UntrustedText::new("item"), UntrustedText::new("qty")],
                rows: vec![vec![UntrustedText::new("bolt"), UntrustedText::new("4")]],
            }),
            Record::Warning(WarningRecord {
                code: UntrustedText::new("INVALID CODE!!"),
                detail: UntrustedText::new("fallback warning code used"),
                byte_offset: Some(4),
            }),
        ])
    }
}

#[test]
fn all_record_variants_writer_adapter_and_descriptor_roundtrip() {
    let desc = load_descriptor("descriptor-v1-0.json");
    let canon = desc.to_canonical_json().unwrap();
    assert_eq!(PluginDescriptor::from_json(&canon).unwrap(), desc);

    let fid = FormatId::new("toy-plugin").unwrap();
    let mut reg = Registry::new();
    register_descriptor(&mut reg, &desc, Origin::FirstParty, HOST_CONTRACT_VERSION).unwrap();

    let tmp = TmpDir::new("all_records");
    tmp.write_in("all.tplg", b"TOYPLG\n");
    let roots = tmp.roots();
    let (mut tree, ns) = test_tree();

    let adapter = PluginReaderAdapter::new(
        AllRecordsPlugin { descriptor: desc.clone() },
        fid.clone(),
        Origin::FirstParty,
        HOST_CONTRACT_VERSION,
    )
    .unwrap();
    let mut cmd = ImportFile::new();
    cmd.add(Box::new(adapter)).unwrap();
    let env = ImportEnv::new(&reg, &roots, Principal::Agent, test_commit(), &tree, ns);
    let mut blobs = MemoryBlobs::default();
    let prepared =
        cmd.prepare(&env, &ImportParams::new("root://in/all.tplg"), Some(&mut blobs as &mut dyn BlobSink)).unwrap();
    assert_eq!(prepared.event().entities.len(), 5);
    assert_eq!(prepared.report().warnings().len(), 1);
    assert_eq!(prepared.report().warnings()[0].code.as_str(), "plugin_warning");
    prepared.commit(&mut tree).unwrap();

    // Writer adapter roundtrip through `fid_io_reg::env::export::Writer`.
    let writer = PluginWriterAdapter::new(ToyPlugin { descriptor: desc }, fid, HOST_CONTRACT_VERSION).unwrap();
    let entities = vec![WriteEntity {
        type_id: ENTITY_TYPE_POINTS.into(),
        name: UntrustedText::new("datum_plane"),
        payload: b"{}".to_vec(),
    }];
    let mut sink = CountingSink::new(1024);
    let mut report = ReportBuilder::new();
    writer.write(&entities, &CValue::Obj(BTreeMap::new()), &mut sink, &mut report).unwrap();
    assert!(sink.bytes_written() > 0);
}
