//! `fid-render`: the renderer core of Metrology Maven (spec node `VIZ.RENDER`).
//!
//! So far: the scene's render-ID tables ([`scene::idtab`], spec leaf `VIZ.RENDER.SCENE.IDTAB`)
//! and view-model projection with dirty-set re-projection ([`scene::proj`], spec leaf
//! `VIZ.RENDER.SCENE.PROJ`) and stale marking with the stale-first style resolver
//! ([`scene::stale`], spec leaf `VIZ.RENDER.SCENE.STALE`), all side-effect class R0 and
//! determinism class D0: no I/O, no
//! clock, no randomness, `BTreeMap` only; and the GPU frame graph ([`frame::graph`], spec leaf
//! `VIZ.RENDER.FRAME.GRAPH`): backend selection, pass registration, colour, reversed-Z depth
//! and ID targets with readback (determinism class N for pixels). The crate has no windowing or
//! UI-toolkit dependency (constitution C2; `fid-shlint` checks it). Nothing here panics
//! (CONSTITUTION §8).

pub mod frame;
pub mod scene;
