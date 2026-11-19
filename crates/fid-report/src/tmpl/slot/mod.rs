//! Slots: the binding-rule, sort-key and repetition-rule language. Spec node
//! `RPT.MODEL.TMPL.SLOT`.
//!
//! `RPT.MODEL.TMPL.FORMAT` carries a slot as `{"kind": <slot kind>, "spec": {"untrusted":
//! true, "value": <object>}}`; this module defines and checks that object. Rules are typed
//! data from closed lists plus ID lists, never text to evaluate, so a template cannot carry
//! code. Evaluating rules at a journal head is `RPT.MODEL.BIND.EXPAND`, which sorts with
//! [`order`] / [`compare`].
//!
//! Spec value (exactly these keys; integers are decimal strings, so no JSON number):
//!
//! ```text
//! { "repeat": {"kind": "row"}
//!           | {"kind": "column", "max_columns_per_group": "1".."64",
//!              "repeat_columns": null | [<column key>, ...]}         (1..64, distinct)
//!           | {"kind": "page_group", "pages": [<page uuidv7>, ...]}  (ascending, present),
//!   "rule": { <the envelope kind>: <selector>, ["outcome": [<outcome>, ...]] },
//!   "sort": null | {"direction": "asc" | "desc", "key": <sort field>} }
//!
//! selector by kind:
//!   characteristics  "all" | {"ids": [<id>, ...]} | {"type": <QIF type token>}
//!   elements, stages, runs  "all" | {"ids": [<id>, ...]}
//!   tags             {"any_of": [<tag>, ...]} | {"all_of": [<tag>, ...]}
//!   saved_query      {"id": <uuidv7>}
//!   field_table      {"table": <table id>}
//! ```
//!
//! ID and tag lists hold 1 to 10 000 distinct entries in ascending UTF-8 byte order, each 1
//! to 256 bytes without NUL. Every failure is a [`SlotError`] (`report.slot_invalid`)
//! naming the block.

mod error;
mod model;
mod order;
mod read;

pub use error::{SlotError, MAX_SLOT_POINTER_BYTES};
pub use model::{
    BindingRule, CharacteristicSelector, CharacteristicType, ColumnKey, Direction, FieldTableId, IdList, IdSelector,
    Outcome, RepetitionRule, SavedQueryId, Selector, Slot, SortField, SortKey, SortedList, StableId, TagList, TagName,
    TagSelector, MAX_COLUMNS_PER_GROUP, MAX_ID_BYTES, MAX_LISTED_IDS, MAX_REPEAT_COLUMNS, MAX_TOKEN_BYTES, TABLE_KIND,
};
pub use order::{compare, order, Item, SortValue};

use crate::doc::{BlockId, PageId};
use crate::tmpl::format::{self, Template, TemplateBlock};
use std::collections::BTreeSet;

/// Validates the slot `slot` of `block` in a template whose pages are `page_ids`, and
/// returns it typed. The spec value is read at pointer `/spec/value`; a slot whose
/// envelope kind differs from its rule's selector key fails with `kind_mismatch`.
pub fn validate(slot: &format::Slot, block: &TemplateBlock, page_ids: &BTreeSet<PageId>) -> Result<Slot, SlotError> {
    let cx = read::Context { kind: slot.kind(), on_table: block.kind().as_str() == TABLE_KIND, pages: page_ids };
    read::spec(slot.spec().value(), "/spec/value", &cx).map_err(|f| SlotError::new(block.id(), &f.pointer, f.reason))
}

/// Validates every slot of `template`, blocks in document order (pages, then blocks by
/// index); the first invalid slot is the error.
pub fn validate_template(template: &Template) -> Result<Vec<(BlockId, Slot)>, SlotError> {
    let pages: BTreeSet<PageId> = template.pages().iter().map(|p| p.id()).collect();
    let mut out = Vec::new();
    for b in template.pages().iter().flat_map(|p| p.blocks()) {
        if let Some(s) = b.slot() {
            out.push((b.id(), validate(s, b, &pages)?));
        }
    }
    Ok(out)
}
