//! The strict reader from a slot's spec value to a [`Slot`].
//!
//! Order (pinned in the leaf's Design): within each object the key set is checked first
//! (the first unknown key in code-point order, then the first missing key), then the
//! members in code-point key order, depth first; list entries in index order. A tagged
//! object (`repeat`) reads its `kind` tag before its key set.

use super::error::child;
use super::model::{
    id_fault, BindingRule, CharacteristicSelector, CharacteristicType, ColumnKey, ColumnRule, Direction, FieldTableId,
    IdSelector, Outcome, PageGroupRule, RepetitionRule, SavedQueryId, Selector, Slot, SortField, SortKey, SortedList,
    StableId, TagName, TagSelector, MAX_COLUMNS_PER_GROUP, MAX_LISTED_IDS, MAX_REPEAT_COLUMNS,
};
use crate::doc::{is_v7, PageId};
use crate::tmpl::format::SlotKind;
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::{BTreeMap, BTreeSet};

type Obj = BTreeMap<String, CValue>;

/// A reader failure before the block is attached.
pub(crate) struct Fault {
    pub(crate) pointer: String,
    pub(crate) reason: &'static str,
}

fn bad<T>(at: &str, reason: &'static str) -> Result<T, Fault> {
    Err(Fault { pointer: at.to_string(), reason })
}

/// What the reader needs from the template around the slot.
pub(crate) struct Context<'a> {
    pub(crate) kind: SlotKind,
    pub(crate) on_table: bool,
    pub(crate) pages: &'a BTreeSet<PageId>,
}

/// An object with exactly `keys` (given in code-point order).
fn exact<'a>(v: &'a CValue, at: &str, keys: &[&str]) -> Result<&'a Obj, Fault> {
    let CValue::Obj(o) = v else { return bad(at, "wrong_type") };
    if let Some(k) = o.keys().find(|k| !keys.contains(&k.as_str())) {
        return bad(&child(at, k), "unknown_field");
    }
    if let Some(k) = keys.iter().find(|k| !o.contains_key(**k)) {
        return bad(&child(at, k), "missing_field");
    }
    Ok(o)
}

/// Member `k` of an object already checked by [`exact`].
fn get<'a>(o: &'a Obj, at: &str, k: &str) -> Result<(&'a CValue, String), Fault> {
    let p = child(at, k);
    match o.get(k) {
        Some(v) => Ok((v, p)),
        None => bad(&p, "missing_field"),
    }
}

fn string<'a>(v: &'a CValue, at: &str) -> Result<&'a str, Fault> {
    match v {
        CValue::Str(s) => Ok(s.as_str()),
        // Unreachable through the format, whose template json holds no number; named like
        // the format's reason all the same.
        CValue::Num(_) | CValue::I64(_) | CValue::U64(_) | CValue::F64Bits(_) => bad(at, "number"),
        _ => bad(at, "wrong_type"),
    }
}

fn array<'a>(v: &'a CValue, at: &str) -> Result<&'a [CValue], Fault> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => bad(at, "wrong_type"),
    }
}

/// Canonical lowercase text of a UUIDv7.
fn v7(v: &CValue, at: &str) -> Result<Uuid, Fault> {
    let s = string(v, at)?;
    match Uuid::parse(s).ok().filter(|u| u.to_string() == s && is_v7(u)) {
        Some(u) => Ok(u),
        None => bad(at, "bad_id"),
    }
}

/// A canonical decimal string (`0` or no leading zero, at most 10 digits) in `lo..=hi`.
fn small_int(v: &CValue, at: &str, lo: u32, hi: u32) -> Result<u32, Fault> {
    let s = string(v, at)?;
    let canonical =
        !s.is_empty() && s.len() <= 10 && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if !canonical {
        return bad(at, "bad_integer");
    }
    match s.parse::<u32>() {
        Ok(n) if (lo..=hi).contains(&n) => Ok(n),
        _ => bad(at, "out_of_range"),
    }
}

/// Entries in strictly ascending order of `key`: an entry equal to the previous one is
/// `duplicate`, a smaller one `not_sorted`.
fn ascending<K: Ord>(prev: Option<&K>, cur: &K, at: &str) -> Result<(), Fault> {
    match prev.map(|p| p.cmp(cur)) {
        Some(std::cmp::Ordering::Equal) => bad(at, "duplicate"),
        Some(std::cmp::Ordering::Greater) => bad(at, "not_sorted"),
        _ => Ok(()),
    }
}

/// A list's length: `empty_list` if empty, `too_many_ids` past [`MAX_LISTED_IDS`].
fn list_len(a: &[CValue], at: &str) -> Result<(), Fault> {
    if a.is_empty() {
        return bad(at, "empty_list");
    }
    if a.len() > MAX_LISTED_IDS {
        return bad(at, "too_many_ids");
    }
    Ok(())
}

