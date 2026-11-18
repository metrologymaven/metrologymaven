//! Fixed vocabularies: WAI-ARIA 1.2 states, properties and non-abstract roles, CSS named
//! colours, and a syntactic BCP 47 language-tag check.
//!
//! The ARIA lists are transcribed from the WAI-ARIA 1.2 recommendation's role and attribute
//! indexes (names only) [unverified transcription]; deprecated but still defined attributes and
//! roles are accepted.

/// WAI-ARIA 1.2 states and properties.
pub const ARIA_ATTRS: &[&str] = &[
    "aria-activedescendant",
    "aria-atomic",
    "aria-autocomplete",
    "aria-busy",
    "aria-checked",
    "aria-colcount",
    "aria-colindex",
    "aria-colspan",
    "aria-controls",
    "aria-current",
    "aria-describedby",
    "aria-details",
    "aria-disabled",
    "aria-dropeffect",
    "aria-errormessage",
    "aria-expanded",
    "aria-flowto",
    "aria-grabbed",
    "aria-haspopup",
    "aria-hidden",
    "aria-invalid",
    "aria-keyshortcuts",
    "aria-label",
    "aria-labelledby",
    "aria-level",
    "aria-live",
    "aria-modal",
    "aria-multiline",
    "aria-multiselectable",
    "aria-orientation",
    "aria-owns",
    "aria-placeholder",
    "aria-posinset",
    "aria-pressed",
    "aria-readonly",
    "aria-relevant",
    "aria-required",
    "aria-roledescription",
    "aria-rowcount",
    "aria-rowindex",
    "aria-rowspan",
    "aria-selected",
    "aria-setsize",
    "aria-sort",
    "aria-valuemax",
    "aria-valuemin",
    "aria-valuenow",
    "aria-valuetext",
];

/// WAI-ARIA 1.2 roles an author may use (abstract roles excluded).
pub const ARIA_ROLES: &[&str] = &[
    "alert",
    "alertdialog",
    "application",
    "article",
    "banner",
    "blockquote",
    "button",
    "caption",
    "cell",
    "checkbox",
    "code",
    "columnheader",
    "combobox",
    "complementary",
    "contentinfo",
    "definition",
    "deletion",
    "dialog",
    "directory",
    "document",
    "emphasis",
    "feed",
    "figure",
    "form",
    "generic",
    "grid",
    "gridcell",
    "group",
    "heading",
    "img",
    "insertion",
    "link",
    "list",
    "listbox",
    "listitem",
    "log",
    "main",
    "marquee",
    "math",
    "menu",
    "menubar",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "meter",
    "navigation",
    "none",
    "note",
    "option",
    "paragraph",
    "presentation",
    "progressbar",
    "radio",
    "radiogroup",
    "region",
    "row",
    "rowgroup",
    "rowheader",
    "scrollbar",
    "search",
    "searchbox",
    "separator",
    "slider",
    "spinbutton",
    "status",
    "strong",
    "subscript",
    "superscript",
    "switch",
    "tab",
    "table",
    "tablist",
    "tabpanel",
    "term",
    "textbox",
    "time",
    "timer",
    "toolbar",
    "tooltip",
    "tree",
    "treegrid",
    "treeitem",
];

