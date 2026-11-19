//! The `Device@1` base package that every capability package imports. Spec node
//! `HAL.IDL.BASE`; its leaves land one at a time.
//!
//! - [`config`]: typed configuration, the acquisition-relevant subset and the configuration
//!   digest (`HAL.IDL.BASE.CFG`).

pub mod config;
