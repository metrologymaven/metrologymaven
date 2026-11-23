//! `BlockKind` registration for report block kind `drawing` (version 1) (`RPT.BALLOON.BLOCK`).

use super::model::{DrawingPayload, DRAWING_BLOCK_KIND, DRAWING_BLOCK_VERSION};
use crate::doc::kind::{BlockKind, KindRegistry, KindRegistryError, PayloadError};
use crate::doc::model::{BlockKindId, CanonicalJson};

/// `RPT.MODEL.DOC` [`BlockKind`] implementation for `drawing` blocks (version 1).
#[derive(Debug, Clone, Copy, Default)]
pub struct DrawingBlockKind;

impl BlockKind for DrawingBlockKind {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal(DRAWING_BLOCK_KIND)
    }

    fn version(&self) -> u32 {
        DRAWING_BLOCK_VERSION
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        let _ = DrawingPayload::from_canonical_json(payload)?;
        Ok(())
    }

    fn value_fields(&self, payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        let _ = DrawingPayload::from_canonical_json(payload)?;
        Ok(Vec::new())
    }
}

/// Registers the `drawing` block kind (version 1) into `registry`.
pub fn register_drawing_kind(registry: &mut KindRegistry) -> Result<(), KindRegistryError> {
    registry.register(Box::new(DrawingBlockKind))
}
