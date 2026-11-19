// AC-1 of CORE.BUS.PIPE.HANDLER: one of a pair of doctests on `fid_bus::handler::Handler`.
// In `mut_state.rs` the handler takes the view by mutable reference and must not build;
// in `mut_state_control.rs` it takes it by shared reference and must build. A test
// asserts the two files differ only in that reference.
use fid_bus::handler::{Event, Handler, HandlerCtx, HandlerError, StateView, ValidParams};

struct Writer;

impl Handler for Writer {
    fn run(&self, view: &dyn StateView, _: &ValidParams, _: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let _ = view.head();
        Ok(Vec::new())
    }
}

fn main() {}
