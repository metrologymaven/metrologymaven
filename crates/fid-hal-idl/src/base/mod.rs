//! The `Device@1` base package that every capability package imports. Spec node
//! `HAL.IDL.BASE`; its leaves land one at a time.
//!
//! - [`device`]: the `Device@1` package itself: seven operations, `DeviceInstance`,
//!   calibration-state references and the `Device-core@1` profile (`HAL.IDL.BASE.DESC`).
//! - [`config`]: typed configuration, the acquisition-relevant subset and the configuration
//!   digest (`HAL.IDL.BASE.CFG`).
//! - [`error`]: the closed device error model, untrusted device text and operator hint IDs
//!   (`HAL.IDL.BASE.ERR`).
//! - [`health`]: the 10-state health machine, closed transition table and warm-up mapping
//!   (`HAL.IDL.BASE.HEALTH`).

pub mod config;
pub mod device;
pub mod error;
pub mod health;
