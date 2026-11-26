//! The composition root of Metrology Maven. Spec node `AUTO.SURF.COMPOSE`.
//!
//! The one place that links the bus, the storage layer, every domain's handlers and the
//! surfaces, and wires them together:
//!
//! - [`HANDLER_SOURCES`], [`UNWIRED`] and the coverage check ([`coverage`]): which handler serves
//!   each major of the manifest in force, and the stub of every major nothing serves yet;
//! - [`Composition`]: the project bus of an [`OpenedProject`] and the no-project bus, as
//!   `fid_bus::port::LocalBus` values;
//! - [`InProcessHost`]: the process's OS user and session host, the presence providers
//!   ([`PRESENCE_PROVIDERS`], empty, so a confirmation fails closed), the one port of the process,
//!   opening a project by handle or raw path, and the [`HostTransport`] handed to a surface;
//! - [`presence`]: the source scan that keeps unlisted `HumanPresence` implementations out of
//!   shipped code;
//! - the `fid` executable (bin target `fid`), moved here from `fid-cli`, which keeps today's
//!   behaviour until `fid-cli` takes transports (`AUTO.CLI.TARGET.SELECT`, `.HEADLESS`).
//!
//! Surfaces never compose a bus: `fid-cli` and `fid-mcp` link no CORE crate but the bus API
//! (REQ-CORE.BUS.LINT-004). No principal kind, identity or upgrade is read from environment
//! variables, argv, files or parameters (CONSTITUTION §3, D-012, D-035): a `cli`, `python`,
//! `control` or `mcp` port starts as `script` or `agent`, and becomes `human` only through a
//! listed presence provider and a `principal_upgrade` entry on the open project's journal.

mod compose;
mod host;
pub mod presence;
mod sources;

pub use compose::{
    host_time, workspace_defs, ComposedProject, Composition, OpenedProject, ProjectGuard, SystemUpgradeClock,
    NO_KERNEL_MANIFEST, RECORD_STREAM, UNFOLDED_EVENTS, WORKING_BRANCH,
};
pub use host::{
    port_kind_for, presence_provider, process_label, process_os_user, roots_from_settings, HostError, HostTransport,
    InProcessHost, Platform, ProjectRef, PRESENCE_PROVIDERS,
};
pub use sources::{
    coverage, not_wired_detail, unwired_list, FreeParts, FreeRegister, HandlerSource, ProjectParts, ProjectRegister,
    HANDLER_SOURCES, NO_PROJECT_OPEN, UNWIRED,
};
