//! Block-wise point access without I/O (spec node `GEO.TYPES.ENV`, REQ-GEO.TYPES.ENV-006).
//!
//! Kernel code reads points through [`PointSource`] whether they are in memory (here) or
//! streamed in chunks (GEO.OOC.STREAM).

use crate::envelope::FrameRef;
use crate::error::GeoTypeError;
use crate::points::Coords;
use core::num::NonZeroUsize;

/// A named per-point channel carried with a block (attributes come with GEO.TYPES.POINTS;
/// the in-memory sources of this crate emit none yet).
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub name: String,
    pub values: Vec<f64>,
}

/// A block of full-resolution points with their original indexes. All arrays have the
/// same length.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PointBlock {
    pub original_index: Vec<u64>,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    pub channels: Vec<Channel>,
}

/// A source of point blocks in index order.
pub trait PointSource {
    /// The next block, or `None` when all points have been emitted.
    fn next_block(&mut self) -> Option<Result<PointBlock, GeoTypeError>>;
    /// The total number of points.
    fn len(&self) -> u64;
    /// Whether the source has no points at all.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The frame of the points.
    fn frame(&self) -> FrameRef;
}

/// [`PointSource`] over an in-memory [`crate::PointSet`] or [`crate::PointCloud`]; made
/// by their `source` method.
#[derive(Debug, Clone)]
pub struct InMemorySource<'a> {
    frame: FrameRef,
    coords: &'a Coords,
    block_size: usize,
    pos: usize,
}

impl<'a> InMemorySource<'a> {
    pub(crate) fn new(frame: FrameRef, coords: &'a Coords, block_size: NonZeroUsize) -> InMemorySource<'a> {
        InMemorySource { frame, coords, block_size: block_size.get(), pos: 0 }
    }
}

fn slice(v: &[f64], start: usize, end: usize) -> Vec<f64> {
    v.get(start..end).unwrap_or(&[]).to_vec()
}

impl PointSource for InMemorySource<'_> {
    fn next_block(&mut self) -> Option<Result<PointBlock, GeoTypeError>> {
        let n = self.coords.len();
        if self.pos >= n {
            return None;
        }
        let start = self.pos;
        let end = start.saturating_add(self.block_size).min(n);
        self.pos = end;
        Some(Ok(PointBlock {
            original_index: (start..end).map(|i| i as u64).collect(),
            x: slice(&self.coords.x, start, end),
            y: slice(&self.coords.y, start, end),
            z: slice(&self.coords.z, start, end),
            channels: Vec::new(),
        }))
    }

    fn len(&self) -> u64 {
        self.coords.len() as u64
    }

    fn frame(&self) -> FrameRef {
        self.frame
    }
}
