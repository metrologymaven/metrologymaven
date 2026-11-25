//! AC-5 and the first part of AC-6 of spec node AUTO.SURF.COMPOSE, on the shipped
//! composition (`InProcessHost::for_process`). Its own test binary, because it changes the
//! process environment. Expected values are hand-labelled from the leaf and CORE.BUS.PORT
//! REQ-008.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus_api::{PortKind, PrincipalKind};
use fid_compose::{presence_provider, process_label, InProcessHost, Platform, PRESENCE_PROVIDERS};

const SPOOF: [&str; 3] = ["USER", "USERNAME", "LOGNAME"];

/// AC-5: `USER`, `USERNAME` and `LOGNAME` set to `spoofed-user` and `FID_PRINCIPAL=human`
/// change nothing: the channel identity equals the one built without them, is not
/// `spoofed-user`, and the principal kind is `script`. Then AC-6: with no provider listed for
/// this platform, `confirm_human` fails `upgrade_refused/declined` with detail
/// `no_presence_provider`, and the port stays `script`.
#[test]
fn ac5_ac6_identity_and_presence_come_from_the_os() {
    for k in SPOOF {
        std::env::set_var(k, "spoofed-user");
    }
    std::env::set_var("FID_PRINCIPAL", "human");
    let spoofed = InProcessHost::for_process(PortKind::Cli).unwrap();
    let with_vars = spoofed.channel();
    assert_eq!(spoofed.principal_kind(), PrincipalKind::Script);

    for k in SPOOF {
        std::env::remove_var(k);
    }
    std::env::remove_var("FID_PRINCIPAL");
    let mut plain = InProcessHost::for_process(PortKind::Cli).unwrap();
    let without = plain.channel();
    assert_eq!(with_vars.identity, without.identity);
    assert_ne!(with_vars.identity, "spoofed-user");
    assert!(!with_vars.identity.contains("spoofed"));
    assert_eq!(with_vars.kind, PortKind::Cli);
    assert!(with_vars.session.starts_with(&process_label()), "{}", with_vars.session);
    drop(spoofed);

    // AC-6, first part.
    assert!(PRESENCE_PROVIDERS.iter().all(|(p, _)| *p != Platform::current()));
    assert!(presence_provider(Platform::current()).is_none());
    let e = plain.confirm_human().unwrap_err();
    assert_eq!((e.code(), e.reason(), e.detail()), ("upgrade_refused", "declined", "no_presence_provider"));
    assert_eq!(plain.principal_kind(), PrincipalKind::Script);

    // REQ-016: `fid mcp` opens an `mcp` port, which starts as `agent`.
    let mcp = InProcessHost::for_process(fid_compose::port_kind_for(&["mcp".to_string()])).unwrap();
    assert_eq!(mcp.channel().kind, PortKind::Mcp);
    assert_eq!(mcp.principal_kind(), PrincipalKind::Agent);
    assert_eq!(fid_compose::port_kind_for(&["canon".to_string()]), PortKind::Cli);
}
