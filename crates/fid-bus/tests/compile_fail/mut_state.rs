// AC-1 of CORE.BUS.PIPE.HANDLER: built as a doctest on `fid_bus::handler::Handler`.
// This file must not build. `mut_state_control.rs` is the same file with the view taken
// by shared reference and must build; a test asserts that is their only difference.
use fid_bus::handler::{Event, Handler, HandlerCtx, HandlerError, StateView, ValidParams};

struct Writer;

impl Handler for Writer {
    fn run(&self, view: &mut dyn StateView, _: &ValidParams, _: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let _ = view.head();
        Ok(Vec::new())
    }
}

fn main() {}
