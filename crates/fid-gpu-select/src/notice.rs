//! Fallback and workaround notice builder (spec node `PLAT.GPU.NOTICE`).
//!
//! Pure content builder (`D0`, `R0`, no I/O, no panics, no `unsafe`) that turns a
//! [`GpuSelection`] and the fallback tier table ([`GpuFallback`], parsed from
//! `platform/gpu-fallback.toml`) into a persistent [`RenderNotice`] whenever the app runs
//! on a fallback tier or with an applied known-bad-driver entry or workaround.
//!
//! # Display contract for VIZ (`VIZ.SHELL.DOCK`, REQ-PLAT.GPU.NOTICE-003)
//!
//! Every [`RenderNotice`] carries `collapsible = true` and `dismissible = false`:
//! - VIZ MAY allow the user to collapse the notice body to save viewport space.
//! - VIZ MUST NOT allow the user to dismiss or hide the notice completely.
//! - While a [`RenderNotice`] exists, VIZ MUST keep a persistent collapsed indicator visible
//!   in the shell status area for the entire session so a degraded rendering state or driver
//!   workaround is never hidden from the operator (CONSTITUTION C2: display only; never
//!   changes a value of record).
//! - Adapter-supplied strings (adapter name, driver version, rejection lines) are treated as
//!   untrusted text (CONSTITUTION C6) and stripped of ANSI escape sequences and control
//!   characters before appearing anywhere in [`RenderNotice`].

use crate::GpuSelection;
use fid_canon::CValue;
use fid_gpu_policy::policy::Backend;
use std::collections::BTreeMap;
use std::fmt;

/// Offline documentation link carried by every [`RenderNotice`].
pub const NOTICE_LINK: &str = "docs/hardware.html#fallback";
/// Maximum accepted size of `gpu-fallback.toml` (64 KiB, matching `PLAT.GPU.FALLBK.TIERS`).
pub const MAX_FALLBACK_BYTES: usize = 64 * 1024;
/// Maximum features accepted per fallback tier.
pub const MAX_FEATURES_PER_TIER: usize = 256;

// ------------------------------------------------------------------ fallback tier data

/// Name of a rendering fallback tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TierName {
    Reduced,
    Software,
    Unknown,
}

impl TierName {
    /// Wire string (`"reduced"`, `"software"`, `"unknown"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            TierName::Reduced => "reduced",
            TierName::Software => "software",
            TierName::Unknown => "unknown",
        }
    }

    /// Parses `"reduced"` or `"software"`.
    pub fn parse_known(s: &str) -> Option<Self> {
        match s {
            "reduced" => Some(TierName::Reduced),
            "software" => Some(TierName::Software),
            _ => None,
        }
    }
}

impl fmt::Display for TierName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a fallback tier degrades a view feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EffectKind {
    Disabled,
    Reduced,
}

impl EffectKind {
    /// Wire string (`"disabled"` or `"reduced"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            EffectKind::Disabled => "disabled",
            EffectKind::Reduced => "reduced",
        }
    }

    /// Parses `"disabled"` or `"reduced"`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "disabled" => Some(EffectKind::Disabled),
            "reduced" => Some(EffectKind::Reduced),
            _ => None,
        }
    }
}

impl fmt::Display for EffectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One reduced or disabled view feature in a fallback tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEffect {
    pub id: String,
    pub effect: EffectKind,
    pub note: String,
}

impl FeatureEffect {
    fn to_cvalue(&self) -> CValue {
        CValue::Obj(BTreeMap::from([
            ("effect".to_string(), s(self.effect.as_str())),
            ("id".to_string(), s(&self.id)),
            ("note".to_string(), s(&self.note)),
        ]))
    }
}

/// Optional software-tier performance floor metadata from `gpu-fallback.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct FallbackFloor {
    pub workload: String,
    pub metric: String,
    pub min: f64,
}

/// One tier definition from `gpu-fallback.toml`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FallbackTier {
    pub features: Vec<FeatureEffect>,
    pub floor: Option<FallbackFloor>,
}

/// Parsed `gpu-fallback.toml` artifact (`PLAT.GPU.FALLBK.TIERS`).
#[derive(Debug, Clone, PartialEq)]
pub struct GpuFallback {
    pub schema_version: u32,
    pub tiers: BTreeMap<TierName, FallbackTier>,
}

/// Typed error returned by [`parse_gpu_fallback`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackParseError {
    pub line: usize,
    pub reason: String,
}

