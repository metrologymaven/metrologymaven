//! Journal events of the two W2 template commands.
//!
//! ```text
//! report.save_template@1   {type, template: {builtin: false, digest, id, version},
//!                           source: {kind: "report", report, report_digest}
//!                                 | {kind: "file", handle: {text, untrusted: true}}}
//! report.apply_template@1  {type, report, report_digest, template: {builtin, digest, id, version}, title}
//! ```
//!
//! All keys are required; integers are decimal strings. A save event names its source so
//! exact replay can rebuild a template saved from a report and compare digests; a file's
//! bytes are only in the content-addressed store (the file itself is never read again).
//! An apply event holds the report ID it drew, so replay draws nothing.

use crate::doc::model::{exact_obj, field};
use crate::doc::{ReportError, ReportId, TemplateRef, Text, Untrusted, MAX_REPORT_BYTES};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;

/// `report.save_template@1`.
pub const SAVE_TEMPLATE: &str = "report.save_template@1";
/// `report.apply_template@1`.
pub const APPLY_TEMPLATE: &str = "report.apply_template@1";
/// `report.show_template@1` (R0; journals nothing).
pub const SHOW_TEMPLATE: &str = "report.show_template@1";
/// `report.list_template@1` (R0; journals nothing).
pub const LIST_TEMPLATE: &str = "report.list_template@1";

/// Where a saved template came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveSource {
    /// A stored report, with the digest of its stored form at the save.
    Report { report: ReportId, report_digest: Digest },
    /// A template file named by an import-root handle (or, for a human, a raw path), as
    /// given. Untrusted text.
    File { handle: Untrusted<Text> },
}

/// One template event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateEvent {
    /// A new template version (REQ-001).
    Save { template: TemplateRef, source: SaveSource },
    /// A report created from a template version (REQ-004).
    Apply { report: ReportId, title: Untrusted<Text>, template: TemplateRef, report_digest: Digest },
}

fn digest_of(v: &CValue, what: &'static str) -> Result<Digest, ReportError> {
    match v {
        CValue::Str(s) => Digest::parse(s).map_err(|_| ReportError::StoreInvalid(what)),
        _ => Err(ReportError::StoreInvalid(what)),
    }
}

impl TemplateEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            TemplateEvent::Save { .. } => SAVE_TEMPLATE,
            TemplateEvent::Apply { .. } => APPLY_TEMPLATE,
        }
    }

    /// The template version the event names.
    pub fn template(&self) -> TemplateRef {
        match self {
            TemplateEvent::Save { template, .. } | TemplateEvent::Apply { template, .. } => *template,
        }
    }

    /// Whether `ty` is the type of a template event.
    pub fn is_template_type(ty: &str) -> bool {
        ty == SAVE_TEMPLATE || ty == APPLY_TEMPLATE
    }

    /// The journal payload (canonical JSON value).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("type".to_string(), CValue::Str(self.event_type().to_string()));
        o.insert("template".to_string(), self.template().to_value());
        match self {
            TemplateEvent::Save { source, .. } => {
                let mut s = BTreeMap::new();
                match source {
                    SaveSource::Report { report, report_digest } => {
                        s.insert("kind".to_string(), CValue::Str("report".to_string()));
                        s.insert("report".to_string(), CValue::Str(report.uuid().to_string()));
                        s.insert("report_digest".to_string(), CValue::Str(report_digest.as_str().to_string()));
                    }
                    SaveSource::File { handle } => {
                        s.insert("handle".to_string(), handle.to_value());
                        s.insert("kind".to_string(), CValue::Str("file".to_string()));
                    }
                }
                o.insert("source".to_string(), CValue::Obj(s));
            }
            TemplateEvent::Apply { report, title, report_digest, .. } => {
                o.insert("report".to_string(), CValue::Str(report.uuid().to_string()));
                o.insert("report_digest".to_string(), CValue::Str(report_digest.as_str().to_string()));
                o.insert("title".to_string(), title.to_value());
            }
        }
        CValue::Obj(o)
    }

    /// Reads an event from JSON bytes: bounded parse (duplicate keys refused), then
    /// [`TemplateEvent::from_value`]. Any failure is `report.store_invalid`.
    pub fn from_json(bytes: &[u8]) -> Result<TemplateEvent, ReportError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_REPORT_BYTES, fid_canon::MAX_DEPTH)
            .map_err(|_| ReportError::StoreInvalid("event is not json"))?;
        TemplateEvent::from_value(&v)
    }

    /// Strict reader of [`TemplateEvent::to_value`] (`report.store_invalid` on any
    /// deviation, unknown keys included).
    pub fn from_value(v: &CValue) -> Result<TemplateEvent, ReportError> {
        let CValue::Obj(raw) = v else { return Err(ReportError::StoreInvalid("event is not an object")) };
        let ty = match raw.get("type") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(ReportError::StoreInvalid("event type")),
        };
        match ty {
            SAVE_TEMPLATE => {
                let o = exact_obj(v, &["type", "template", "source"])?;
                let template = TemplateRef::from_value(field(o, "template")?)?;
                if template.builtin {
                    return Err(ReportError::StoreInvalid("a save names a built-in version"));
                }
                let src = field(o, "source")?;
                let CValue::Obj(so) = src else { return Err(ReportError::StoreInvalid("source")) };
                let source = match so.get("kind") {
                    Some(CValue::Str(k)) if k == "report" => {
                        let so = exact_obj(src, &["kind", "report", "report_digest"])?;
                        SaveSource::Report {
                            report: ReportId::from_value(field(so, "report")?)?,
                            report_digest: digest_of(field(so, "report_digest")?, "report_digest")?,
                        }
                    }
                    Some(CValue::Str(k)) if k == "file" => {
                        let so = exact_obj(src, &["handle", "kind"])?;
                        SaveSource::File { handle: Untrusted::from_value(field(so, "handle")?)? }
                    }
                    _ => return Err(ReportError::StoreInvalid("source kind")),
                };
                Ok(TemplateEvent::Save { template, source })
            }
            APPLY_TEMPLATE => {
                let o = exact_obj(v, &["type", "report", "report_digest", "template", "title"])?;
                Ok(TemplateEvent::Apply {
                    report: ReportId::from_value(field(o, "report")?)?,
                    title: Untrusted::from_value(field(o, "title")?)?,
                    template: TemplateRef::from_value(field(o, "template")?)?,
                    report_digest: digest_of(field(o, "report_digest")?, "report_digest")?,
                })
            }
            _ => Err(ReportError::StoreInvalid("unknown event type")),
        }
    }
}
