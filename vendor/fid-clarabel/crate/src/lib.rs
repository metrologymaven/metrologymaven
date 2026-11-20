// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
// Add a stylish Clarabel cow and gear logo to the docs
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/oxfordcontrol/ClarabelDocs/main/docs/src/assets/cow-and-gear-logo.png",
    html_favicon_url = "https://raw.githubusercontent.com/oxfordcontrol/ClarabelDocs/main/docs/src/assets/favicon.ico"
)]

//! <p align="center">
//! <picture>
//! <source srcset="https://github.com/oxfordcontrol/ClarabelDocs/blob/main/docs/src/assets/logo-banner-dark-rs.png?raw=true" media="(prefers-color-scheme: dark)" width=50% >
//! <img src="https://github.com/oxfordcontrol/ClarabelDocs/blob/main/docs/src/assets/logo-banner-light-rs.png?raw=true" width=50% >
//! </picture>
//! </p>
//!
//!  __Clarabel.rs__ is a Rust implementation of an interior point numerical solver for convex optimization problems using a novel homogeneous embedding.  Clarabel solves the following problem:
//!
//! $$
//! \begin{array}{rl}
//! \text{minimize} & \frac{1}{2}x^T P x + q^T x\\\\\[2ex\]
//!  \text{subject to} & Ax + s = b \\\\\[1ex\]
//!         & s \in \mathcal{K}
//!  \end{array}
//! $$
//!
//!
//! with decision variables
//! $x \in \mathbb{R}^n$,
//! $s \in \mathbb{R}^m$
//! and data matrices
//! $P=P^\top \succeq 0$,
//! $q \in \mathbb{R}^n$,
//! $A \in \mathbb{R}^{m \times n}$, and
//! $b \in \mathbb{R}^m$.
//! The convex set $\mathcal{K}$ is a composition of convex cones.
//!
//! __For installation, tutorials and examples see the Clarabel User's Guide ([stable](https://oxfordcontrol.github.io/ClarabelDocs/stable) |  [dev](https://oxfordcontrol.github.io/ClarabelDocs/dev)).__
//!
//! Clarabel is also available in a Julia implementation.  See [here](https://github.com/oxfordcontrol/Clarabel.jl).
//!
//! ## Features
//!
//! * __Scope of this fork__ (`fid-clarabel`, Metrology Maven kernel): linear programs (LPs), quadratic programs (QPs) and second-order cone programs (SOCPs) only, over zero, nonnegative and second-order cones. Upstream's exponential, power, generalized power and semidefinite cones, its Python and Julia interfaces, printing, timers, file I/O and optional linear solvers are removed (patch 0001 of `vendor/fid-clarabel/patches/`).
//!
//! * __Quadratic objectives__: Unlike interior point solvers based on the standard homogeneous self-dual embedding (HSDE), Clarabel.rs handles quadratic objectives without requiring any epigraphical reformulation of the objective.   It can therefore be significantly faster than other HSDE-based solvers for problems with quadratic objective functions.
//!
//! * __Infeasibility detection__: Infeasible problems are detected using a homogeneous embedding technique.
//!
//! # License
//!
//! Licensed under Apache License, Version 2.0.  [LICENSE](https://github.com/oxfordcontrol/Clarabel.rs/blob/main/LICENSE.md)

//Rust hates greek characters
#![allow(confusable_idents)]
// Vendored upstream code is kept close to upstream: clippy's style, complexity and perf
// groups are allowed; correctness, suspicious and the workspace lints (no unsafe, no
// unwrap/expect/panic) apply.
#![allow(clippy::style, clippy::complexity, clippy::perf)]
#![warn(missing_docs)]

pub mod algebra;
pub mod qdldl;
pub mod solver;

pub(crate) mod utils;
pub use crate::utils::infbounds::*;

pub(crate) const _INFINITY_DEFAULT: f64 = 1e20;