impl fmt::Display for FallbackParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "gpu-fallback parse error: {}", self.reason)
        } else {
            write!(f, "gpu-fallback parse error at line {}: {}", self.line, self.reason)
        }
    }
}

impl std::error::Error for FallbackParseError {}

// ------------------------------------------------------------------ RenderNotice types

/// Stable message identifier of a [`RenderNotice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoticeId {
    FallbackSoftware,
    FallbackReduced,
    Workaround,
}

impl NoticeId {
    /// Wire identifier string.
    pub const fn as_str(self) -> &'static str {
        match self {
            NoticeId::FallbackSoftware => "gpu.fallback.software",
            NoticeId::FallbackReduced => "gpu.fallback.reduced",
            NoticeId::Workaround => "gpu.workaround",
        }
    }
}

impl fmt::Display for NoticeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// English human-readable text carried by a [`RenderNotice`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeText {
    pub title: String,
    pub body: String,
}

/// Persistent rendering notice delivered to VIZ (`VIZ.SHELL.DOCK`).
///
/// Always carries `collapsible = true`, `dismissible = false`, and `link = NOTICE_LINK`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderNotice {
    pub id: NoticeId,
    pub tier: Option<TierName>,
    pub reason: String,
    pub features: Vec<FeatureEffect>,
    pub entries: Vec<String>,
    pub workarounds: Vec<String>,
    pub link: &'static str,
    pub collapsible: bool,
    pub dismissible: bool,
    pub text: NoticeText,
}

impl RenderNotice {
    /// Canonical JSON representation with fixed field order (`BTreeMap`).
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("collapsible".to_string(), CValue::Bool(self.collapsible));
        m.insert("dismissible".to_string(), CValue::Bool(self.dismissible));
        m.insert("entries".to_string(), CValue::Arr(self.entries.iter().map(|e| s(e)).collect()));
        m.insert("features".to_string(), CValue::Arr(self.features.iter().map(FeatureEffect::to_cvalue).collect()));
        m.insert("id".to_string(), s(self.id.as_str()));
        m.insert("link".to_string(), s(self.link));
        m.insert("reason".to_string(), s(&self.reason));
        m.insert(
            "text".to_string(),
            CValue::Obj(BTreeMap::from([
                ("body".to_string(), s(&self.text.body)),
                ("title".to_string(), s(&self.text.title)),
            ])),
        );
        m.insert("tier".to_string(), self.tier.map_or(CValue::Null, |t| s(t.as_str())));
        m.insert("workarounds".to_string(), CValue::Arr(self.workarounds.iter().map(|w| s(w)).collect()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes (byte-identical for equal inputs, REQ-005, AC-5).
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }
}

// ------------------------------------------------------------------ untrusted text sanitizer (C6)

/// Sanitizes untrusted adapter-supplied text (CONSTITUTION C6, REQ-005, AC-4):
/// strips ANSI escape sequences (CSI/OSC/two-byte ESC) and removes all control characters,
/// collapsing newlines/tabs to single ASCII spaces.
pub fn sanitize_untrusted(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    // ANSI CSI sequence: ESC '[' <param/intermediate 0x20..=0x3F>* <final 0x40..=0x7E>
                    let _ = chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // ANSI OSC sequence: terminated by BEL (\x07) or ST (ESC \)
                    let _ = chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            let _ = chars.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    // Skip one byte following ESC.
                    let _ = chars.next();
                }
                None => {}
            }
            continue;
        }
        if matches!(ch, '\n' | '\r' | '\t') {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            continue;
        }
        if ch.is_control() {
            continue;
        }
        out.push(ch);
    }
    out.trim().to_string()
}

// ------------------------------------------------------------------ notice builder

/// Builds a persistent [`RenderNotice`] when `selection` is running on a fallback tier or
/// has applied known-bad entries or workarounds; returns `None` otherwise
/// (REQ-PLAT.GPU.NOTICE-001..005).
pub fn build_notice(selection: &GpuSelection, tiers: &GpuFallback) -> Option<RenderNotice> {
    let requested_tier = if selection.fallback {
        Some(if selection.backend == Backend::Software { TierName::Software } else { TierName::Reduced })
    } else {
        None
    };
    build_notice_for_tier(selection, requested_tier, tiers)
}

