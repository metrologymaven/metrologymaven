//! UUID value and UUIDv5. Expected IDs: RFC 9562 Appendix A.4 (checked with Python `uuid.uuid5`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_uuid::{uuid_v5, Uuid, UuidError};

#[test]
fn rfc9562_v5_example() {
    let dns = Uuid::parse("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap();
    let id = uuid_v5(&dns, b"www.example.com");
    assert_eq!(id.to_string(), "2ed6657d-e927-568b-95e1-2665a8aea6a2");
    assert_eq!(id.version(), 5);
    assert_eq!(id.variant_bits(), 0b10);
}

#[test]
fn parse_and_print() {
    let id = Uuid::parse("2ED6657D-E927-568B-95E1-2665A8AEA6A2").unwrap();
    assert_eq!("2ed6657d-e927-568b-95e1-2665a8aea6a2".parse::<Uuid>().unwrap(), id);
    assert_eq!(Uuid::from_u128(id.as_u128()), id);
    assert_eq!(Uuid::NIL.to_string(), "00000000-0000-0000-0000-000000000000");
    let bads = [
        "",
        "2ed6657d-e927-568b-95e1-2665a8aea6a",
        "2ed6657de927-568b-95e1-2665a8aea6a2x",
        "2ed6657d-e927-568b-95e1-2665a8aea6g2",
    ];
    for bad in bads {
        assert_eq!(Uuid::parse(bad), Err(UuidError), "{bad:?}");
    }
    assert_eq!(UuidError.code(), "bad_id");
}
