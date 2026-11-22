//! XSD 1.0 schema parser and Raw Schema IR (spec node `IO.QIF.GEN.PARSE`).
//!
//! Reads verified XSD bytes from a [`PinnedSet`] via the hardened [`fid_xml::XmlReader`] under
//! [`fid_io_reg`] limits, resolves `xs:include` and `xs:import` strictly through the pinned set
//! (each file processed at most once), validates every symbol reference in source order, and
//! produces a deterministic [`RawIr`] serialisable as RFC 8785 canonical JSON.

mod ir;
mod json;
mod reader;
mod resolve;

pub use ir::{
    Annotation, AttributeDecl, AttributeGroupDecl, AttributeItem, AttributeType, AttributeUse, ComplexContentModel,
    ComplexTypeDecl, Decl, DeclCounts, DerivationKind, ElementDecl, ElementType, Facet, FacetKind, FileId, FormDefault,
    GroupDecl, IdentityConstraint, IdentityKind, LocalElement, MaxOccurs, Occurs, ParseError, Particle, Pos, QName,
    RawIr, RefKind, SimpleTypeDecl, SimpleTypeVariety, WhiteSpaceMode, XML_NS, XS_NS,
};

use crate::pin::{PinnedSet, Role};
use fid_io_reg::{Budget, LimitRequest, Principal};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Parses all verified XSD files in `pinned` with default `fid-io-reg` reader limits.
pub fn parse_pinned(pinned: &PinnedSet) -> Result<RawIr, ParseError> {
    parse_pinned_with_limits(pinned, &LimitRequest::default())
}

/// Parses all verified XSD files in `pinned` using the given `limits` per file (lowered from the
/// default `IO.REG.LIMITS` bounds as `Principal::Script`).
pub fn parse_pinned_with_limits(pinned: &PinnedSet, limits: &LimitRequest) -> Result<RawIr, ParseError> {
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut parsed_by_file: BTreeMap<FileId, reader::ParsedFile> = BTreeMap::new();

    for f in pinned.files() {
        if f.role() == Role::Xsd && visited.insert(f.path().to_string()) {
            queue.push_back(f.path().to_string());
        }
    }

    while let Some(path) = queue.pop_front() {
        let verified = pinned.resolve(&path)?;
        let file_id = FileId(verified.path().to_string());
        let budget = Budget::new(Principal::Script, limits).map_err(|io_err| ParseError::LimitExceeded {
            detail: io_err.code().to_string(),
            pos: Pos { file: file_id.clone(), line: 1, col: 1 },
        })?;
        let parsed = reader::parse_xsd_file(&file_id, verified.bytes(), budget)?;
        for loc in &parsed.schema_locations {
            let target = pinned.resolve_relative(verified.path(), loc)?;
            if target.role() == Role::Xsd && visited.insert(target.path().to_string()) {
                queue.push_back(target.path().to_string());
            }
        }
        parsed_by_file.insert(file_id, parsed);
    }

    let mut files = Vec::with_capacity(parsed_by_file.len());
    let mut target_namespaces = BTreeMap::new();
    let mut decls = Vec::new();
    for (file_id, parsed) in parsed_by_file {
        files.push(file_id.clone());
        target_namespaces.insert(file_id, parsed.target_namespace);
        decls.extend(parsed.decls);
    }

    let ir = RawIr { files, target_namespaces, decls };
    resolve::check_references(&ir)?;
    Ok(ir)
}
