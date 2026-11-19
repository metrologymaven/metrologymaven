//! Test-only stub kinds (feature `test-kinds`). Not shipped: the real `text` and `image`
//! kinds belong to `RPT.MODEL.BLOCKS`.
//!
//! - `text` v1: payload exactly `{"text": <string>}`, the string within the [`Text`] bounds.
//! - `image` v1: payload exactly `{"blob": "sha256:<64 hex>"}`.

use super::kind::{BlockKind, PayloadError};
use super::model::{BlockKindId, CanonicalJson};
use super::text::Text;
use fid_canon::CValue;

/// The `text` stub kind.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextStub;

impl BlockKind for TextStub {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal("text")
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        let CValue::Obj(o) = payload.value() else { return Err(PayloadError::new("", "not_object")) };
        if o.len() != 1 {
            return Err(PayloadError::new("", "unexpected_keys"));
        }
        match o.get("text") {
            Some(CValue::Str(s)) => Text::new(s).map(|_| ()).map_err(|_| PayloadError::new("/text", "text_invalid")),
            _ => Err(PayloadError::new("/text", "not_string")),
        }
    }
}

/// The `image` stub kind.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImageStub;

impl BlockKind for ImageStub {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal("image")
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        let CValue::Obj(o) = payload.value() else { return Err(PayloadError::new("", "not_object")) };
        if o.len() != 1 {
            return Err(PayloadError::new("", "unexpected_keys"));
        }
        match o.get("blob") {
            Some(CValue::Str(s)) if fid_types::Digest::is_valid(s) => Ok(()),
            _ => Err(PayloadError::new("/blob", "not_digest")),
        }
    }
}
