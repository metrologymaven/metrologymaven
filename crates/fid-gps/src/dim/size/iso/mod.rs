//! ISO 14405-1 sizes (spec node `GDT.DIM.SIZEISO`). Today only [`rank`]
//! (`GDT.DIM.SIZEISO.RANK`): the rank-order statistics over a list of local sizes. Local,
//! global and calculated sizes and the ISO dispatch path are other leaves.

pub mod rank;
