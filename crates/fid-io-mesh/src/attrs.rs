//! Per-vertex attribute table shared by all mesh readers (spec node `IO.MESH.MBLD`).
//!
//! Standard GEO attributes (`normal`, `intensity`, `colour`, `scanner_id`, `timestamp_ns`,
//! `quality`, `scan_origins`, `grid`) flow directly into [`fid_geo_types::AttributeSet`].
//! Format-specific custom vertex properties (such as PLY scalar properties,
//! `REQ-IO.MESH-003`) are stored losslessly in a deterministic `BTreeMap` keyed by
//! property name, with zero `HashMap` usage.

use crate::error::MeshBuildError;
use core::mem::size_of;
use fid_geo_types::AttributeSet;
use fid_io_reg::{Budget, Position};
use std::collections::BTreeMap;

/// Declared scalar storage type of a custom per-vertex attribute column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CustomScalarType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
}

impl CustomScalarType {
    /// Byte size of one scalar value of this type.
    pub const fn byte_width(self) -> u64 {
        match self {
            CustomScalarType::I8 | CustomScalarType::U8 => 1,
            CustomScalarType::I16 | CustomScalarType::U16 => 2,
            CustomScalarType::I32 | CustomScalarType::U32 | CustomScalarType::F32 => 4,
            CustomScalarType::I64 | CustomScalarType::U64 | CustomScalarType::F64 => 8,
        }
    }

    /// Stable type name.
    pub const fn as_str(self) -> &'static str {
        match self {
            CustomScalarType::I8 => "i8",
            CustomScalarType::U8 => "u8",
            CustomScalarType::I16 => "i16",
            CustomScalarType::U16 => "u16",
            CustomScalarType::I32 => "i32",
            CustomScalarType::U32 => "u32",
            CustomScalarType::I64 => "i64",
            CustomScalarType::U64 => "u64",
            CustomScalarType::F32 => "f32",
            CustomScalarType::F64 => "f64",
        }
    }
}

/// Lossless typed column of custom per-vertex scalar values.
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarColumn {
    I8(Vec<i8>),
    U8(Vec<u8>),
    I16(Vec<i16>),
    U16(Vec<u16>),
    I32(Vec<i32>),
    U32(Vec<u32>),
    I64(Vec<i64>),
    U64(Vec<u64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
}

impl ScalarColumn {
    /// An empty column with the declared scalar type.
    pub fn empty(kind: CustomScalarType) -> Self {
        match kind {
            CustomScalarType::I8 => ScalarColumn::I8(Vec::new()),
            CustomScalarType::U8 => ScalarColumn::U8(Vec::new()),
            CustomScalarType::I16 => ScalarColumn::I16(Vec::new()),
            CustomScalarType::U16 => ScalarColumn::U16(Vec::new()),
            CustomScalarType::I32 => ScalarColumn::I32(Vec::new()),
            CustomScalarType::U32 => ScalarColumn::U32(Vec::new()),
            CustomScalarType::I64 => ScalarColumn::I64(Vec::new()),
            CustomScalarType::U64 => ScalarColumn::U64(Vec::new()),
            CustomScalarType::F32 => ScalarColumn::F32(Vec::new()),
            CustomScalarType::F64 => ScalarColumn::F64(Vec::new()),
        }
    }

    /// Declared scalar type of this column.
    pub fn scalar_type(&self) -> CustomScalarType {
        match self {
            ScalarColumn::I8(_) => CustomScalarType::I8,
            ScalarColumn::U8(_) => CustomScalarType::U8,
            ScalarColumn::I16(_) => CustomScalarType::I16,
            ScalarColumn::U16(_) => CustomScalarType::U16,
            ScalarColumn::I32(_) => CustomScalarType::I32,
            ScalarColumn::U32(_) => CustomScalarType::U32,
            ScalarColumn::I64(_) => CustomScalarType::I64,
            ScalarColumn::U64(_) => CustomScalarType::U64,
            ScalarColumn::F32(_) => CustomScalarType::F32,
            ScalarColumn::F64(_) => CustomScalarType::F64,
        }
    }

    /// Number of vertex values stored in this column.
    pub fn len(&self) -> u64 {
        let n = match self {
            ScalarColumn::I8(v) => v.len(),
            ScalarColumn::U8(v) => v.len(),
            ScalarColumn::I16(v) => v.len(),
            ScalarColumn::U16(v) => v.len(),
            ScalarColumn::I32(v) => v.len(),
            ScalarColumn::U32(v) => v.len(),
            ScalarColumn::I64(v) => v.len(),
            ScalarColumn::U64(v) => v.len(),
            ScalarColumn::F32(v) => v.len(),
            ScalarColumn::F64(v) => v.len(),
        };
        u64::try_from(n).unwrap_or(u64::MAX)
    }

    /// Whether this column has zero values.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total heap bytes currently allocated by this column's backing buffer.
    pub fn allocated_bytes(&self) -> u64 {
        let cap = match self {
            ScalarColumn::I8(v) => v.capacity(),
            ScalarColumn::U8(v) => v.capacity(),
            ScalarColumn::I16(v) => v.capacity(),
            ScalarColumn::U16(v) => v.capacity(),
            ScalarColumn::I32(v) => v.capacity(),
            ScalarColumn::U32(v) => v.capacity(),
            ScalarColumn::I64(v) => v.capacity(),
            ScalarColumn::U64(v) => v.capacity(),
            ScalarColumn::F32(v) => v.capacity(),
            ScalarColumn::F64(v) => v.capacity(),
        };
        u64::try_from(cap).unwrap_or(u64::MAX).saturating_mul(self.scalar_type().byte_width())
    }
}

/// Optional per-vertex attributes collected alongside mesh coordinates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttrTable {
    /// Standard GEO vertex attributes (normals, intensity, colour, scanner ID, timestamp, quality).
    pub standard: AttributeSet,
    /// Format-specific custom vertex scalar properties in deterministic name order.
    pub custom: BTreeMap<String, ScalarColumn>,
}

