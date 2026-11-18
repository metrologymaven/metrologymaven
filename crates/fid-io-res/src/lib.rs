//! Result exchange of Metrology Maven (spec node `IO.RES`). This crate grows one leaf at a
//! time; today it holds [`csv::map`] (`IO.RES.CSV.MAP`): versioned column maps for result
//! tables, their table dialect, the built-in maps and the lossy check.
//!
//! Nothing here panics on any input: every outcome is a returned `Result`.

pub mod csv;
