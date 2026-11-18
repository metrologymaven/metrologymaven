//! Metrology Maven dataset tooling (spec node `ECO.DATA`): the dataset catalogue and
//! its admission check (`cat`, spec node `ECO.DATA.CAT`), and the fetch, cache and
//! verify tool (`fetch`, spec node `ECO.DATA.FETCH`). Repository tooling, layer `tool`:
//! no product crate depends on it.

pub mod cat;
pub mod fetch;

pub use fetch::{dataset_path, DataError};
