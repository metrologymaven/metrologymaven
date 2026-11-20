// AC-5 (CORE.BUS.POL.EGRESS): the handler of a W2 command is given no NetPermit, so it cannot
// open a socket; this call does not compile. `with_permit_control.rs` is the same call with a
// permit and compiles.
use fid_bus::handler::{Event, Handler, HandlerCtx, HandlerError, StateView, ValidParams};
use std::net::SocketAddr;
use std::time::Duration;

struct W2Handler {
    addr: SocketAddr,
}

impl Handler for W2Handler {
    fn run(&self, _view: &dyn StateView, _params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let _stream = fid_net::connect_tcp(&self.addr, Duration::from_secs(1));
        Ok(Vec::new())
    }
}

fn main() {
    let _ = W2Handler { addr: SocketAddr::from(([127, 0, 0, 1], 9)) };
}
