//! Pure-Rust in-process [`CommandBus`] router for standalone handlers and unit/integration tests.

use crate::dispatch::{CommandBus, CommandHandler};
use crate::envelope::{BusEnvelope, CommandCall};
use crate::error::BusApiErrorCode;
use fid_canon::CValue;
use std::collections::BTreeMap;

/// Deterministic in-process [`CommandBus`] dispatching to registered [`CommandHandler`]s.
#[derive(Default)]
pub struct InProcessBus {
    handlers: BTreeMap<(String, u32), Box<dyn CommandHandler>>,
}

impl InProcessBus {
    /// Creates an empty in-process bus.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a boxed [`CommandHandler`], replacing any prior handler for the same `(name, major)`.
    pub fn register(&mut self, handler: Box<dyn CommandHandler>) {
        let (name, major) = handler.id();
        let base = name.split('@').next().unwrap_or(name).to_string();
        self.handlers.insert((base, major), handler);
    }

    /// Registers a closure-backed handler producing a [`BusEnvelope`].
    pub fn register_envelope_fn<F>(&mut self, command_name: impl Into<String>, major: u32, f: F)
    where
        F: FnMut(&CommandCall) -> BusEnvelope + Send + Sync + 'static,
    {
        let base = command_name.into();
        let clean = base.split('@').next().unwrap_or(&base).to_string();
        self.handlers.insert((clean.clone(), major), Box::new(FnEnvelopeHandler { name: clean, major, f }));
    }

    /// Returns the number of registered handlers.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Returns true if no handlers are registered.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl CommandBus for InProcessBus {
    fn dispatch(&mut self, call: &CommandCall) -> BusEnvelope {
        let full = call.full_command_id();
        if let Err(err) = call.parsed_command_id() {
            return err.to_envelope(full);
        }
        let key = (call.base_name().to_string(), call.major);
        match self.handlers.get_mut(&key) {
            Some(handler) => handler.handle_envelope(call),
            None => {
                BusEnvelope::failed(full.clone(), BusApiErrorCode::NotFound, format!("unknown command `{full}`"), None)
            }
        }
    }

    fn supports(&self, command_id: &str, major: u32) -> bool {
        if let Some((base, suffix)) = command_id.split_once('@') {
            if suffix.parse::<u32>() != Ok(major) {
                return false;
            }
            return self.handlers.contains_key(&(base.to_string(), major));
        }
        self.handlers.contains_key(&(command_id.to_string(), major))
    }

    fn list_commands(&self) -> Vec<(String, u32)> {
        self.handlers.keys().cloned().collect()
    }
}

struct FnEnvelopeHandler<F> {
    name: String,
    major: u32,
    f: F,
}

impl<F> CommandHandler for FnEnvelopeHandler<F>
where
    F: FnMut(&CommandCall) -> BusEnvelope + Send + Sync,
{
    fn id(&self) -> (&str, u32) {
        (&self.name, self.major)
    }

    fn handle(&mut self, call: &CommandCall) -> Result<CValue, crate::error::BusApiError> {
        let env = (self.f)(call);
        match env.result {
            Some(v) if env.error_code.is_none() => Ok(v),
            _ if env.error_code.is_none() => Ok(CValue::Null),
            _ => Err(crate::error::BusApiError::new(
                env.error_code.unwrap_or(BusApiErrorCode::InternalError),
                env.message.unwrap_or_else(|| "command failed".to_string()),
            )),
        }
    }

    fn handle_envelope(&mut self, call: &CommandCall) -> BusEnvelope {
        (self.f)(call)
    }
}
