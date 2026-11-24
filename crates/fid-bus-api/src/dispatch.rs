//! Surface-facing bus and handler traits (`CommandBus` and `CommandHandler`).

use crate::envelope::{BusEnvelope, BusOutcome, CommandCall};
use crate::error::BusApiError;
use fid_canon::CValue;

/// Trait implemented by command bus providers (`fid-bus` adapter, `InProcessBus`, or
/// control-channel / test transports) and consumed by surfaces (`fid-cli`, `fid-mcp`).
pub trait CommandBus {
    /// Dispatches a command call and returns a complete [`BusEnvelope`].
    fn dispatch(&mut self, call: &CommandCall) -> BusEnvelope;

    /// Returns true if this bus knows how to execute `(command_id, major)`.
    fn supports(&self, command_id: &str, major: u32) -> bool;

    /// Lists registered `(command_id, major)` pairs if known.
    fn list_commands(&self) -> Vec<(String, u32)> {
        Vec::new()
    }
}

/// Pure-Rust handler contract for registering commands into [`crate::InProcessBus`].
pub trait CommandHandler: Send + Sync {
    /// Returns `(command_name, major)` without the `@major` suffix.
    fn id(&self) -> (&str, u32);

    /// Executes the command and returns its result payload or a [`BusApiError`].
    fn handle(&mut self, call: &CommandCall) -> Result<CValue, BusApiError>;

    /// Optional rich-envelope hook (defaults to wrapping [`Self::handle`] as `Computed` or
    /// `DryRun` when `call.options.dry_run` is set).
    fn handle_envelope(&mut self, call: &CommandCall) -> BusEnvelope {
        let full = call.full_command_id();
        match self.handle(call) {
            Ok(val) => {
                if call.options.dry_run {
                    BusEnvelope {
                        outcome: BusOutcome::DryRun,
                        result: Some(val),
                        ..BusEnvelope::bare(full, BusOutcome::DryRun)
                    }
                } else {
                    BusEnvelope::computed(full, val)
                }
            }
            Err(err) => err.to_envelope(full),
        }
    }
}
