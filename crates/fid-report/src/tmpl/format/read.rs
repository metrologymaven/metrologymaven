//! The schema-version-1 check: a strict reader from a parsed value to a [`Template`].
//!
//! Order (pinned in the leaf's Design): within each object the key set is checked first
//! (the first unknown key in code-point order, then the first missing key), then the
//! members in code-point key order, depth first; pages and blocks in index order. A page's
//! continuation flags are checked once all its blocks are read.

use super::error::{child, TemplateError};
use super::model::{Clearance, Slot, SlotKind, Template, TemplateBlock, TemplateId, TemplateJson, TemplatePage};
use super::{MAX_PAGES, MAX_REPEAT_HEADER_ROWS, MAX_TEMPLATE_BLOCKS, MEDIA_TYPE, SCHEMA_VERSION};
use crate::doc::Text;
use crate::doc::{BlockId, BlockKindId, PageId, ReportError, TextFault, Untrusted};
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::{BTreeMap, BTreeSet};

type Obj = BTreeMap<String, CValue>;

fn bad(at: &str, reason: &'static str) -> TemplateError {
    TemplateError::invalid(at, reason)
}

/// An object with exactly `keys` (given in code-point order).
pub(crate) fn exact<'a>(v: &'a CValue, at: &str, keys: &[&str]) -> Result<&'a Obj, TemplateError> {
    let CValue::Obj(o) = v else { return Err(bad(at, "wrong_type")) };
    if let Some(k) = o.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(bad(&child(at, k), "unknown_field"));
    }
    if let Some(k) = keys.iter().find(|k| !o.contains_key(**k)) {
        return Err(bad(&child(at, k), "missing_field"));
    }
    Ok(o)
}

/// Member `k` of an object already checked by [`exact`].
pub(crate) fn get<'a>(o: &'a Obj, at: &str, k: &str) -> Result<(&'a CValue, String), TemplateError> {
    let p = child(at, k);
    match o.get(k) {
        Some(v) => Ok((v, p)),
        None => Err(bad(&p, "missing_field")),
    }
}

fn v7(v: &CValue, at: &str) -> Result<Uuid, TemplateError> {
    let CValue::Str(s) = v else { return Err(bad(at, "wrong_type")) };
    Uuid::parse(s).ok().filter(|u| u.to_string() == *s && crate::doc::is_v7(u)).ok_or_else(|| bad(at, "bad_id"))
}

/// A `u32` written as a canonical decimal string (`0` or no leading zero).
pub(crate) fn dec_u32(v: &CValue, at: &str) -> Result<u32, TemplateError> {
    let CValue::Str(s) = v else {
        return Err(bad(at, if matches!(v, CValue::Num(_)) { "number" } else { "wrong_type" }));
    };
    let canonical =
        !s.is_empty() && s.len() <= 10 && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if !canonical {
        return Err(bad(at, "bad_integer"));
    }
    s.parse::<u32>().map_err(|_| bad(at, "out_of_range"))
}

fn untrusted_text(v: &CValue, at: &str) -> Result<Untrusted<Text>, TemplateError> {
    let o = exact(v, at, &["text", "untrusted"])?;
    let (t, tp) = get(o, at, "text")?;
    let (m, mp) = get(o, at, "untrusted")?;
    let CValue::Str(s) = t else { return Err(bad(&tp, "wrong_type")) };
    match Untrusted::text(s) {
        Ok(u) => {
            if m != &CValue::Bool(true) {
                return Err(bad(&mp, "bad_marker"));
            }
            Ok(u)
        }
        Err(ReportError::TextInvalid(TextFault::TooLong)) => Err(bad(&tp, "text_too_long")),
        Err(_) => Err(bad(&tp, "text_nul")),
    }
}

/// `{"untrusted": true, "value": <template json>}`.
fn untrusted_json(v: &CValue, at: &str) -> Result<TemplateJson, TemplateError> {
    let o = exact(v, at, &["untrusted", "value"])?;
    let (m, mp) = get(o, at, "untrusted")?;
    if m != &CValue::Bool(true) {
        return Err(bad(&mp, "bad_marker"));
    }
    let (val, vp) = get(o, at, "value")?;
    TemplateJson::new(val, &vp)
}

