//! The command bus of Metrology Maven: every surface invokes commands through it
//! (CONSTITUTION C3). Modules land one spec leaf at a time; see `spec/core/bus/`.

pub mod error;
pub mod handler;
pub mod policy;
pub mod principal;
mod unicode16;
pub mod unit;

/// Short alias for [`error`].
pub use self::error as err;

pub use error::{
    bus_error_schema, bus_errors_digest, bus_errors_json, catalog_value, check_compatible, current_catalog,
    parse_catalog, BusError, Catalog, CatalogError, Code, Detail, Removal, Untrusted, MAX_DETAIL_CHARS, SCHEMA_VERSION,
    TRUNCATION_MARKER,
};
pub use handler::{
    canonical_len, effects_check, run_handler, CancelSource, CancelToken, CheckedEvents, EntityId, EntityRef, Event,
    EventType, Handler, HandlerCtx, HandlerError, HandlerSet, SettingKey, StateView, ValidParams, MAX_EVENTS,
    MAX_EVENT_BODY_BYTES,
};
pub use policy::class::{
    authorise_cancel, authorise_class, class_rule, policy_matrix_json, policy_matrix_value, ClassRule,
};
pub use policy::egress::{
    permits_network, ApprovalDialog, Cleared, Destination, DestinationId, DestinationKind, Destinations,
    EgressApproval, EgressDecision, EgressPrompt, EgressSender, HttpMethod, HttpsUrl, NetPermit, NetworkRootTransport,
    Outcome, PermitScope, SendOutcome, SendParams, Sent, Transport, Transports,
};
pub use policy::projection::{
    authorise, authorise_projection, list_requests, Approval, Authorised, Decision, Filing, Projected, RejectHandler,
    RequestBook, RequestId, RequestRecord, RequestState, WithdrawHandler,
};
pub use principal::{
    Channel, ChannelKind, ClientId, ConfirmMethod, ConfirmedUpgrade, HumanPresence, IdError, Identity, JournalAppend,
    OsUser, PendingUpgrade, PresenceOutcome, Principal, PrincipalId, PrincipalKind, PrincipalRef, SelfAsserted,
    SessionId, SessionKey, SessionTable, UpgradeClock, UpgradeContext, UpgradeGrant, UpgradeRequest, BLANK_CHARS,
    DEFAULT_IGNORABLE_RANGES, FORMAT_RANGES, MAX_ID_BYTES, MAX_SELF_ASSERTED_CHARS, MAX_UPGRADED_SESSIONS,
    PRIVATE_USE_RANGES, UNASSIGNED_RANGES, UPGRADE_DEADLINE,
};
pub use unit::{
    algorithm_id, normalise, normalise_value, AuthoredLiteral, DecimalString, Normalised, UnitSymbol, ALGORITHM,
    MAX_EXPONENT, MAX_LITERAL_BYTES, MAX_SIG_DIGITS, MIN_EXPONENT,
};
