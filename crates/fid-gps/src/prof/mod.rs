//! The profile family (spec node `GDT.PROF`). Today it holds [`dev`] (`GDT.PROF.DEV`): the
//! per-point signed deviations from the nominal CAD faces, with each face's nominal
//! approximation error, that the profile and runout zones consume.

pub mod dev;
