//! The typed error of slot validation: `report.slot_invalid` naming the block, with the
//! RFC 6901 pointer of the first offending element (relative to the block's `slot` member)
//! and a fixed reason word.

use crate::doc::{must_escape, BlockId};
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;

/// Longest pointer an error holds, in bytes of UTF-8; a longer one is cut at a character
/// boundary. Pointers are built from input keys, so they are untrusted text.
pub const MAX_SLOT_POINTER_BYTES: usize = 1024;

/// `report.slot_invalid`: the slot of `block` failed validation at `pointer` for `reason`.
///
/// `pointer` is relative to the block's `slot` member, so the spec's own members start at
/// `/spec/value`; the full template pointer is `/pages/<i>/blocks/<j>/slot` followed by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotError {
    block: BlockId,
    pointer: String,
    reason: &'static str,
}

impl SlotError {
    /// A slot error; `pointer` is cut to [`MAX_SLOT_POINTER_BYTES`].
    pub(crate) fn new(block: BlockId, pointer: &str, reason: &'static str) -> SlotError {
        let mut cut = pointer.len().min(MAX_SLOT_POINTER_BYTES);
        while !pointer.is_char_boundary(cut) {
            cut -= 1;
        }
        SlotError { block, pointer: pointer.get(..cut).unwrap_or("").to_string(), reason }
    }

    /// Stable machine-readable error code: always `report.slot_invalid`.
    pub fn code(&self) -> &'static str {
        "report.slot_invalid"
    }

    /// The block whose slot is invalid.
    pub fn block(&self) -> BlockId {
        self.block
    }

    /// The pointer of the first offending element. Untrusted text: it holds input keys.
    pub fn pointer(&self) -> &str {
        &self.pointer
    }

    /// The fixed reason word.
    pub fn reason(&self) -> &'static str {
        self.reason
    }

    /// The `--json` / MCP error object, pointer marked untrusted:
    /// `{"block": <uuid>, "code", "pointer": {"text", "untrusted": true}, "reason"}`.
    pub fn to_value(&self) -> CValue {
        let mut p = BTreeMap::new();
        p.insert("text".to_string(), CValue::Str(self.pointer.clone()));
        p.insert("untrusted".to_string(), CValue::Bool(true));
        let mut o = BTreeMap::new();
        o.insert("block".to_string(), CValue::Str(self.block.uuid().to_string()));
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        o.insert("pointer".to_string(), CValue::Obj(p));
        o.insert("reason".to_string(), CValue::Str(self.reason.to_string()));
        CValue::Obj(o)
    }
}

impl fmt::Display for SlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut p = String::with_capacity(self.pointer.len());
        for c in self.pointer.chars() {
            if c == '\\' {
                p.push_str("\\\\");
            } else if must_escape(c) {
                p.push_str(&format!("\\u{{{:04X}}}", u32::from(c)));
            } else {
                p.push(c);
            }
        }
        write!(f, "{}: {} at \"{}\" in block {}", self.code(), self.reason, p, self.block.uuid())
    }
}

impl std::error::Error for SlotError {}

/// `base` extended by one RFC 6901 reference token (`~` as `~0`, `/` as `~1`).
pub(crate) fn child(base: &str, token: &str) -> String {
    let mut out = String::with_capacity(base.len() + token.len() + 1);
    out.push_str(base);
    out.push('/');
    for c in token.chars() {
        match c {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            c => out.push(c),
        }
    }
    out
}
