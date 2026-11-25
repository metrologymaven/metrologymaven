//! The rules that need the catalogues, and the per-field rules shared by the loader and
//! [`validate`] (REQ-002 to REQ-008).

use crate::catalogue::namespace_of;
use crate::schema::Draft;
use crate::{
    code, is_gap_text, is_node_id, is_placeholder, is_seed_id, sort_findings, AppManifest, Catalogues, Finding, Gap,
    RegistryView, Status, MAX_STATUS_SCOPE_CHARS, SCHEMA_VERSION,
};
use fid_store::settings::validate_value;
use fid_types::CommandId;
use std::collections::BTreeSet;

/// Validates a manifest value against `catalogues` and returns every finding, sorted
/// (REQ-009). Empty means the app may start.
pub fn validate(manifest: &AppManifest, catalogues: &Catalogues) -> Vec<Finding> {
    let mut f = check(&Draft::from_manifest(manifest), catalogues);
    if manifest.schema != SCHEMA_VERSION {
        f.push(Finding::new(code::SCHEMA, "schema", "not_1"));
    }
    // The parser enforces these shapes; a manifest built in code is held to the same rule.
    for (i, g) in manifest.known_gaps.iter().enumerate() {
        let ok = match g {
            Gap::Seed(s) => is_seed_id(s),
            Gap::Text { text, node } => is_node_id(node) && is_gap_text(text),
        };
        if !ok {
            f.push(Finding::new(code::GAP, format!("known_gaps[{i}]"), ""));
        }
    }
    sort_findings(f)
}

/// Registry commands whose namespace is enabled.
pub(crate) fn effective_commands(namespaces: &BTreeSet<String>, registry: &RegistryView) -> BTreeSet<CommandId> {
    registry.commands().keys().filter(|id| namespaces.contains(namespace_of(id))).cloned().collect()
}

pub(crate) fn check(d: &Draft, c: &Catalogues) -> Vec<Finding> {
    let mut f = Vec::new();

    // REQ-005: the app ID is an [[app]] of apps.toml.
    if let Some(id) = &d.id {
        if !c.apps.contains(id) {
            f.push(Finding::new(code::APP_ID, id.as_str(), ""));
        }
    }

    // REQ-006: status scope (D-017).
    if !d.scope_reported {
        match (&d.status_scope, d.status) {
            (Some(scope), _) => {
                let n = scope.chars().count();
                if scope.trim().is_empty() || n > MAX_STATUS_SCOPE_CHARS {
                    f.push(Finding::new(code::STATUS, "status_scope", "length"));
                } else if scope.chars().any(char::is_control) {
                    f.push(Finding::new(code::STATUS, "status_scope", "control"));
                }
            }
            (None, Some(Status::Alpha | Status::Beta)) => f.push(Finding::new(code::STATUS, "status_scope", "missing")),
            (None, _) => {}
        }
    }

    // REQ-007: seed gaps, when a seed list is supplied.
    if let (Some(seeds), Some(gaps)) = (&c.seeds, &d.known_gaps) {
        for g in gaps {
            if let Gap::Seed(s) = g {
                if !seeds.contains(s) {
                    f.push(Finding::new(code::GAP, s.as_str(), ""));
                }
            }
        }
    }

    // REQ-002 and REQ-004: namespaces and forbidden classes.
    if let Some(ns) = &d.namespaces {
        for n in ns {
            if !c.registry.namespaces().contains(n) {
                f.push(Finding::new(code::UNKNOWN_NS, n.as_str(), ""));
            }
        }
        if !d.forbidden_classes.is_empty() {
            for (id, class) in c.registry.commands() {
                if d.forbidden_classes.contains(class) && ns.contains(namespace_of(id)) {
                    f.push(Finding::new(code::FORBIDDEN_CLASS, id.as_str(), class.as_str()));
                }
            }
        }
    }

    // REQ-002: panels, layout, templates.
    for p in d.panels.iter().flatten() {
        if !c.panels.contains(p) {
            f.push(Finding::new(code::UNKNOWN_PANEL, p.as_str(), ""));
        }
    }
    if let Some(l) = &d.layout {
        if !c.layouts.contains(l) {
            f.push(Finding::new(code::UNKNOWN_LAYOUT, l.as_str(), ""));
        }
    }
    for t in &d.templates {
        if !c.templates.contains(t) {
            f.push(Finding::new(code::UNKNOWN_TEMPLATE, t.as_str(), ""));
        }
    }

    // REQ-002 and REQ-003: defaults.
    for (key, value) in &d.defaults {
        match c.settings.get(key) {
            None => f.push(Finding::new(code::UNKNOWN_SETTING, key.as_str(), "")),
            // An admin setting is changed only by the A0 command settings.set_admin_value@1.
            Some(decl) if decl.admin() => f.push(Finding::new(code::DEFAULT, key.as_str(), "admin")),
            Some(decl) => {
                if validate_value(decl.schema(), value).is_err() {
                    f.push(Finding::new(code::DEFAULT, key.as_str(), "schema"));
                }
            }
        }
    }

    // REQ-008: placeholder branding while D-002 is undecided.
    if d.brand_internal == Some(false) {
        f.push(Finding::new(code::BRANDING, "branding.internal", ""));
    }
    if d.brand_name.as_deref().is_some_and(|s| !is_placeholder(s)) {
        f.push(Finding::new(code::BRANDING, "branding.name", ""));
    }
    if d.brand_icon.as_deref().is_some_and(|s| !is_placeholder(s)) {
        f.push(Finding::new(code::BRANDING, "branding.icon", ""));
    }
    if d.brand_colours.as_ref().is_some_and(|cs| cs.iter().any(|s| !is_placeholder(s))) {
        f.push(Finding::new(code::BRANDING, "branding.colours", ""));
    }
    f
}
