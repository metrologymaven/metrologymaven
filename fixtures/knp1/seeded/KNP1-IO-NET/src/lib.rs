//! Seeded KNP1-IO-NET violation: exactly one planted finding (KRN.NUM.LINT.SYN AC-1).

pub fn is_loopback(a: [u8; 4]) -> bool {
    std::net::Ipv4Addr::from(a).is_loopback()
}