fn clearance(v: &CValue, at: &str) -> Result<Option<Clearance>, TemplateError> {
    let o = match v {
        CValue::Null => return Ok(None),
        CValue::Obj(o) => o,
        _ => return Err(bad(at, "wrong_type")),
    };
    let state_at = child(at, "state");
    match o.get("state") {
        Some(CValue::Str(s)) if s == "pending" => {
            exact(v, at, &["state"])?;
            Ok(Some(Clearance::Pending))
        }
        Some(CValue::Str(s)) if s == "cleared" => {
            let o = exact(v, at, &["review_ref", "state"])?;
            let (r, rp) = get(o, at, "review_ref")?;
            Ok(Some(Clearance::Cleared { review_ref: untrusted_text(r, &rp)? }))
        }
        Some(_) => Err(bad(&state_at, "bad_clearance")),
        None => Err(bad(&state_at, "missing_field")),
    }
}

fn slot(v: &CValue, at: &str) -> Result<Option<Slot>, TemplateError> {
    if v == &CValue::Null {
        return Ok(None);
    }
    let o = exact(v, at, &["kind", "spec"])?;
    let (k, kp) = get(o, at, "kind")?;
    let CValue::Str(ks) = k else { return Err(bad(&kp, "wrong_type")) };
    let kind = SlotKind::parse(ks).ok_or_else(|| bad(&kp, "unknown_slot_kind"))?;
    let (s, sp) = get(o, at, "spec")?;
    let spec = untrusted_json(s, &sp)?;
    if !matches!(spec.value(), CValue::Obj(_)) {
        return Err(bad(&child(&sp, "value"), "wrong_type"));
    }
    Ok(Some(Slot { kind, spec }))
}

const BLOCK_KEYS: [&str; 7] =
    ["continuation_block", "id", "kind", "kind_version", "payload", "repeat_header_rows", "slot"];

fn block(v: &CValue, at: &str, seen: &mut BTreeSet<Uuid>) -> Result<TemplateBlock, TemplateError> {
    let o = exact(v, at, &BLOCK_KEYS)?;
    let (c, cp) = get(o, at, "continuation_block")?;
    let continuation_block = match c {
        CValue::Null => None,
        c => Some(BlockId::new(v7(c, &cp)?).ok_or_else(|| bad(&cp, "bad_id"))?),
    };
    let (i, ip) = get(o, at, "id")?;
    let id = BlockId::new(v7(i, &ip)?).ok_or_else(|| bad(&ip, "bad_id"))?;
    if !seen.insert(id.uuid()) {
        return Err(bad(&ip, "repeated_id"));
    }
    let (k, kp) = get(o, at, "kind")?;
    let CValue::Str(ks) = k else { return Err(bad(&kp, "wrong_type")) };
    let kind = BlockKindId::new(ks).ok_or_else(|| bad(&kp, "bad_kind"))?;
    let (kv, kvp) = get(o, at, "kind_version")?;
    let kind_version = dec_u32(kv, &kvp)?;
    if kind_version == 0 {
        return Err(bad(&kvp, "out_of_range"));
    }
    let (p, pp) = get(o, at, "payload")?;
    let payload = untrusted_json(p, &pp)?;
    let (r, rp) = get(o, at, "repeat_header_rows")?;
    let rows = dec_u32(r, &rp)?;
    let repeat_header_rows =
        u8::try_from(rows).ok().filter(|n| *n <= MAX_REPEAT_HEADER_ROWS).ok_or_else(|| bad(&rp, "out_of_range"))?;
    let (s, sp) = get(o, at, "slot")?;
    let slot = slot(s, &sp)?;
    Ok(TemplateBlock { id, kind, kind_version, payload, slot, repeat_header_rows, continuation_block })
}

