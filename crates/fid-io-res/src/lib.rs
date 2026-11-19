//! Result exchange of Metrology Maven (spec node `IO.RES`). This crate grows one leaf at a
//! time; today it holds [`csv::map`] (`IO.RES.CSV.MAP`): versioned column maps for result
//! tables, their table dialect, the built-in maps and the lossy check; and [`csv::out`]
//! (`IO.RES.CSV.OUT`): result table export through a column map.
//!
//! Nothing here panics on any input: every outcome is a returned `Result`.

pub mod csv;