/// Variant of [`build_notice`] that accepts an explicit fallback tier override (for example
/// `Some(TierName::Reduced)` or `Some(TierName::Unknown)` when a hardware tier is degraded).
pub fn build_notice_for_tier(
    selection: &GpuSelection,
    requested_tier: Option<TierName>,
    tiers: &GpuFallback,
) -> Option<RenderNotice> {
    let has_fallback = selection.fallback || requested_tier.is_some();
    let entries: Vec<String> = selection.known_bad.iter().map(|e| sanitize_untrusted(e)).collect();
    let workarounds: Vec<String> = selection.workarounds.iter().map(|w| sanitize_untrusted(w)).collect();

    if !has_fallback && entries.is_empty() && workarounds.is_empty() {
        return None;
    }

    let reason = sanitize_reason(selection);

    if has_fallback {
        let want_tier = requested_tier.unwrap_or_else(|| {
            if selection.backend == Backend::Software {
                TierName::Software
            } else {
                TierName::Reduced
            }
        });
        let (tier, features) = match tiers.tiers.get(&want_tier) {
            Some(def) => (
                want_tier,
                def.features
                    .iter()
                    .map(|f| FeatureEffect {
                        id: sanitize_untrusted(&f.id),
                        effect: f.effect,
                        note: sanitize_untrusted(&f.note),
                    })
                    .collect::<Vec<_>>(),
            ),
            None => (TierName::Unknown, Vec::new()),
        };

        let id = match tier {
            TierName::Reduced => NoticeId::FallbackReduced,
            TierName::Software | TierName::Unknown => NoticeId::FallbackSoftware,
        };
        let title = match tier {
            TierName::Software => "Software rendering fallback active".to_string(),
            TierName::Reduced => "Reduced-feature rendering fallback active".to_string(),
            TierName::Unknown => "Degraded rendering fallback active".to_string(),
        };

        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("Rendering is using the {} fallback tier.", tier.as_str()));
        if !reason.is_empty() {
            parts.push(format!("Reason: {reason}."));
        }
        if !features.is_empty() {
            let summary: Vec<String> = features.iter().map(|f| format!("{} ({})", f.id, f.effect.as_str())).collect();
            parts.push(format!("Reduced or disabled features: {}.", summary.join(", ")));
        }
        if !entries.is_empty() {
            parts.push(format!("Applied known-bad driver entries: {}.", entries.join(", ")));
        }
        if !workarounds.is_empty() {
            parts.push(format!("Active workarounds: {}.", workarounds.join(", ")));
        }
        parts.push(format!("See {NOTICE_LINK}."));

        return Some(RenderNotice {
            id,
            tier: Some(tier),
            reason,
            features,
            entries,
            workarounds,
            link: NOTICE_LINK,
            collapsible: true,
            dismissible: false,
            text: NoticeText { title, body: parts.join(" ") },
        });
    }

    // Workaround / known-bad entry applied on a non-fallback hardware backend (AC-2:
    // never mentions "fallback" as a tier).
    let title = "Graphics driver workaround active".to_string();
    let mut parts: Vec<String> = Vec::new();
    match &selection.adapter {
        Some(a) => {
            let name = sanitize_untrusted(&a.name);
            parts.push(format!(
                "Graphics rendering on {} ({}) is running with driver mitigations.",
                selection.backend.as_str(),
                name
            ));
        }
        None => {
            parts.push(format!(
                "Graphics rendering on {} is running with driver mitigations.",
                selection.backend.as_str()
            ));
        }
    }
    if !entries.is_empty() {
        parts.push(format!("Known-bad driver entries applied: {}.", entries.join(", ")));
    }
    if !workarounds.is_empty() {
        parts.push(format!("Workarounds enabled: {}.", workarounds.join(", ")));
    }
    if !reason.is_empty() {
        parts.push(format!("Detail: {reason}."));
    }
    parts.push(format!("Guidance: {NOTICE_LINK}."));

    Some(RenderNotice {
        id: NoticeId::Workaround,
        tier: None,
        reason,
        features: Vec::new(),
        entries,
        workarounds,
        link: NOTICE_LINK,
        collapsible: true,
        dismissible: false,
        text: NoticeText { title, body: parts.join(" ") },
    })
}

fn sanitize_reason(selection: &GpuSelection) -> String {
    let lines: Vec<String> = selection.reason.lines().map(sanitize_untrusted).filter(|l| !l.is_empty()).collect();
    lines.join("; ")
}

// ------------------------------------------------------------------ bounded gpu-fallback.toml parser

