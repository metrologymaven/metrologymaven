//! The ordering contract (REQ-RPT.MODEL.TMPL.SLOT-004): sort value in the slot's direction,
//! then ascending stable ID. Total and deterministic, with one pinned collation: byte order
//! of UTF-8 (equal to code-point order), no locale, no normalisation, no case folding.

use super::model::{Direction, SortKey, StableId};
use std::cmp::Ordering;

/// An item's value for the slot's sort field, supplied by the evaluator
/// (`RPT.MODEL.BIND.EXPAND`): `Text` for characteristic IDs, names and element IDs; `Int`
/// for balloon numbers, the project stage position and run start times (integer timestamps
/// on one time base); `Missing` when the item has none (for example no balloon).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SortValue {
    Int(i64),
    Text(String),
    Missing,
}

/// One selected item: its stable ID and its sort value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Item {
    pub id: StableId,
    pub value: SortValue,
}

/// Present values: `Int` by number, `Text` by UTF-8 bytes, every `Int` before every `Text`.
fn present(a: &SortValue, b: &SortValue) -> Ordering {
    match (a, b) {
        (SortValue::Int(x), SortValue::Int(y)) => x.cmp(y),
        (SortValue::Text(x), SortValue::Text(y)) => x.as_bytes().cmp(y.as_bytes()),
        (SortValue::Int(_), _) => Ordering::Less,
        (_, SortValue::Int(_)) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

/// The contract order of `a` and `b` under `sort`:
///
/// 1. an item with a value comes before one with `Missing`, in either direction;
/// 2. two values compare by [`present`] rules, reversed for [`Direction::Desc`];
/// 3. ties fall back to ascending stable ID (UTF-8 bytes), in either direction.
///
/// Items that compare equal have the same ID and the same value, so they are identical:
/// the order is total and the sorted list is a function of the input multiset alone.
/// `sort.field` names what the values mean; it does not change the comparison.
pub fn compare(a: &Item, b: &Item, sort: SortKey) -> Ordering {
    let by_value = match (&a.value, &b.value) {
        (SortValue::Missing, SortValue::Missing) => Ordering::Equal,
        (SortValue::Missing, _) => Ordering::Greater,
        (_, SortValue::Missing) => Ordering::Less,
        (x, y) => match sort.direction {
            Direction::Asc => present(x, y),
            Direction::Desc => present(x, y).reverse(),
        },
    };
    by_value.then_with(|| a.id.untrusted_str().as_bytes().cmp(b.id.untrusted_str().as_bytes()))
}

/// `items` in contract order ([`compare`]). O(n log n), no allocation beyond the sort's.
pub fn order(mut items: Vec<Item>, sort: SortKey) -> Vec<Item> {
    items.sort_by(|a, b| compare(a, b, sort));
    items
}
