//! Projection onto trimmed CAD faces (spec node GEO.DIST.FACE).
//!
//! - [`one`] (GEO.DIST.FACE.ONE): the closest point on a single trimmed face, from the exact
//!   surface, edge curves and trimming loops.
//! - [`incid`] (GEO.DIST.FACE.INCID): the faces incident at a foot point (interior, edge or
//!   vertex), their outward normals and incident angles, and the CAD signed distance.
//! - [`body`] (GEO.DIST.FACE.BODY): GEO.DIST.BATCH projection targets for one face, a face
//!   set, a whole body and a placed instance: exact face bounds, pruned candidate search,
//!   the lowest-face-ID tie rule, the sign, and projection along a direction.

pub mod body;
pub mod incid;
pub mod one;