fn budgeted_push<T>(vec: &mut Vec<T>, val: T, budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
    if vec.len() == vec.capacity() {
        let old_cap = vec.capacity();
        let grow_by = if old_cap == 0 { 16 } else { old_cap };
        let bytes = u64::try_from(grow_by)
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::try_from(size_of::<T>()).unwrap_or(u64::MAX));
        budget.charge_alloc(bytes, at)?;
        vec.try_reserve(grow_by).map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;
    }
    vec.push(val);
    Ok(())
}

impl AttrTable {
    /// An empty attribute table with no channels.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a per-vertex unit normal, charging buffer growth to `budget`.
    pub fn push_normal(&mut self, normal: [f64; 3], budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        let cols = self.standard.normal.get_or_insert_with(|| [Vec::new(), Vec::new(), Vec::new()]);
        budgeted_push(&mut cols[0], normal[0], budget, at)?;
        budgeted_push(&mut cols[1], normal[1], budget, at)?;
        budgeted_push(&mut cols[2], normal[2], budget, at)?;
        Ok(())
    }

    /// Appends a per-vertex RGB colour, charging buffer growth to `budget`.
    pub fn push_colour(&mut self, rgb: [u8; 3], budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        let col = self.standard.colour.get_or_insert_with(Vec::new);
        budgeted_push(col, rgb, budget, at)
    }

    /// Appends a per-vertex intensity, charging buffer growth to `budget`.
    pub fn push_intensity(&mut self, intensity: f32, budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        let col = self.standard.intensity.get_or_insert_with(Vec::new);
        budgeted_push(col, intensity, budget, at)
    }

    /// Appends a per-vertex scanner ID, charging buffer growth to `budget`.
    pub fn push_scanner_id(&mut self, id: u32, budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        let col = self.standard.scanner_id.get_or_insert_with(Vec::new);
        budgeted_push(col, id, budget, at)
    }

    /// Appends a per-vertex acquisition timestamp in nanoseconds, charging buffer growth to `budget`.
    pub fn push_timestamp_ns(&mut self, ts: i64, budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        let col = self.standard.timestamp_ns.get_or_insert_with(Vec::new);
        budgeted_push(col, ts, budget, at)
    }

    /// Appends a per-vertex quality flag, charging buffer growth to `budget`.
    pub fn push_quality(&mut self, quality: u16, budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        let col = self.standard.quality.get_or_insert_with(Vec::new);
        budgeted_push(col, quality, budget, at)
    }

    /// Inserts a complete custom scalar column, charging its allocated buffer bytes to `budget`.
    pub fn insert_custom(
        &mut self,
        name: impl Into<String>,
        column: ScalarColumn,
        budget: &mut Budget,
        at: Position,
    ) -> Result<(), MeshBuildError> {
        let name = name.into();
        let bytes = column.allocated_bytes().saturating_add(u64::try_from(name.len()).unwrap_or(u64::MAX));
        budget.charge_alloc(bytes, at)?;
        self.custom.insert(name, column);
        Ok(())
    }

    /// Appends an `f64` value to a custom `F64` column, creating the column on first use.
    pub fn push_custom_f64(
        &mut self,
        name: &str,
        value: f64,
        budget: &mut Budget,
        at: Position,
    ) -> Result<(), MeshBuildError> {
        let col = self.custom.entry(name.to_string()).or_insert_with(|| ScalarColumn::F64(Vec::new()));
        match col {
            ScalarColumn::F64(v) => budgeted_push(v, value, budget, at),
            _ => Err(MeshBuildError::AttrTypeMismatch { name: name.to_string() }),
        }
    }

    /// Appends an `f32` value to a custom `F32` column, creating the column on first use.
    pub fn push_custom_f32(
        &mut self,
        name: &str,
        value: f32,
        budget: &mut Budget,
        at: Position,
    ) -> Result<(), MeshBuildError> {
        let col = self.custom.entry(name.to_string()).or_insert_with(|| ScalarColumn::F32(Vec::new()));
        match col {
            ScalarColumn::F32(v) => budgeted_push(v, value, budget, at),
            _ => Err(MeshBuildError::AttrTypeMismatch { name: name.to_string() }),
        }
    }

    /// Validates that every custom column has exactly `vertex_count` values.
    /// Standard GEO attribute lengths and invariants are validated by `fid-geo-types`.
    pub fn validate_custom_lengths(&self, vertex_count: u64) -> Result<(), MeshBuildError> {
        for (name, col) in &self.custom {
            let found = col.len();
            if found != vertex_count {
                return Err(MeshBuildError::AttrLengthMismatch { name: name.clone(), expected: vertex_count, found });
            }
        }
        Ok(())
    }
}
