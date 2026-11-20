// AC-5 (CORE.BUS.POL.EGRESS): a W2 handler cannot build a NetPermit with a struct literal: its
// fields are private to fid-bus. `with_permit_control.rs` is the control.
use fid_bus::handler::{Event, Handler, HandlerCtx, HandlerError, StateView, ValidParams};
use fid_bus::policy::egress::NetPermit;
use fid_types::SideEffect;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::time::Duration;

struct W2Handler {
    addr: SocketAddr,
}

impl Handler for W2Handler {
    fn run(&self, _view: &dyn StateView, _params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let permit = NetPermit { class: SideEffect::X1, _scope: PhantomData };
        let _stream = fid_net::connect_tcp(&permit, &self.addr, Duration::from_secs(1));
        Ok(Vec::new())
    }
}

fn main() {
    let _ = W2Handler { addr: SocketAddr::from(([127, 0, 0, 1], 9)) };
}
