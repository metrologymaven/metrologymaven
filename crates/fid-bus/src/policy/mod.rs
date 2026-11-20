//! The authorise stage of the bus (spec node `CORE.BUS.POL`). Checks land one leaf at a
//! time: [`class`] (class and principal matrix) and [`fs`] (file handles and approved roots).

pub mod class;
pub mod fs;
