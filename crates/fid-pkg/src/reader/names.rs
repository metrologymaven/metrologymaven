//! Entry-name rules beyond `PkgPath` (which already refuses absolute paths, `.` and `..`
//! segments, backslashes, NUL, drive prefixes, non-NFC text and over-long names).
//!
//! The reader never writes files, but the names it accepts are handed to verifiers, shown to
//! people and may be extracted by other tools. So it also refuses:
//!
//! - `control_char`: C0 controls, DEL and C1 controls;
//! - `lookalike`: a character whose compatibility decomposition (NFKC) contains `/`, `\`,
//!   `.` or `:` (fullwidth dots and solidi, one-dot leaders, ...), a slash-like symbol that
//!   has no such decomposition (division slash, fraction slash, big solidus, set minus, ...),
//!   or an invisible formatting character (zero-width spaces and joiners, bidirectional
//!   overrides and isolates, the byte-order mark, soft hyphen).
//!
//! - `windows_name`: a name no Windows file system can hold as written, so extraction there
//!   would rename, merge or redirect it: a `:` anywhere (alternate data streams), a segment
//!   ending in `.` or a space (silently stripped), or a segment whose stem (the part before
//!   the first `.`, trailing spaces removed) is a device name in any letter case: `CON`, `PRN`,
//!   `AUX`, `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9`, `COM` or `LPT` with a superscript one, two
//!   or three, `CONIN$` and `CONOUT$` (so `nul.json` and `Com1 .txt` are refused too).
//!
//! Case-colliding names are refused separately, over the whole entry set (`case_collision`,
//! `path_conflict` in the parent module).

use unicode_normalization::char::decompose_compatible;

/// Slash- and dot-like symbols without a compatibility decomposition to ASCII.
const SLASH_LIKE: [char; 9] =
    ['\u{2044}', '\u{2215}', '\u{2216}', '\u{2571}', '\u{2572}', '\u{29F5}', '\u{29F8}', '\u{29F9}', '\u{3033}'];

/// Invisible formatting characters.
fn invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}' | '\u{180E}'
        | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}'
        | '\u{3164}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}' | '\u{FFA0}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Checks one entry name; the error is the reason tag.
pub(crate) fn check_name(name: &str) -> Result<(), &'static str> {
    for c in name.chars() {
        if c.is_control() {
            return Err("control_char");
        }
        if c.is_ascii() {
            continue;
        }
        if SLASH_LIKE.contains(&c) || invisible(c) {
            return Err("lookalike");
        }
        let mut hit = false;
        decompose_compatible(c, |d| hit |= matches!(d, '/' | '\\' | '.' | ':'));
        if hit {
            return Err("lookalike");
        }
    }
    if name.contains(':') {
        return Err("windows_name");
    }
    for seg in name.split('/') {
        if seg.ends_with('.') || seg.ends_with(' ') || is_device(seg) {
            return Err("windows_name");
        }
    }
    Ok(())
}

/// Whether a segment's stem is a Windows device name.
fn is_device(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or_default().trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => {
            let rest = upper.strip_prefix("COM").or_else(|| upper.strip_prefix("LPT"));
            matches!(rest, Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "\u{b9}" | "\u{b2}" | "\u{b3}"))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn accepted() {
        for ok in [
            "results/records.json",
            "results/caf\u{e9} \u{fc}\u{df}er.json",
            "\u{65e5}\u{672c}/x",
            "results/console.json",
            "results/com10.json",
            "results/lpt0",
            "results/nul_x.json",
            "results/a.b.c",
            "results/.hidden",
            "results/ a",
        ] {
            assert_eq!(check_name(ok), Ok(()), "{ok:?}");
        }
    }

    #[test]
    fn refused() {
        for (name, why) in [
            ("a\u{1}b", "control_char"),
            ("a\u{7f}", "control_char"),
            ("a\u{85}", "control_char"),
            ("a\u{ff0e}\u{ff0e}", "lookalike"),
            ("a\u{ff0f}b", "lookalike"),
            ("a\u{fe68}b", "lookalike"),
            ("a\u{2215}b", "lookalike"),
            ("a\u{2044}b", "lookalike"),
            ("a\u{2024}", "lookalike"),
            ("a\u{2488}", "lookalike"),
            ("a\u{ff1a}", "lookalike"),
            ("a\u{202e}b", "lookalike"),
            ("a\u{200b}", "lookalike"),
            ("\u{feff}a", "lookalike"),
            ("a:b", "windows_name"),
            ("results/x.json:stream", "windows_name"),
            ("results/x.", "windows_name"),
            ("results/x ", "windows_name"),
            ("results/dir./x", "windows_name"),
            ("results/CON", "windows_name"),
            ("results/con.json", "windows_name"),
            ("results/Nul.tar.gz", "windows_name"),
            ("results/aux", "windows_name"),
            ("results/prn.txt", "windows_name"),
            ("results/COM1", "windows_name"),
            ("results/lpt9.json", "windows_name"),
            ("results/com\u{b9}.json", "windows_name"),
            ("results/Com1 .txt", "windows_name"),
            ("results/conin$", "windows_name"),
            ("nul/x.json", "windows_name"),
        ] {
            assert_eq!(check_name(name), Err(why), "{name:?}");
        }
    }
}
