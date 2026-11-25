//! Projection onto trimmed CAD faces (spec node GEO.DIST.FACE).
//!
//! - [`one`] (GEO.DIST.FACE.ONE): the closest point on a single trimmed face, from the exact
//!   surface, edge curves and trimming loops.
//! - [`incid`] (GEO.DIST.FACE.INCID): the faces incident at a foot point (interior, edge or
//!   vertex), their outward normals and incident angles, and the CAD signed distance.

pub mod incid;
pub mod one;