/// CSS named colours (CSS Color 4), lowercase, without `transparent` and `currentcolor`.
pub const NAMED_COLOURS: &[&str] = &[
    "aliceblue",
    "antiquewhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "black",
    "blanchedalmond",
    "blue",
    "blueviolet",
    "brown",
    "burlywood",
    "cadetblue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "crimson",
    "cyan",
    "darkblue",
    "darkcyan",
    "darkgoldenrod",
    "darkgray",
    "darkgreen",
    "darkgrey",
    "darkkhaki",
    "darkmagenta",
    "darkolivegreen",
    "darkorange",
    "darkorchid",
    "darkred",
    "darksalmon",
    "darkseagreen",
    "darkslateblue",
    "darkslategray",
    "darkslategrey",
    "darkturquoise",
    "darkviolet",
    "deeppink",
    "deepskyblue",
    "dimgray",
    "dimgrey",
    "dodgerblue",
    "firebrick",
    "floralwhite",
    "forestgreen",
    "fuchsia",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "gray",
    "green",
    "greenyellow",
    "grey",
    "honeydew",
    "hotpink",
    "indianred",
    "indigo",
    "ivory",
    "khaki",
    "lavender",
    "lavenderblush",
    "lawngreen",
    "lemonchiffon",
    "lightblue",
    "lightcoral",
    "lightcyan",
    "lightgoldenrodyellow",
    "lightgray",
    "lightgreen",
    "lightgrey",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightslategrey",
    "lightsteelblue",
    "lightyellow",
    "lime",
    "limegreen",
    "linen",
    "magenta",
    "maroon",
    "mediumaquamarine",
    "mediumblue",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumslateblue",
    "mediumspringgreen",
    "mediumturquoise",
    "mediumvioletred",
    "midnightblue",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "navy",
    "oldlace",
    "olive",
    "olivedrab",
    "orange",
    "orangered",
    "orchid",
    "palegoldenrod",
    "palegreen",
    "paleturquoise",
    "palevioletred",
    "papayawhip",
    "peachpuff",
    "peru",
    "pink",
    "plum",
    "powderblue",
    "purple",
    "rebeccapurple",
    "red",
    "rosybrown",
    "royalblue",
    "saddlebrown",
    "salmon",
    "sandybrown",
    "seagreen",
    "seashell",
    "sienna",
    "silver",
    "skyblue",
    "slateblue",
    "slategray",
    "slategrey",
    "snow",
    "springgreen",
    "steelblue",
    "tan",
    "teal",
    "thistle",
    "tomato",
    "turquoise",
    "violet",
    "wheat",
    "white",
    "whitesmoke",
    "yellow",
    "yellowgreen",
];

fn alpha(s: &str) -> bool {
    s.bytes().all(|c| c.is_ascii_alphabetic())
}

fn digits(s: &str) -> bool {
    s.bytes().all(|c| c.is_ascii_digit())
}

/// Syntactic check of a BCP 47 (RFC 5646) language tag: language, up to three extlangs,
/// script, region, variants, extensions and private use, in that order. Subtags are not
/// checked against the IANA registry [unverified registry coverage].
pub fn valid_bcp47(tag: &str) -> bool {
    if tag.is_empty() || tag.len() > 64 {
        return false;
    }
    let parts: Vec<&str> = tag.split('-').collect();
    if parts.iter().any(|p| p.is_empty() || p.len() > 8 || !p.bytes().all(|c| c.is_ascii_alphanumeric())) {
        return false;
    }
    let len = parts.len();
    let part = |i: usize| parts.get(i).copied().unwrap_or("");
    let first = part(0);
    if first.eq_ignore_ascii_case("x") || first.eq_ignore_ascii_case("i") {
        return len > 1;
    }
    if !alpha(first) || !matches!(first.len(), 2 | 3 | 5..=8) {
        return false;
    }
    let mut i = 1;
    let mut ext = 0;
    while i < len && ext < 3 && first.len() <= 3 && part(i).len() == 3 && alpha(part(i)) {
        i += 1;
        ext += 1;
    }
    if i < len && part(i).len() == 4 && alpha(part(i)) {
        i += 1;
    }
    if i < len && ((part(i).len() == 2 && alpha(part(i))) || (part(i).len() == 3 && digits(part(i)))) {
        i += 1;
    }
    while i < len
        && ((5..=8).contains(&part(i).len()) || (part(i).len() == 4 && part(i).bytes().next().is_some_and(|c| c.is_ascii_digit())))
    {
        i += 1;
    }
    while i < len && part(i).len() == 1 && !part(i).eq_ignore_ascii_case("x") {
        i += 1;
        let mut k = 0;
        while i < len && (2..=8).contains(&part(i).len()) {
            i += 1;
            k += 1;
        }
        if k == 0 {
            return false;
        }
    }
    if i < len && part(i).eq_ignore_ascii_case("x") {
        i += 1;
        if i >= len {
            return false;
        }
        i = len;
    }
    i == len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcp47_examples() {
        for ok in ["en", "en-US", "de-CH-1996", "zh-Hant-TW", "sr-Latn-RS", "es-419", "en-x-site", "x-private"] {
            assert!(valid_bcp47(ok), "{ok}");
        }
        for bad in ["", "e", "english-", "en_US", "en--US", "123", "en-x", "toolongtag1"] {
            assert!(!valid_bcp47(bad), "{bad}");
        }
    }
}
