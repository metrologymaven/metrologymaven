//! Network facade of Metrology Maven (spec node `CORE.BUS.POL.EGRESS`, REQ-004).
//!
//! The bus opens no network connection except inside the execution of an X1, I1 or D1
//! command (CONSTITUTION §3.1, REQ-CORE.BUS.POL-008). This crate is where product code opens
//! sockets, and every socket-opening function here takes a [`NetPermit`]. The permit type
//! belongs to `fid-bus`, which alone can issue one (its constructor is crate-private; the
//! pipeline issues it to the handler of an executing X1, I1 or D1 command, at most once per
//! execution, and it cannot outlive that execution). A handler of any other class gets no
//! permit, so its call to a function here does not compile (AC-5): see the `compile_fail`
//! examples below, built from `tests/compile_fail/`.
//!
//! Only outgoing TCP connections are offered ([`connect_tcp`]); listening sockets, UDP and
//! name resolution are not. A lint that bans socket crates (`std::net` included) outside
//! this facade belongs to `CORE.BUS.LINT` and is not part of this crate; until it lands,
//! the rule rests on the type requirement and review.
//!
//! A W2 handler calling [`connect_tcp`] without a permit does not compile:
#![doc = "```compile_fail"]
#![doc = include_str!("../tests/compile_fail/w2_without_permit.rs")]
#![doc = "```"]
//!
//! Nor does one that tries to build a permit itself:
#![doc = "```compile_fail"]
#![doc = include_str!("../tests/compile_fail/w2_forged_permit.rs")]
#![doc = "```"]
//!
//! Nor does one that builds a permit with a struct literal:
#![doc = "```compile_fail"]
#![doc = include_str!("../tests/compile_fail/w2_struct_literal_permit.rs")]
#![doc = "```"]
//!
//! The control, the same call with a permit handed in, compiles:
#![doc = "```no_run"]
#![doc = include_str!("../tests/compile_fail/with_permit_control.rs")]
#![doc = "```"]

pub use fid_bus::policy::egress::{permits_network, NetPermit};
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Opens an outgoing TCP connection to `addr` under `permit`, giving up after `timeout`
/// (a zero timeout is refused with `InvalidInput`, as the standard library does). No name
/// resolution: the caller passes a socket address.
pub fn connect_tcp(permit: &NetPermit<'_>, addr: &SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    if !permits_network(permit.class()) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "permit is not for an X1, I1 or D1 command"));
    }
    TcpStream::connect_timeout(addr, timeout)
}
