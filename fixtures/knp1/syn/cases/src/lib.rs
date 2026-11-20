//! Hand-labelled cases for KRN.NUM.LINT.SYN. A trailing `//~` marker lists every finding the
//! line must yield as `RULE=token` (the column is that of the token's first occurrence on the
//! line, or of its n-th with `token#n`); `note:RULE=token` is an informational opt-out note.
//! A line without a marker must yield nothing.

pub mod alias;
pub mod cfgs;
pub mod facade;
pub mod facade_impl;
pub mod inner;
pub mod literals;
pub mod macros;
pub mod minmax;
pub mod review;
pub mod review2;
pub mod review3;
pub mod review4;
pub mod misc;
pub mod shadow;
