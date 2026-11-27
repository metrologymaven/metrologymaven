//! Minimal deterministic ODF Text packaging: a ZIP archive with every member stored
//! (method 0, no compression), so the bytes depend only on the member contents.
//!
//! Layout (all integers little-endian):
//! - members, in this order: `mimetype` (`application/vnd.oasis.opendocument.text`, no
//!   newline, first and stored as ODF requires), `META-INF/manifest.xml` ([`MANIFEST_XML`]),
//!   `content.xml`;
//! - each local header: signature `0x04034b50`, version needed 10, flags 0, method 0,
//!   time 0 and date `0x0021` (1980-01-01 00:00, the DOS epoch), CRC-32 (IEEE, reflected
//!   polynomial `0xEDB88320`), both sizes equal to the member length, name length, extra
//!   length 0, then the name and the data;
//! - each central-directory entry: signature `0x02014b50`, version made by 20, version
//!   needed 10, flags, method, time, date, CRC and sizes as above, name length, extra,
//!   comment, disk number, internal and external attributes all 0, the local-header offset,
//!   then the name;
//! - the end-of-central-directory record: signature `0x06054b50`, disk numbers 0, the entry
//!   count twice, the central-directory size and offset, comment length 0.
//!
//! No timestamps, permissions, extra fields or data descriptors are written. A package that
//! would need ZIP64 is refused ([`TplError::Limit`]), never silently truncated.

use super::TplError;

/// The ODF Text media type, stored as the `mimetype` member.
pub const MIMETYPE: &str = "application/vnd.oasis.opendocument.text";

/// The `META-INF/manifest.xml` member.
pub const MANIFEST_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\n\
<manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" \
manifest:media-type=\"application/vnd.oasis.opendocument.text\"/>\n\
<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\n\
</manifest:manifest>\n";

const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = 0x0021;
const VERSION_NEEDED: u16 = 10;
const VERSION_MADE_BY: u16 = 20;

/// CRC-32 (IEEE 802.3, reflected, initial and final XOR `0xFFFFFFFF`), bitwise.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn u16_le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32_le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn len32(n: usize, what: &str) -> Result<u32, TplError> {
    u32::try_from(n).ok().filter(|v| *v < u32::MAX).ok_or_else(|| TplError::Limit(format!("{what} needs ZIP64")))
}

/// Writes a stored ZIP archive of `members` (name, data) in the given order.
pub fn zip_stored(members: &[(&str, &[u8])]) -> Result<Vec<u8>, TplError> {
    let count = u16::try_from(members.len())
        .ok()
        .filter(|n| *n < u16::MAX)
        .ok_or_else(|| TplError::Limit("too many ZIP members".to_string()))?;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in members {
        let offset = len32(out.len(), "archive")?;
        let name_len = u16::try_from(name.len()).map_err(|_| TplError::Limit("member name too long".to_string()))?;
        let size = len32(data.len(), "member")?;
        let crc = crc32(data);
        u32_le(&mut out, 0x0403_4b50);
        u16_le(&mut out, VERSION_NEEDED);
        u16_le(&mut out, 0);
        u16_le(&mut out, 0);
        u16_le(&mut out, DOS_TIME);
        u16_le(&mut out, DOS_DATE);
        u32_le(&mut out, crc);
        u32_le(&mut out, size);
        u32_le(&mut out, size);
        u16_le(&mut out, name_len);
        u16_le(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        u32_le(&mut central, 0x0201_4b50);
        u16_le(&mut central, VERSION_MADE_BY);
        u16_le(&mut central, VERSION_NEEDED);
        u16_le(&mut central, 0);
        u16_le(&mut central, 0);
        u16_le(&mut central, DOS_TIME);
        u16_le(&mut central, DOS_DATE);
        u32_le(&mut central, crc);
        u32_le(&mut central, size);
        u32_le(&mut central, size);
        u16_le(&mut central, name_len);
        u16_le(&mut central, 0);
        u16_le(&mut central, 0);
        u16_le(&mut central, 0);
        u16_le(&mut central, 0);
        u32_le(&mut central, 0);
        u32_le(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = len32(out.len(), "archive")?;
    let cd_size = len32(central.len(), "central directory")?;
    out.extend_from_slice(&central);
    len32(out.len(), "archive")?;
    u32_le(&mut out, 0x0605_4b50);
    u16_le(&mut out, 0);
    u16_le(&mut out, 0);
    u16_le(&mut out, count);
    u16_le(&mut out, count);
    u32_le(&mut out, cd_size);
    u32_le(&mut out, cd_offset);
    u16_le(&mut out, 0);
    Ok(out)
}

/// Packages a `content.xml` as an ODF Text document.
pub fn package(content_xml: &str) -> Result<Vec<u8>, TplError> {
    zip_stored(&[
        ("mimetype", MIMETYPE.as_bytes()),
        ("META-INF/manifest.xml", MANIFEST_XML.as_bytes()),
        ("content.xml", content_xml.as_bytes()),
    ])
}
