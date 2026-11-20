// Control for the AC-5 compile_fail cases (CORE.BUS.POL.EGRESS): the same socket call, made
// by code that was handed a permit (as the pipeline hands one to an X1, I1 or D1 handler),
// compiles.
use fid_bus::policy::egress::NetPermit;
use std::net::SocketAddr;
use std::time::Duration;

struct X1Step {
    addr: SocketAddr,
}

impl X1Step {
    fn run(&self, permit: &NetPermit<'_>) {
        let _stream = fid_net::connect_tcp(permit, &self.addr, Duration::from_secs(1));
    }
}

fn main() {
    let step = X1Step { addr: SocketAddr::from(([127, 0, 0, 1], 9)) };
    let _ = &step;
    let _ = X1Step::run;
}
