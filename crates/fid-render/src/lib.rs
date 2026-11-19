//! `fid-render`: the renderer core of Metrology Maven (spec node `VIZ.RENDER`).
//!
//! So far: the scene's render-ID tables ([`scene::idtab`], spec leaf `VIZ.RENDER.SCENE.IDTAB`)
//! and view-model projection with dirty-set re-projection ([`scene::proj`], spec leaf
//! `VIZ.RENDER.SCENE.PROJ`). The crate is side-effect class R0 and determinism class D0: no
//! I/O, no clock, no randomness, `BTreeMap` only. It has no GPU, windowing or UI-toolkit
//! dependency (constitution C2; `fid-shlint` checks it). Nothing here panics (CONSTITUTION §8).

pub mod scene;