/// Parses `gpu-fallback.toml` (schema 1, capped at 64 KiB, zero third-party deps).
///
/// Accepts:
/// ```toml
/// schema_version = 1
///
/// [[tiers.reduced.features]]
/// id = "msaa"
/// effect = "reduced"
/// note = "2x MSAA instead of 4x"
///
/// [[tiers.software.features]]
/// id = "shadows"
/// effect = "disabled"
/// note = "Shadows disabled in software rendering"
///
/// [tiers.software.floor]
/// workload = "render_software_1m"
/// metric = "fps"
/// min = 5.0
/// ```
pub fn parse_gpu_fallback(bytes: &[u8]) -> Result<GpuFallback, FallbackParseError> {
    if bytes.len() > MAX_FALLBACK_BYTES {
        return Err(FallbackParseError { line: 0, reason: format!("exceeds {MAX_FALLBACK_BYTES} bytes") });
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FallbackParseError { line: 0, reason: "file is not valid UTF-8".to_string() })?;

    #[derive(Clone, Copy)]
    enum Section {
        Top,
        Feature,
        Floor,
    }

    struct PendingFeature {
        tier: TierName,
        id: Option<String>,
        effect: Option<EffectKind>,
        note: Option<String>,
        start_line: usize,
    }

    struct PendingFloor {
        tier: TierName,
        workload: Option<String>,
        metric: Option<String>,
        min: Option<f64>,
        start_line: usize,
    }

    let mut schema_version: Option<u32> = None;
    let mut tiers: BTreeMap<TierName, FallbackTier> = BTreeMap::new();
    let mut section = Section::Top;
    let mut cur_feature: Option<PendingFeature> = None;
    let mut cur_floor: Option<PendingFloor> = None;

    let flush_feature = |slot: &mut Option<PendingFeature>,
                         tiers: &mut BTreeMap<TierName, FallbackTier>|
     -> Result<(), FallbackParseError> {
        let Some(PendingFeature { tier, id, effect, note, start_line }) = slot.take() else {
            return Ok(());
        };
        let id =
            id.ok_or_else(|| FallbackParseError { line: start_line, reason: "feature missing `id`".to_string() })?;
        let effect = effect
            .ok_or_else(|| FallbackParseError { line: start_line, reason: "feature missing `effect`".to_string() })?;
        let note =
            note.ok_or_else(|| FallbackParseError { line: start_line, reason: "feature missing `note`".to_string() })?;
        let entry = tiers.entry(tier).or_default();
        if entry.features.len() >= MAX_FEATURES_PER_TIER {
            return Err(FallbackParseError {
                line: start_line,
                reason: format!("tier {} exceeds {MAX_FEATURES_PER_TIER} features", tier.as_str()),
            });
        }
        entry.features.push(FeatureEffect { id, effect, note });
        Ok(())
    };

    let flush_floor = |slot: &mut Option<PendingFloor>,
                       tiers: &mut BTreeMap<TierName, FallbackTier>|
     -> Result<(), FallbackParseError> {
        let Some(PendingFloor { tier, workload, metric, min, start_line }) = slot.take() else {
            return Ok(());
        };
        let workload = workload
            .ok_or_else(|| FallbackParseError { line: start_line, reason: "floor missing `workload`".to_string() })?;
        let metric = metric
            .ok_or_else(|| FallbackParseError { line: start_line, reason: "floor missing `metric`".to_string() })?;
        let min =
            min.ok_or_else(|| FallbackParseError { line: start_line, reason: "floor missing `min`".to_string() })?;
        tiers.entry(tier).or_default().floor = Some(FallbackFloor { workload, metric, min });
        Ok(())
    };

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(inner) = line.strip_prefix("[[").and_then(|s| s.strip_suffix("]]")) {
            flush_feature(&mut cur_feature, &mut tiers)?;
            flush_floor(&mut cur_floor, &mut tiers)?;
            let header = inner.trim();
            match header {
                "tiers.reduced.features" => {
                    section = Section::Feature;
                    cur_feature = Some(PendingFeature {
                        tier: TierName::Reduced,
                        id: None,
                        effect: None,
                        note: None,
                        start_line: line_no,
                    });
                }
                "tiers.software.features" => {
                    section = Section::Feature;
                    cur_feature = Some(PendingFeature {
                        tier: TierName::Software,
                        id: None,
                        effect: None,
                        note: None,
                        start_line: line_no,
                    });
                }
                _ => {
                    return Err(FallbackParseError {
                        line: line_no,
                        reason: format!("unknown array-table header `[[{header}]]`"),
                    });
                }
            }
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            flush_feature(&mut cur_feature, &mut tiers)?;
            flush_floor(&mut cur_floor, &mut tiers)?;
            let header = inner.trim();
            match header {
                "tiers.software.floor" => {
                    section = Section::Floor;
                    cur_floor = Some(PendingFloor {
                        tier: TierName::Software,
                        workload: None,
                        metric: None,
                        min: None,
                        start_line: line_no,
                    });
                }
                "tiers.reduced.floor" => {
                    section = Section::Floor;
                    cur_floor = Some(PendingFloor {
                        tier: TierName::Reduced,
                        workload: None,
                        metric: None,
                        min: None,
                        start_line: line_no,
                    });
                }
                "tiers.reduced" => {
                    tiers.entry(TierName::Reduced).or_default();
                    section = Section::Top;
                }
                "tiers.software" => {
                    tiers.entry(TierName::Software).or_default();
                    section = Section::Top;
                }
                _ => {
                    return Err(FallbackParseError {
                        line: line_no,
                        reason: format!("unknown table header `[{header}]`"),
                    });
                }
            }
            continue;
        }

        let Some((k, v)) = line.split_once('=') else {
            return Err(FallbackParseError { line: line_no, reason: "expected `key = value`".to_string() });
        };
        let key = k.trim();
        let val = v.trim();
        match section {
            Section::Top => {
                if key == "schema_version" {
                    let parsed: u32 = val.parse().map_err(|_| FallbackParseError {
                        line: line_no,
                        reason: "schema_version must be an integer".to_string(),
                    })?;
                    schema_version = Some(parsed);
                } else {
                    return Err(FallbackParseError { line: line_no, reason: format!("unknown top-level key `{key}`") });
                }
            }
            Section::Feature => {
                let Some(pending) = cur_feature.as_mut() else {
                    return Err(FallbackParseError { line: line_no, reason: "internal parser state".to_string() });
                };
                match key {
                    "id" => pending.id = Some(parse_toml_string(val, line_no)?),
                    "effect" => {
                        let s = parse_toml_string(val, line_no)?;
                        pending.effect = Some(EffectKind::parse(&s).ok_or_else(|| FallbackParseError {
                            line: line_no,
                            reason: format!("invalid effect `{s}`; expected `disabled` or `reduced`"),
                        })?);
                    }
                    "note" => pending.note = Some(parse_toml_string(val, line_no)?),
                    _ => {
                        return Err(FallbackParseError {
                            line: line_no,
                            reason: format!("unknown feature key `{key}`"),
                        });
                    }
                }
            }
            Section::Floor => {
                let Some(pending) = cur_floor.as_mut() else {
                    return Err(FallbackParseError { line: line_no, reason: "internal parser state".to_string() });
                };
                match key {
                    "workload" => pending.workload = Some(parse_toml_string(val, line_no)?),
                    "metric" => pending.metric = Some(parse_toml_string(val, line_no)?),
                    "min" => {
                        let n: f64 = val.parse().map_err(|_| FallbackParseError {
                            line: line_no,
                            reason: "floor.min must be a finite number".to_string(),
                        })?;
                        if !n.is_finite() || n < 0.0 {
                            return Err(FallbackParseError {
                                line: line_no,
                                reason: "floor.min must be finite and >= 0".to_string(),
                            });
                        }
                        pending.min = Some(n);
                    }
                    _ => {
                        return Err(FallbackParseError { line: line_no, reason: format!("unknown floor key `{key}`") });
                    }
                }
            }
        }
    }

    flush_feature(&mut cur_feature, &mut tiers)?;
    flush_floor(&mut cur_floor, &mut tiers)?;

    let schema_version =
        schema_version.ok_or_else(|| FallbackParseError { line: 0, reason: "missing `schema_version`".to_string() })?;
    if schema_version != 1 {
        return Err(FallbackParseError {
            line: 0,
            reason: format!("unsupported schema_version {schema_version}; expected 1"),
        });
    }
    Ok(GpuFallback { schema_version, tiers })
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_quotes = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_quotes = false;
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == '#' {
            return &line[..idx];
        }
    }
    line
}

fn parse_toml_string(raw: &str, line: usize) -> Result<String, FallbackParseError> {
    let Some(inner) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return Err(FallbackParseError { line, reason: format!("expected quoted string, got `{raw}`") });
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                other => {
                    return Err(FallbackParseError {
                        line,
                        reason: format!("unsupported string escape `\\{other:?}`"),
                    });
                }
            }
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}
