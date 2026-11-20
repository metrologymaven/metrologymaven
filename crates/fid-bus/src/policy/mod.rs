//! The authorise stage of the bus (spec node `CORE.BUS.POL`). Checks land one leaf at a
//! time: [`class`] (class and principal matrix), [`fs`] (file handles and approved roots) and
//! [`projection`] (MCP projection, pending requests and approval).

pub mod class;
pub mod fs;
pub mod projection;
