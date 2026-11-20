#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_types)]
//! RPT.OUTPUT.PDF.TEXT acceptance tests. Expected values come from
//! `fixtures/rpt/output/text/make_fixtures.py` (an independent Python implementation of the
//! spec's rules) or are hand-labelled (`ac4-*.txt`/`.cmap`, the literals below); none is
//! produced by this crate. Fixtures are compiled in, so the same tests run on x86-64,
//! ARM64 and wasm32 (AC-1).

use fid_num::fmt::{len::format, FormatSpec, FormatterId, Precision, Unit};
use fid_report_pdf::text::pdf::{
    escape_literal, escape_name, fmt_milli, milli_points, single_page, text_object, PlacedRun, MAX_CONTENT_BYTES,
};
use fid_report_pdf::text::{
    break_opportunities, bundled_font_bytes, bundled_font_file, bundled_manifest, check_licences, spdx_ids,
    DefaultStyle, FontEntry, FontId, FontManifest, FontSet, LoadedFont, Shaped, Slant, SubsetBuilder, TextError,
    TextWarning, MAX_TEXT_BYTES,
};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};

const STRINGS_EXPECTED: &str = include_str!("../../../fixtures/rpt/output/text/expected-shapes.txt");
const SUBSETS_EXPECTED: &str = include_str!("../../../fixtures/rpt/output/text/expected-subsets.txt");
const SCRIPT_FONT: &[u8] = include_bytes!("../../../fixtures/rpt/output/text/script-test.ttf");
const SHARED_2048_FONT: &[u8] = include_bytes!("../../../fixtures/rpt/output/text/shared-2048-test.ttf");
const COMPOSITE_FONT: &[u8] = include_bytes!("../../../fixtures/rpt/output/text/composite-test.ttf");
const AC4_TEXT_OBJECT: &[u8] = include_bytes!("../../../fixtures/rpt/output/text/ac4-text-object.txt");
const AC4_CMAP: &[u8] = include_bytes!("../../../fixtures/rpt/output/text/ac4-tounicode.cmap");
const AC4_PDF: &[u8] = include_bytes!("../../../fixtures/rpt/output/text/ac4-value.pdf");
const MIXED_PDF: &[u8] = include_bytes!("../../../fixtures/rpt/output/text/mixed.pdf");
const DENY_TOML: &str = include_str!("../../../deny.toml");
const SPDX_ALLOWLIST: &str = include_str!("../../../house-rules/spdx-allowlist.toml");
const ATTRIBUTION: &str = include_str!("../../../ATTRIBUTION.md");

/// Digests of the test fonts (written by hand from `sha256sum`; `make_fonts.py --check`
/// keeps the files stable).
const SCRIPT_SHA: &str = "sha256:7cd8b1b46f6221ada7e99fcd2952837d47a589e59bc9a488a956382b504e38c7";
const SHARED_2048_SHA: &str = "sha256:9d306daea3ef9705bf917ec02a4dfa9236541e7750553612ad345a9356b40544";
const COMPOSITE_SHA: &str = "sha256:06625ffd9b653667da6e07f1be4fd6731397c989c542d813f195e2b335257edb";

const SIZE_UM: i64 = 3528;
const A4: (i64, i64) = (210_000, 297_000);

fn id(s: &str) -> FontId {
    FontId::new(s).unwrap()
}

fn entry(fid: &str, family: &str, ps: &str, sha: &str) -> FontEntry {
    FontEntry {
        id: id(fid),
        family: family.to_string(),
        weight: 400,
        style: Slant::Upright,
        sha256: Digest::parse(sha).unwrap(),
        spdx: "MIT OR Apache-2.0".to_string(),
        postscript_name: ps.to_string(),
    }
}

fn script_manifest() -> FontManifest {
    FontManifest {
        fonts: vec![entry("script-test", "Maven Script Test", "MavenScriptTest-Regular", SCRIPT_SHA)],
        fallback: vec![id("script-test")],
    }
}

fn fonts_with_script() -> FontSet {
    FontSet::bundled_with_script_fonts(&script_manifest(), &[("script-test", SCRIPT_FONT)]).unwrap()
}

fn unhex(s: &str) -> String {
    let bytes: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect();
    String::from_utf8(bytes).unwrap()
}