/// Continuation flags of one page: each names another block of the same page, and no
/// chain of them loops.
fn continuations(blocks: &[TemplateBlock], at: &str) -> Result<(), TemplateError> {
    let index: BTreeMap<Uuid, usize> = blocks.iter().enumerate().map(|(j, b)| (b.id.uuid(), j)).collect();
    let ptr = |j: usize| child(&child(at, &j.to_string()), "continuation_block");
    let mut next: Vec<Option<usize>> = Vec::with_capacity(blocks.len());
    for (j, b) in blocks.iter().enumerate() {
        let Some(c) = b.continuation_block else {
            next.push(None);
            continue;
        };
        if c == b.id {
            return Err(bad(&ptr(j), "continuation_self"));
        }
        match index.get(&c.uuid()) {
            Some(t) => next.push(Some(*t)),
            None => return Err(bad(&ptr(j), "continuation_not_on_page")),
        }
    }
    // Each block has at most one successor, so the flags form a functional graph. One
    // O(n) pass: walk from each unvisited block, marking blocks on the current path; a walk
    // that meets its own path has found a loop (the path from that block on). Every block
    // is walked once. The error names the first block in page order that lies on a loop.
    const UNSEEN: u8 = 0;
    const ON_PATH: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![UNSEEN; blocks.len()];
    let mut first_on_loop: Option<usize> = None;
    let mut path: Vec<usize> = Vec::new();
    for start in 0..blocks.len() {
        if state.get(start) != Some(&UNSEEN) {
            continue;
        }
        path.clear();
        let mut j = start;
        loop {
            match state.get(j).copied() {
                Some(UNSEEN) => {
                    if let Some(s) = state.get_mut(j) {
                        *s = ON_PATH;
                    }
                    path.push(j);
                    match next.get(j).copied().flatten() {
                        Some(n) => j = n,
                        None => break,
                    }
                }
                Some(ON_PATH) => {
                    // `j` is on this walk's path: the loop is the path from `j` on.
                    let from = path.iter().position(|p| *p == j).unwrap_or(0);
                    let low = path.get(from..).and_then(|l| l.iter().min().copied());
                    first_on_loop = match (first_on_loop, low) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                    break;
                }
                _ => break,
            }
        }
        for p in &path {
            if let Some(s) = state.get_mut(*p) {
                *s = DONE;
            }
        }
    }
    if let Some(j) = first_on_loop {
        return Err(bad(&ptr(j), "continuation_cycle"));
    }
    Ok(())
}

const TEMPLATE_KEYS: [&str; 7] =
    ["clearance", "locked", "media_type", "pages", "schema_version", "template_id", "title"];

/// Reads a schema-version-1 value.
pub(crate) fn template(v: &CValue) -> Result<Template, TemplateError> {
    let o = exact(v, "", &TEMPLATE_KEYS)?;
    let (c, cp) = get(o, "", "clearance")?;
    let clearance = clearance(c, &cp)?;
    let (l, lp) = get(o, "", "locked")?;
    let CValue::Bool(locked) = l else { return Err(bad(&lp, "wrong_type")) };
    let (m, mp) = get(o, "", "media_type")?;
    if m != &CValue::Str(MEDIA_TYPE.to_string()) {
        return Err(bad(&mp, "bad_media_type"));
    }
    let (ps, pp) = get(o, "", "pages")?;
    let CValue::Arr(pages_in) = ps else { return Err(bad(&pp, "wrong_type")) };
    let mut seen: BTreeSet<Uuid> = BTreeSet::new();
    let mut pages = Vec::with_capacity(pages_in.len().min(MAX_PAGES));
    let mut total = 0usize;
    for (i, p) in pages_in.iter().enumerate() {
        let at = child(&pp, &i.to_string());
        if i >= MAX_PAGES {
            return Err(bad(&at, "too_many_pages"));
        }
        let po = exact(p, &at, &["blocks", "id"])?;
        let (bs, bp) = get(po, &at, "blocks")?;
        let CValue::Arr(blocks_in) = bs else { return Err(bad(&bp, "wrong_type")) };
        let mut blocks = Vec::with_capacity(blocks_in.len().min(MAX_TEMPLATE_BLOCKS));
        for (j, b) in blocks_in.iter().enumerate() {
            let bat = child(&bp, &j.to_string());
            if total >= MAX_TEMPLATE_BLOCKS {
                return Err(bad(&bat, "too_many_blocks"));
            }
            total += 1;
            blocks.push(block(b, &bat, &mut seen)?);
        }
        let (pid, pidp) = get(po, &at, "id")?;
        let id = PageId::new(v7(pid, &pidp)?).ok_or_else(|| bad(&pidp, "bad_id"))?;
        if !seen.insert(id.uuid()) {
            return Err(bad(&pidp, "repeated_id"));
        }
        continuations(&blocks, &bp)?;
        pages.push(TemplatePage { id, blocks });
    }
    let (sv, svp) = get(o, "", "schema_version")?;
    if dec_u32(sv, &svp)? != SCHEMA_VERSION {
        return Err(bad(&svp, "bad_version"));
    }
    let (t, tp) = get(o, "", "template_id")?;
    let template_id = TemplateId::new(v7(t, &tp)?).ok_or_else(|| bad(&tp, "bad_id"))?;
    if !seen.insert(template_id.uuid()) {
        return Err(bad(&tp, "repeated_id"));
    }
    let (ti, tip) = get(o, "", "title")?;
    let title = untrusted_text(ti, &tip)?;
    Ok(Template { schema_version: SCHEMA_VERSION, template_id, title, locked: *locked, clearance, pages })
}
