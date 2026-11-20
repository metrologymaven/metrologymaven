//! The authorise stage of the bus (spec node `CORE.BUS.POL`). Checks land one leaf at a
//! time: [`class`] (class and principal matrix), [`fs`] (file handles and approved roots) and
//! [`projection`] (MCP projection, pending requests and approval), and [`egress`] (the X1
//! egress primitive and the network permit).

pub mod class;
pub mod egress;
pub mod fs;
pub mod projection;