fn expected_subset(key: &str) -> &'static str {
    SUBSETS_EXPECTED
        .lines()
        .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(' ')))
        .unwrap_or_else(|| panic!("no {key} in expected-subsets.txt"))
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// Decodes a run through the subset's ToUnicode entries, as a PDF reader would.
fn read_back(shaped: &Shaped, subsets: &[&fid_report_pdf::text::Subset]) -> String {
    let mut out = String::new();
    for r in &shaped.runs {
        let s = subsets.iter().find(|s| s.font == r.font).unwrap();
        for g in s.encode_run(r).unwrap() {
            let c = s.to_unicode.entries().iter().find(|(x, _)| *x == g).map(|(_, c)| *c).unwrap_or('\u{FFFD}');
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// AC-1: shaping is identical on every target, and a missing glyph is reported.

#[test]
fn ac1_shaping_matches_reference() {
    let set = fonts_with_script();
    let mut cases = 0;
    let mut lines = STRINGS_EXPECTED.lines().filter(|l| !l.starts_with('#')).peekable();
    while let Some(l) = lines.next() {
        let name = l.strip_prefix("case ").expect("case line");
        let text = unhex(lines.next().unwrap().strip_prefix("text ").unwrap());
        let font = id(lines.next().unwrap().strip_prefix("font ").unwrap());
        let size: i64 = lines.next().unwrap().strip_prefix("size ").unwrap().parse().unwrap();
        let got = set.shape(&font, &text, size).unwrap();
        let mut run_i = 0usize;
        let mut glyph_i = 0usize;
        let mut warn_i = 0usize;
        for l in lines.by_ref() {
            let f: Vec<&str> = l.split(' ').collect();
            match f[0] {
                "run" => {
                    if run_i > 0 || glyph_i > 0 {
                        assert_eq!(got.runs[run_i].glyphs.len(), glyph_i, "{name}: glyph count of run {run_i}");
                        run_i += 1;
                    }
                    glyph_i = 0;
                    let r = &got.runs[run_i];
                    assert_eq!(r.font.as_str(), f[1], "{name}: font of run {run_i}");
                    assert_eq!(r.start, f[2].parse::<u32>().unwrap(), "{name}: start of run {run_i}");
                    assert_eq!(r.text, unhex(f[3]), "{name}: text of run {run_i}");
                    assert_eq!(r.size_um, size);
                }
                "g" => {
                    let g = got.runs[run_i].glyphs[glyph_i];
                    let want =
                        (f[1].parse::<u16>().unwrap(), f[2].parse::<i64>().unwrap(), f[3].parse::<u32>().unwrap());
                    assert_eq!((g.glyph, g.advance_um, g.cluster), want, "{name}: run {run_i} glyph {glyph_i}");
                    glyph_i += 1;
                }
                "warn" => {
                    let w = &got.warnings[warn_i];
                    let TextWarning::GlyphMissing { code_point, cluster, font: wf } = w else {
                        panic!("{name}: unexpected warning {w}");
                    };
                    assert_eq!(*code_point, u32::from_str_radix(f[1], 16).unwrap(), "{name}: warning {warn_i}");
                    assert_eq!(*cluster, f[2].parse::<u32>().unwrap(), "{name}: warning {warn_i}");
                    assert_eq!(wf, &font);
                    assert_eq!(w.code(), "report.glyph_missing");
                    warn_i += 1;
                }
                "end" => break,
                other => panic!("unexpected line {other}"),
            }
        }
        assert_eq!(got.runs.len(), run_i + 1, "{name}: run count");
        assert_eq!(got.runs[run_i].glyphs.len(), glyph_i, "{name}: glyph count of last run");
        assert_eq!(got.warnings.len(), warn_i, "{name}: warning count");
        assert_eq!(got.text(), text, "{name}: characters unchanged");
        cases += 1;
    }
    assert_eq!(cases, 6);
}

#[test]
fn ac1_missing_glyph_reports_code_point() {
    let set = FontSet::bundled().unwrap();
    let got = set.shape(&id("sans-regular"), "A\u{E000}B", SIZE_UM).unwrap();
    assert_eq!(got.runs.len(), 1);
    assert_eq!(got.runs[0].glyphs[1].glyph, 0, ".notdef");
    assert_eq!(
        got.warnings,
        vec![TextWarning::GlyphMissing { code_point: 0xE000, cluster: 1, font: id("sans-regular") }]
    );
    assert_eq!(got.warnings[0].to_string(), "report.glyph_missing: U+E000 at byte 1 (font sans-regular)");
}

#[test]
fn shaping_is_repeatable() {
    let set = fonts_with_script();
    let s = "Gr\u{f6}\u{df}e \u{65e5}\u{672c} \u{2300}12,5 \u{b1}0,05";
    assert_eq!(
        set.shape(&id("sans-regular"), s, SIZE_UM).unwrap(),
        set.shape(&id("sans-regular"), s, SIZE_UM).unwrap()
    );
}

#[test]
fn hand_labelled_advances() {
    // sans regular: 'H' is 680 units, '1' is 560 units, upem 1000.
    let set = FontSet::bundled().unwrap();
    let r = set.shape(&id("sans-regular"), "H1", 1000).unwrap();
    assert_eq!(r.runs[0].glyphs.iter().map(|g| g.advance_um).collect::<Vec<_>>(), vec![680, 560]);
    // 560 * 2500 / 1000 = 1400 exactly; 560 * 2501 / 1000 = 1400.56 -> 1401.
    assert_eq!(set.shape(&id("sans-regular"), "1", 2500).unwrap().runs[0].glyphs[0].advance_um, 1400);
    assert_eq!(set.shape(&id("sans-regular"), "1", 2501).unwrap().runs[0].glyphs[0].advance_um, 1401);
    // Half to even: 'i' is 220 units; 220 * 25 / 1000 = 5.5 -> 6 (even).
    assert_eq!(set.shape(&id("sans-regular"), "i", 25).unwrap().runs[0].glyphs[0].advance_um, 6);
    // 'l' is 220 units: 220 * 75 / 1000 = 16.5 -> 16 (even).
    assert_eq!(set.shape(&id("sans-regular"), "l", 75).unwrap().runs[0].glyphs[0].advance_um, 16);
    // mono: every glyph 600 units.
    let m = set.shape(&id("mono-regular"), "iW", 1000).unwrap();
    assert_eq!(m.runs[0].glyphs.iter().map(|g| g.advance_um).collect::<Vec<_>>(), vec![600, 600]);
}

#[test]
fn default_styles_select_bundled_fonts() {
    let set = FontSet::bundled().unwrap();
    assert_eq!(set.default_style(DefaultStyle::Body).unwrap().as_str(), "sans-regular");
    assert_eq!(set.default_style(DefaultStyle::Strong).unwrap().as_str(), "sans-bold");
    assert_eq!(set.default_style(DefaultStyle::Mono).unwrap().as_str(), "mono-regular");
    assert_eq!(set.default_style(DefaultStyle::MonoStrong).unwrap().as_str(), "mono-bold");
    assert!(set.select("Maven Report Sans", 400, Slant::Italic).is_none());
}

#[test]
fn shaping_limits_and_unknown_font() {
    let set = FontSet::bundled().unwrap();
    let long = "a".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(set.shape(&id("sans-regular"), &long, SIZE_UM).unwrap_err().code(), "report.text_limit");
    assert_eq!(set.shape(&id("sans-regular"), "a", 0).unwrap_err().code(), "report.text_limit");
    assert_eq!(set.shape(&id("sans-regular"), "a", 1_000_001).unwrap_err().code(), "report.text_limit");
    assert_eq!(set.shape(&id("serif"), "a", SIZE_UM).unwrap_err().code(), "report.font_unknown");
    assert!(set.shape(&id("sans-regular"), "", SIZE_UM).unwrap().runs.is_empty());
}

#[test]
fn shape_100k_characters() {
    // Performance budget (spec: 100 000 characters under 100 ms, [unverified until
    // measured]): run it so the time shows in the test log; no timing assertion.
    let set = FontSet::bundled().unwrap();
    let text: String = "Flatness 0.012 mm \u{b1}0,05 Gr\u{f6}\u{df}e ".chars().cycle().take(100_000).collect();
    let t = std::time::Instant::now();
    let r = set.shape(&id("sans-regular"), &text, SIZE_UM).unwrap();
    eprintln!("shaped 100000 characters in {:?}", t.elapsed());
    assert_eq!(r.runs.iter().map(|r| r.glyphs.len()).sum::<usize>(), 100_000);
    assert!(r.warnings.is_empty());
}

// ---------------------------------------------------------------------------------------
// AC-2: subsetting is byte-stable and holds exactly the used glyphs plus .notdef.

#[test]
fn ac2_subset_of_120_glyphs() {
    let set = FontSet::bundled().unwrap();
    let text: String = (0x21u32..0x7F).chain(0xC0..0xDA).map(|c| char::from_u32(c).unwrap()).collect();
    let shaped = set.shape(&id("sans-regular"), &text, SIZE_UM).unwrap();
    assert!(shaped.warnings.is_empty());
    let mut glyphs: Vec<u16> = shaped.runs[0].glyphs.iter().map(|g| g.glyph).collect();
    glyphs.sort_unstable();
    glyphs.dedup();
    assert_eq!(glyphs.len(), 120);

    let build = || {
        let mut b = SubsetBuilder::new(&set, &id("sans-regular")).unwrap();
        for r in &shaped.runs {
            b.add_run(r).unwrap();
        }
        b.finish().unwrap()
    };
    let a = build();
    let b = build();
    assert_eq!(a.bytes, b.bytes, "same glyph set, same bytes");
    assert_eq!(a.glyph_count(), 121);
    assert_eq!(a.glyph_count().to_string(), expected_subset("ac2-glyphs"));
    // The subset's maxp.numGlyphs says 121 too.
    let maxp = find_table(&a.bytes, b"maxp");
    assert_eq!(u16::from_be_bytes([maxp[4], maxp[5]]), 121);
    assert_eq!(a.originals()[0], 0, ".notdef first");
    assert_eq!(&a.originals()[1..], &glyphs[..]);
    assert_eq!(a.tag_str(), expected_subset("ac2-tag"));
    assert_eq!(sha_hex(&a.bytes), expected_subset("ac2-sha256"));
    assert_eq!(a.bytes.len().to_string(), expected_subset("ac2-bytes"));
    assert_eq!(sha_hex(a.to_unicode.bytes()), expected_subset("ac2-cmap-sha256"));
    assert_eq!(read_back(&shaped, &[&a]), text);

    // Adding glyphs in another order gives the same subset.
    let mut c = SubsetBuilder::new(&set, &id("sans-regular")).unwrap();
    for r in &shaped.runs {
        for g in r.glyphs.iter().rev() {
            c.add_glyph(g.glyph, r.char_of(g)).unwrap();
        }
    }
    assert_eq!(c.finish().unwrap(), a);
}

fn find_table<'a>(font: &'a [u8], tag: &[u8; 4]) -> &'a [u8] {
    let n = usize::from(u16::from_be_bytes([font[4], font[5]]));
    for i in 0..n {
        let r = 12 + 16 * i;
        if &font[r..r + 4] == tag {
            let off = u32::from_be_bytes(font[r + 8..r + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(font[r + 12..r + 16].try_into().unwrap()) as usize;
            return &font[off..off + len];
        }
    }
    panic!("no table");
}

#[test]
fn subset_of_composite_glyph_takes_components() {
    let m = FontManifest {
        fonts: vec![entry("composite-test", "Maven Composite Test", "MavenCompositeTest-Regular", COMPOSITE_SHA)],
        fallback: vec![],
    };
    let set = FontSet::load(&m, &[("composite-test", COMPOSITE_FONT)]).unwrap();
    let shaped = set.shape(&id("composite-test"), "C", SIZE_UM).unwrap();
    let mut b = SubsetBuilder::new(&set, &id("composite-test")).unwrap();
    b.add_run(&shaped.runs[0]).unwrap();
    let s = b.finish().unwrap();
    let want: Vec<u16> = expected_subset("composite-originals").split(' ').map(|x| x.parse().unwrap()).collect();
    assert_eq!(s.originals(), &want[..]);
    assert_eq!(s.tag_str(), expected_subset("composite-tag"));
    assert_eq!(sha_hex(&s.bytes), expected_subset("composite-sha256"));
    // Only 'C' is mapped back to text: components and .notdef carry no character.
    assert_eq!(s.to_unicode.entries(), &[(3, 'C')]);
}

#[test]
fn subset_rejects_foreign_runs_and_glyphs() {
    let set = FontSet::bundled().unwrap();
    let bold = set.shape(&id("sans-bold"), "x", SIZE_UM).unwrap();
    let mut b = SubsetBuilder::new(&set, &id("sans-regular")).unwrap();
    assert_eq!(b.add_run(&bold.runs[0]).unwrap_err().code(), "report.font_unknown");
    assert_eq!(b.add_glyph(60_000, None).unwrap_err().code(), "report.glyph_not_in_subset");
    let s = b.finish().unwrap();
    assert_eq!(s.glyph_count(), 1, ".notdef only");
    let reg = set.shape(&id("sans-regular"), "x", SIZE_UM).unwrap();
    assert_eq!(s.encode_run(&reg.runs[0]).unwrap_err().code(), "report.glyph_not_in_subset");
}

// ---------------------------------------------------------------------------------------
// AC-3: a font whose digest does not match the manifest is refused.

#[test]
fn ac3_digest_mismatch_is_font_invalid() {
    let mut m = bundled_manifest().unwrap();
    m.fonts[0].sha256 =
        Digest::parse("sha256:0000000000000000000000000000000000000000000000000000000000000000").unwrap();
    let files: Vec<(&str, &[u8])> = ["sans-regular", "sans-bold", "mono-regular", "mono-bold"]
        .iter()
        .map(|i| (*i, bundled_font_bytes(i).unwrap()))
        .collect();
    let err = FontSet::load(&m, &files).unwrap_err();
    assert_eq!(err.code(), "report.font_invalid");
    assert_eq!(err, TextError::FontInvalid { font: "sans-regular".into(), reason: "digest_mismatch" });

    // A single changed byte of a bundled font is caught the same way.
    let mut bytes = bundled_font_bytes("mono-bold").unwrap().to_vec();
    bytes[1000] ^= 1;
    let e = bundled_manifest().unwrap().fonts[3].clone();
    assert_eq!(
        LoadedFont::load(&e, &bytes).unwrap_err(),
        TextError::FontInvalid { font: "mono-bold".into(), reason: "digest_mismatch" }
    );

    // The script font with a wrong digest fails the whole set.
    let mut sm = script_manifest();
    sm.fonts[0].sha256 = Digest::parse(COMPOSITE_SHA).unwrap();
    assert_eq!(
        FontSet::bundled_with_script_fonts(&sm, &[("script-test", SCRIPT_FONT)]).unwrap_err().code(),
        "report.font_invalid"
    );
}

#[test]
fn manifest_errors() {
    let m = bundled_manifest().unwrap();
    let files: Vec<(&str, &[u8])> =
        ["sans-regular", "sans-bold", "mono-regular"].iter().map(|i| (*i, bundled_font_bytes(i).unwrap())).collect();
    assert_eq!(FontSet::load(&m, &files).unwrap_err().code(), "report.font_invalid", "missing file");
    let mut dup = m.clone();
    dup.fonts.push(dup.fonts[0].clone());
    assert!(matches!(dup.validate(), Err(TextError::ManifestInvalid { .. })));
    let mut bad_fb = m.clone();
    bad_fb.fallback.push(id("nope"));
    assert!(bad_fb.validate().is_err());
    let mut bad_ps = m.clone();
    bad_ps.fonts[0].postscript_name = "Bad Name".into();
    assert!(bad_ps.validate().is_err());
    assert!(FontId::new("Upper").is_err());
    assert!(FontId::new("").is_err());
}

#[test]
fn hostile_font_bytes_never_panic() {
    // Truncations and byte flips of a real font, each declared with its own digest so it
    // reaches the parser: every load either succeeds or fails with report.font_invalid,
    // and a font that loads can still shape and subset without panicking.
    let base = bundled_font_bytes("sans-regular").unwrap();
    let mut variants: Vec<Vec<u8>> = Vec::new();
    for cut in (0..base.len()).step_by(997).chain([1, 11, 12, 13, 100, 200, 400]) {
        variants.push(base[..cut].to_vec());
    }
    for (i, pos) in (0..400).step_by(3).enumerate() {
        let mut v = base.to_vec();
        v[pos] ^= 0xFF - (i as u8 % 7);
        variants.push(v);
    }
    for i in 0..64usize {
        let mut v = base.to_vec();
        let pos = (i * 7919) % v.len();
        v[pos] = v[pos].wrapping_add(0x80);
        variants.push(v);
    }
    let mut loaded = 0;
    for v in &variants {
        let sha = format!("sha256:{}", sha_hex(v));
        let e = entry("hostile", "Hostile", "Hostile", &sha);
        let m = FontManifest { fonts: vec![e], fallback: vec![id("hostile")] };
        match FontSet::load(&m, &[("hostile", v)]) {
            Ok(set) => {
                loaded += 1;
                let s = set.shape(&id("hostile"), "Flatness -0,050 \u{df}\u{65e5}", SIZE_UM).unwrap();
                let mut b = SubsetBuilder::new(&set, &id("hostile")).unwrap();
                for r in &s.runs {
                    b.add_run(r).unwrap();
                }
                if let Ok(sub) = b.finish() {
                    assert!(sub.bytes.len() <= v.len() + 4096);
                }
            }
            Err(e) => assert_eq!(e.code(), "report.font_invalid"),
        }
    }
    assert!(loaded > 0);
}

// ---------------------------------------------------------------------------------------
// AC-4: a German-locale value survives shaping, embedding and text extraction.

#[test]
fn ac4_value_round_trips_through_pdf() {
    let set = FontSet::bundled().unwrap();
    let shaped = set.shape(&id("sans-regular"), "-0,050", SIZE_UM).unwrap();
    assert_eq!(shaped.runs.len(), 1);
    let mut b = SubsetBuilder::new(&set, &id("sans-regular")).unwrap();
    b.add_run(&shaped.runs[0]).unwrap();
    let sub = b.finish().unwrap();
    assert_eq!(sub.to_unicode.bytes(), AC4_CMAP, "hand-labelled ToUnicode CMap");
    assert_eq!(read_back(&shaped, &[&sub]), "-0,050");
    let obj = text_object("F1", &sub, &shaped.runs[0], 25_000, 270_000).unwrap();
    assert_eq!(obj, AC4_TEXT_OBJECT, "hand-labelled text object");
    let pdf =
        single_page(A4.0, A4.1, &[&sub], &[PlacedRun { run: &shaped.runs[0], x_um: 25_000, y_um: 270_000 }]).unwrap();
    assert_eq!(pdf, AC4_PDF, "reference PDF (make_fixtures.py); check_pdf_text.py extracts -0,050 from it");
    let again =
        single_page(A4.0, A4.1, &[&sub], &[PlacedRun { run: &shaped.runs[0], x_um: 25_000, y_um: 270_000 }]).unwrap();
    assert_eq!(pdf, again);
}

#[test]
fn two_fonts_on_one_page_match_reference() {
    let set = fonts_with_script();
    let text = "Ma\u{df} \u{65e5}\u{672c} 12,5";
    let shaped = set.shape(&id("sans-bold"), text, SIZE_UM).unwrap();
    assert!(shaped.warnings.is_empty());
    assert_eq!(
        shaped.runs.iter().map(|r| r.font.as_str()).collect::<Vec<_>>(),
        vec!["sans-bold", "script-test", "sans-bold"]
    );
    let mut order: Vec<FontId> = Vec::new();
    for r in &shaped.runs {
        if !order.contains(&r.font) {
            order.push(r.font.clone());
        }
    }
    let subsets: Vec<_> = order
        .iter()
        .map(|f| {
            let mut b = SubsetBuilder::new(&set, f).unwrap();
            for r in shaped.runs.iter().filter(|r| &r.font == f) {
                b.add_run(r).unwrap();
            }
            b.finish().unwrap()
        })
        .collect();
    let refs: Vec<&_> = subsets.iter().collect();
    let mut x = 25_000;
    let mut placed = Vec::new();
    for r in &shaped.runs {
        placed.push(PlacedRun { run: r, x_um: x, y_um: 250_000 });
        x += r.advance_um();
    }
    let pdf = single_page(A4.0, A4.1, &refs, &placed).unwrap();
    assert_eq!(pdf, MIXED_PDF);
    assert_eq!(read_back(&shaped, &refs), text);
}

#[test]
fn formatter_output_is_shaped_verbatim() {
    // REQ-RPT.OUTPUT.PDF-004 / TEXT-006: the KRN.NUM.FMT.LEN string goes in unchanged and
    // comes back unchanged through the ToUnicode map.
    let spec = FormatSpec {
        formatter: FormatterId::LENGTH_V1,
        unit: Unit::Mm,
        precision: Precision::Decimals(3),
        limits: None,
    };
    let set = FontSet::bundled().unwrap();
    for x in [-0.00005, 0.012_345_6, 12.5e-3, -1.0e-9, 0.0254] {
        let rep = format(x, &spec).unwrap();
        let shaped = set.shape_reported(&id("sans-regular"), &rep, SIZE_UM).unwrap();
        assert!(shaped.warnings.is_empty());
        assert_eq!(shaped.text(), rep.value);
        let mut b = SubsetBuilder::new(&set, &id("sans-regular")).unwrap();
        b.add_run(&shaped.runs[0]).unwrap();
        let sub = b.finish().unwrap();
        assert_eq!(read_back(&shaped, &[&sub]), rep.value);
    }
    assert_eq!(format(-0.00005, &spec).unwrap().value, "-0.050");
}

// ---------------------------------------------------------------------------------------
// AC-5: every bundled font's licence is on both allowlists and in ATTRIBUTION.md.

fn toml_list<'a>(doc: &'a str, key: &str) -> Vec<&'a str> {
    let start = doc
        .lines()
        .position(|l| l.trim_start().starts_with(&format!("{key} = [")))
        .unwrap_or_else(|| panic!("no {key}"));
    let mut out = Vec::new();
    for l in doc.lines().skip(start + 1) {
        let t = l.trim();
        if t.starts_with(']') {
            break;
        }
        if let Some(v) = t.strip_prefix('"').and_then(|r| r.split('"').next()) {
            out.push(v);
        }
    }
    out
}

#[test]
fn ac5_licences_are_allowed_and_attributed() {
    let m = bundled_manifest().unwrap();
    let deny = toml_list(DENY_TOML, "allow");
    let assets = toml_list(SPDX_ALLOWLIST, "asset_licences");
    assert!(deny.contains(&"MIT") && assets.contains(&"MIT"));
    check_licences(&m, &deny).unwrap();
    check_licences(&m, &assets).unwrap();
    for f in &m.fonts {
        let file = bundled_font_file(f.id.as_str()).unwrap();
        let line =
            ATTRIBUTION.lines().find(|l| l.contains(file)).unwrap_or_else(|| panic!("{file} not in ATTRIBUTION.md"));
        assert!(line.contains(&f.spdx), "{file}: licence {} not on its ATTRIBUTION.md line", f.spdx);
    }
    // A licence off the list is caught, as is an exception.
    let mut bad = m.clone();
    bad.fonts[0].spdx = "OFL-1.1".into();
    assert_eq!(check_licences(&bad, &deny).unwrap_err(), vec![(id("sans-regular"), "OFL-1.1".to_string())]);
    assert_eq!(spdx_ids("(MIT OR Apache-2.0) AND Zlib"), vec!["MIT", "Apache-2.0", "Zlib"]);
    assert_eq!(spdx_ids("Apache-2.0 WITH LLVM-exception"), vec!["Apache-2.0", "WITH", "LLVM-exception"]);
}

// ---------------------------------------------------------------------------------------
// PDF primitives and line breaks (hand-labelled).

#[test]
fn pdf_numbers_and_escaping() {
    assert_eq!(milli_points(25_400), 72_000);
    assert_eq!(milli_points(127), 360);
    assert_eq!(milli_points(-25_000), -70_866);
    assert_eq!(fmt_milli(72_000), "72.000");
    assert_eq!(fmt_milli(-500), "-0.500");
    assert_eq!(fmt_milli(5), "0.005");
    assert_eq!(escape_name("F1").unwrap(), "/F1");
    assert_eq!(escape_name("A B/(x)#").unwrap(), "/A#20B#2F#28x#29#23");
    assert_eq!(escape_name("\u{e9}").unwrap(), "/#C3#A9");
    assert_eq!(escape_name("\u{1}").unwrap(), "/#01");
    assert_eq!(escape_name("F\u{0}1").unwrap_err(), TextError::PdfNameInvalid);
    assert_eq!(escape_name("\u{0}").unwrap_err().code(), "report.pdf_name_invalid");
    assert_eq!(escape_literal("a(b)\\c"), "(a\\(b\\)\\\\c)");
    assert_eq!(escape_literal("x\ny\u{e9}"), "(x\\012y\\303\\251)");
    assert_eq!(escape_literal(") Tj ET q"), "(\\) Tj ET q)");
}

#[test]
fn text_object_and_page_limits() {
    let set = FontSet::bundled().unwrap();
    let shaped = set.shape(&id("sans-regular"), "1", SIZE_UM).unwrap();
    let mut b = SubsetBuilder::new(&set, &id("sans-regular")).unwrap();
    b.add_run(&shaped.runs[0]).unwrap();
    let sub = b.finish().unwrap();
    assert_eq!(text_object("F1", &sub, &shaped.runs[0], 10_000_001, 0).unwrap_err().code(), "report.text_limit");
    assert_eq!(single_page(0, 1, &[&sub], &[]).unwrap_err().code(), "report.text_limit");
    assert_eq!(single_page(1, 1, &[], &[]).unwrap_err().code(), "report.text_limit");
    let bold = set.shape(&id("sans-bold"), "1", SIZE_UM).unwrap();
    let placed = [PlacedRun { run: &bold.runs[0], x_um: 0, y_um: 0 }];
    assert_eq!(single_page(1, 1, &[&sub], &placed).unwrap_err().code(), "report.font_unknown");
}

#[test]
fn line_break_opportunities() {
    assert_eq!(break_opportunities("Flatness 0.012 mm").unwrap(), vec![9, 15]);
    assert_eq!(break_opportunities("-0,050").unwrap(), Vec::<usize>::new());
    assert_eq!(break_opportunities("a  b").unwrap(), vec![3]);
    assert_eq!(break_opportunities("Mess-Ergebnis").unwrap(), vec![5]);
    assert_eq!(break_opportunities("10-20").unwrap(), Vec::<usize>::new());
    assert_eq!(break_opportunities("12\u{202f}345 x").unwrap(), vec![9]);
    assert_eq!(break_opportunities("9\u{2009}876").unwrap(), Vec::<usize>::new());
    // 日本語 (3 bytes each): breaks before 本 and 語.
    assert_eq!(break_opportunities("\u{65e5}\u{672c}\u{8a9e}").unwrap(), vec![3, 6]);
    assert_eq!(break_opportunities(" a").unwrap(), vec![1]);
    assert_eq!(break_opportunities("a ").unwrap(), Vec::<usize>::new());
    let long = " ".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(break_opportunities(&long).unwrap_err().code(), "report.text_limit");
}

#[test]
fn content_stream_is_bounded() {
    // 1 MiB of text is the largest run; placed again and again it must hit MAX_CONTENT_BYTES
    // (64 MiB) with report.text_limit instead of growing without bound.
    let set = FontSet::bundled().unwrap();
    let text = "a".repeat(MAX_TEXT_BYTES);
    let shaped = set.shape(&id("sans-regular"), &text, SIZE_UM).unwrap();
    let mut b = SubsetBuilder::new(&set, &id("sans-regular")).unwrap();
    b.add_run(&shaped.runs[0]).unwrap();
    let sub = b.finish().unwrap();
    let one = PlacedRun { run: &shaped.runs[0], x_um: 0, y_um: 0 };
    // 15 objects of 4 MiB + overhead fit; the 16th would pass 64 MiB.
    assert!(single_page(A4.0, A4.1, &[&sub], &[one; 15]).is_ok());
    let err = single_page(A4.0, A4.1, &[&sub], &[one; 16]).unwrap_err();
    assert_eq!(err, TextError::Limit { what: "content stream longer than MAX_CONTENT_BYTES" });
    assert_eq!(MAX_CONTENT_BYTES, 64 * 1024 * 1024);
}

#[test]
fn shared_glyph_and_upem_2048_hand_labelled() {
    // shared-2048-test.ttf (make_fonts.py): units per em 2048; glyph 0 .notdef advance 1000,
    // glyph 1 a bar with advance 1229 that both 'A' (U+0041) and Greek Alpha (U+0391) map to;
    // head bbox (20, -40, 480, 1400); hhea ascender 950, descender -250.
    let m = FontManifest {
        fonts: vec![entry("shared-test", "Maven Shared Test", "MavenSharedTest-Regular", SHARED_2048_SHA)],
        fallback: vec![],
    };
    let set = FontSet::load(&m, &[("shared-test", SHARED_2048_FONT)]).unwrap();
    let shaped = set.shape(&id("shared-test"), "A\u{391}A", 2048).unwrap();
    // Advance 1229 units at an em of 2048 um is 1229 um.
    let got: Vec<(u16, i64)> = shaped.runs[0].glyphs.iter().map(|g| (g.glyph, g.advance_um)).collect();
    assert_eq!(got, vec![(1, 1229); 3]);
    let mut b = SubsetBuilder::new(&set, &id("shared-test")).unwrap();
    b.add_run(&shaped.runs[0]).unwrap();
    let s = b.finish().unwrap();
    // PDF glyph space is 1/1000 em, rounded half to even:
    // 1000*1000/2048 = 488.28 -> 488; 1229*1000/2048 = 600.10 -> 600.
    assert_eq!(s.widths(), &[488, 600]);
    // 20000/2048 = 9.77 -> 10; -40000/2048 = -19.53 -> -20; 480000/2048 = 234.375 -> 234;
    // 1400000/2048 = 683.59 -> 684.
    assert_eq!(s.bbox(), [10, -20, 234, 684]);
    // 950000/2048 = 463.87 -> 464; -250000/2048 = -122.07 -> -122.
    assert_eq!((s.ascent(), s.descent()), (464, -122));
    // One ToUnicode entry per glyph: the lower code point is kept and the other reported.
    assert_eq!(s.to_unicode.entries(), &[(1, 'A')]);
    let want = vec![TextWarning::GlyphShared { font: id("shared-test"), glyph: 1, kept: 0x41, dropped: 0x391 }];
    assert_eq!(s.warnings, want);
    assert_eq!(s.warnings[0].code(), "report.glyph_shared");
    assert_eq!(read_back(&shaped, &[&s]), "AAA");
    // The PDF says the same.
    let placed = [PlacedRun { run: &shaped.runs[0], x_um: 0, y_um: 0 }];
    let pdf = single_page(A4.0, A4.1, &[&s], &placed).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/W [0 [488 600]]"));
    assert!(text.contains("/FontBBox [10 -20 234 684] /ItalicAngle 0 /Ascent 464 /Descent -122 /CapHeight 464"));
}