/// A list of untrusted IDs or tag names: bounded, each 1..=256 bytes without NUL, in
/// strictly ascending byte order.
fn untrusted_list<T: Ord>(v: &CValue, at: &str, make: fn(&str) -> Option<T>) -> Result<SortedList<T>, Fault> {
    let a = array(v, at)?;
    list_len(a, at)?;
    let mut out: Vec<T> = Vec::with_capacity(a.len());
    for (i, e) in a.iter().enumerate() {
        let p = child(at, &i.to_string());
        let s = string(e, &p)?;
        if let Some(r) = id_fault(s) {
            return bad(&p, r);
        }
        let Some(item) = make(s) else { return bad(&p, "bad_id") };
        ascending(out.last(), &item, &p)?;
        out.push(item);
    }
    Ok(SortedList::checked(out))
}

/// A selector object holding exactly one member from `keys`: an unknown key (first in
/// code-point order) is `unknown_field`, no member `missing_field` at the object, a second
/// member `conflicting_fields` at the later key. Returns the key, its value and pointer.
fn one_of<'a>(v: &'a CValue, at: &str, keys: &[&str]) -> Result<(&'a str, &'a CValue, String), Fault> {
    let CValue::Obj(o) = v else { return bad(at, "wrong_type") };
    if let Some(k) = o.keys().find(|k| !keys.contains(&k.as_str())) {
        return bad(&child(at, k), "unknown_field");
    }
    let mut it = o.iter();
    match (it.next(), it.next()) {
        (None, _) => bad(at, "missing_field"),
        (Some(_), Some((k2, _))) => bad(&child(at, k2), "conflicting_fields"),
        (Some((k, val)), None) => Ok((k.as_str(), val, child(at, k))),
    }
}

fn ids_selector(v: &CValue, at: &str) -> Result<IdSelector, Fault> {
    match v {
        CValue::Str(s) if s == "all" => Ok(IdSelector::All),
        CValue::Str(_) => bad(at, "unknown_value"),
        _ => {
            let (_, val, p) = one_of(v, at, &["ids"])?;
            Ok(IdSelector::Ids(untrusted_list(val, &p, StableId::new)?))
        }
    }
}

fn characteristics(v: &CValue, at: &str) -> Result<CharacteristicSelector, Fault> {
    match v {
        CValue::Str(s) if s == "all" => Ok(CharacteristicSelector::All),
        CValue::Str(_) => bad(at, "unknown_value"),
        _ => {
            let (k, val, p) = one_of(v, at, &["ids", "type"])?;
            if k == "ids" {
                return Ok(CharacteristicSelector::Ids(untrusted_list(val, &p, StableId::new)?));
            }
            match CharacteristicType::new(string(val, &p)?) {
                Some(t) => Ok(CharacteristicSelector::Type(t)),
                None => bad(&p, "bad_type_name"),
            }
        }
    }
}

fn tags(v: &CValue, at: &str) -> Result<TagSelector, Fault> {
    let (k, val, p) = one_of(v, at, &["all_of", "any_of"])?;
    let list = untrusted_list(val, &p, TagName::new)?;
    Ok(if k == "all_of" { TagSelector::AllOf(list) } else { TagSelector::AnyOf(list) })
}

fn selector(kind: SlotKind, v: &CValue, at: &str) -> Result<Selector, Fault> {
    Ok(match kind {
        SlotKind::Characteristics => Selector::Characteristics(characteristics(v, at)?),
        SlotKind::Elements => Selector::Elements(ids_selector(v, at)?),
        SlotKind::Tags => Selector::Tags(tags(v, at)?),
        SlotKind::Stages => Selector::Stages(ids_selector(v, at)?),
        SlotKind::Runs => Selector::Runs(ids_selector(v, at)?),
        SlotKind::SavedQuery => {
            let o = exact(v, at, &["id"])?;
            let (i, ip) = get(o, at, "id")?;
            match SavedQueryId::new(v7(i, &ip)?) {
                Some(q) => Selector::SavedQuery(q),
                None => return bad(&ip, "bad_id"),
            }
        }
        SlotKind::FieldTable => {
            let o = exact(v, at, &["table"])?;
            let (t, tp) = get(o, at, "table")?;
            match FieldTableId::new(string(t, &tp)?) {
                Some(id) => Selector::FieldTable(id),
                None => return bad(&tp, "bad_table_id"),
            }
        }
    })
}

fn outcome(v: &CValue, at: &str) -> Result<Vec<Outcome>, Fault> {
    let a = array(v, at)?;
    list_len(a, at)?;
    let mut out: Vec<Outcome> = Vec::new();
    for (i, e) in a.iter().enumerate() {
        let p = child(at, &i.to_string());
        let Some(o) = Outcome::parse(string(e, &p)?) else { return bad(&p, "unknown_value") };
        ascending(out.last(), &o, &p)?;
        out.push(o);
    }
    Ok(out)
}

