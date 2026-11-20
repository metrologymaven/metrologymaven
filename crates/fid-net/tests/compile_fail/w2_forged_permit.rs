// AC-5 (CORE.BUS.POL.EGRESS): a W2 handler cannot make a permit of its own: NetPermit has
// private fields and the bus's issuing function is crate-private. Each of the two lines that
// try is an error; `with_permit_control.rs` is the control.
use fid_bus::handler::{Event, Handler, HandlerCtx, HandlerError, StateView, ValidParams};
use fid_bus::policy::egress::{NetPermit, PermitScope};
use std::net::SocketAddr;
use std::time::Duration;

struct W2Handler {
    addr: SocketAddr,
}

impl Handler for W2Handler {
    fn run(&self, _view: &dyn StateView, _params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let mut scope = PermitScope::new();
        let permit: NetPermit<'_> = fid_bus::policy::egress::issue_permit(todo!(), &mut scope).unwrap();
        let _stream = fid_net::connect_tcp(&permit, &self.addr, Duration::from_secs(1));
        Ok(Vec::new())
    }
}

fn main() {
    let _ = W2Handler { addr: SocketAddr::from(([127, 0, 0, 1], 9)) };
}
