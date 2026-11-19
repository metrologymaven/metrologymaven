// AC-1 of CORE.BUS.PIPE.HANDLER: a handler that asks for `&mut` project state must not
// build. Built as a `compile_fail,E0053` doctest on `fid_bus::handler::Handler` (a doctest
// needs no extra dependency); E0053 is "method has an incompatible type for trait".
use fid_bus::handler::{Event, Handler, HandlerCtx, HandlerError, StateView, ValidParams};

struct Writer;

impl Handler for Writer {
    fn run(&self, view: &mut dyn StateView, _: &ValidParams, _: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let _ = view.head();
        Ok(Vec::new())
    }
}

fn main() {}