/// `{<slot kind>: <selector>}` plus an optional `outcome`; the selector key must be the
/// envelope's kind (another kind word is `kind_mismatch`).
fn rule(kind: SlotKind, v: &CValue, at: &str) -> Result<BindingRule, Fault> {
    let CValue::Obj(o) = v else { return bad(at, "wrong_type") };
    let own = kind.as_str();
    for k in o.keys() {
        if k != own && k != "outcome" {
            let reason = if SlotKind::parse(k).is_some() { "kind_mismatch" } else { "unknown_field" };
            return bad(&child(at, k), reason);
        }
    }
    if !o.contains_key(own) {
        return bad(&child(at, own), "missing_field");
    }
    let mut sel = None;
    let mut out = None;
    for (k, val) in o {
        let p = child(at, k);
        if k == "outcome" {
            out = Some(outcome(val, &p)?);
        } else {
            sel = Some(selector(kind, val, &p)?);
        }
    }
    match sel {
        Some(selector) => Ok(BindingRule { selector, outcome: out }),
        None => bad(&child(at, own), "missing_field"),
    }
}

fn sort(v: &CValue, at: &str) -> Result<Option<SortKey>, Fault> {
    if v == &CValue::Null {
        return Ok(None);
    }
    let o = exact(v, at, &["direction", "key"])?;
    let (d, dp) = get(o, at, "direction")?;
    let Some(direction) = Direction::parse(string(d, &dp)?) else { return bad(&dp, "unknown_value") };
    let (k, kp) = get(o, at, "key")?;
    let Some(field) = SortField::parse(string(k, &kp)?) else { return bad(&kp, "unknown_value") };
    Ok(Some(SortKey { field, direction }))
}

fn columns(v: &CValue, at: &str) -> Result<Option<Vec<ColumnKey>>, Fault> {
    if v == &CValue::Null {
        return Ok(None);
    }
    let a = array(v, at)?;
    if a.is_empty() {
        return bad(at, "empty_list");
    }
    if a.len() > MAX_REPEAT_COLUMNS {
        return bad(at, "too_many_columns");
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(a.len());
    for (i, e) in a.iter().enumerate() {
        let p = child(at, &i.to_string());
        let Some(c) = ColumnKey::new(string(e, &p)?) else { return bad(&p, "bad_column_key") };
        if !seen.insert(c.clone()) {
            return bad(&p, "duplicate");
        }
        out.push(c);
    }
    Ok(Some(out))
}

fn pages(v: &CValue, at: &str, present: &BTreeSet<PageId>) -> Result<Vec<PageId>, Fault> {
    let a = array(v, at)?;
    list_len(a, at)?;
    let mut out: Vec<PageId> = Vec::with_capacity(a.len());
    for (i, e) in a.iter().enumerate() {
        let p = child(at, &i.to_string());
        let Some(id) = PageId::new(v7(e, &p)?) else { return bad(&p, "bad_id") };
        ascending(out.last(), &id, &p)?;
        if !present.contains(&id) {
            return bad(&p, "page_not_found");
        }
        out.push(id);
    }
    Ok(out)
}

fn repeat(v: &CValue, at: &str, on_table: bool, present: &BTreeSet<PageId>) -> Result<RepetitionRule, Fault> {
    let CValue::Obj(o) = v else { return bad(at, "wrong_type") };
    let kp = child(at, "kind");
    let kind = match o.get("kind") {
        None => return bad(&kp, "missing_field"),
        Some(k) => string(k, &kp)?,
    };
    match kind {
        "row" => {
            exact(v, at, &["kind"])?;
            if !on_table {
                return bad(&kp, "row_needs_table");
            }
            Ok(RepetitionRule::Row)
        }
        "column" => {
            let o = exact(v, at, &["kind", "max_columns_per_group", "repeat_columns"])?;
            let (m, mp) = get(o, at, "max_columns_per_group")?;
            let max = small_int(m, &mp, 1, u32::from(MAX_COLUMNS_PER_GROUP))?;
            let (c, cp) = get(o, at, "repeat_columns")?;
            let repeat_columns = columns(c, &cp)?;
            // `max` is at most 64, so the conversion cannot fail.
            let max_columns_per_group = u8::try_from(max).unwrap_or(MAX_COLUMNS_PER_GROUP);
            Ok(RepetitionRule::Column(ColumnRule { max_columns_per_group, repeat_columns }))
        }
        "page_group" => {
            let o = exact(v, at, &["kind", "pages"])?;
            let (p, pp) = get(o, at, "pages")?;
            Ok(RepetitionRule::PageGroup(PageGroupRule { pages: pages(p, &pp, present)? }))
        }
        _ => bad(&kp, "unknown_value"),
    }
}

/// Reads a spec value at pointer `at`.
pub(crate) fn spec(v: &CValue, at: &str, cx: &Context<'_>) -> Result<Slot, Fault> {
    let o = exact(v, at, &["repeat", "rule", "sort"])?;
    let (r, rp) = get(o, at, "repeat")?;
    let repeat = repeat(r, &rp, cx.on_table, cx.pages)?;
    let (ru, rup) = get(o, at, "rule")?;
    let rule = rule(cx.kind, ru, &rup)?;
    let (so, sop) = get(o, at, "sort")?;
    let sort = sort(so, &sop)?;
    Ok(Slot { rule, sort, repeat })
}
