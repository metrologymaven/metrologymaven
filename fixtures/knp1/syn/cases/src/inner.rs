#![cfg(not(clippy))] //~ KNP1-CFG=clippy
#![cfg_attr(debug_assertions, allow(unused))] //~ KNP1-CFG=debug_assertions
//! Re-review 3 of PR #235: inner attributes go through the same cfg checks.

pub fn n() {}
