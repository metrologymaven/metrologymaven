//! Acceptance and robustness tests for `IO.REG.PLUGIN.DRAW` (`fid-io-draw`, AC-1..AC-8).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Instant;

use fid_canon::CValue;
use fid_io_draw::{
    fuzz_read_all, read_drawing_bytes, BoxUsed, DrawError, DrawPluginReader, PixelFormat, FORMAT_PDF_DRAWING,
    FORMAT_RASTER_DRAWING, WARN_EMBEDDED_FILES_IGNORED, WARN_FORMS_IGNORED, WARN_JS_IGNORED,
};
use fid_io_plugin::abi::adapter::PluginReaderAdapter;
use fid_io_plugin::abi::{FormatPluginReader, ReadRequest, Record, HOST_CONTRACT_VERSION};
use fid_io_reg::{Direction, FormatId, Origin, Registry};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/reg/plugin/draw")
}

fn read_fixture(name: &str) -> Vec<u8> {
    let p = fixtures_dir().join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", p.display()))
}

// ------------------------------------------------------------------ AC-1 .. AC-7

#[test]
fn ac1_a3_landscape_pdf() {
    let bytes = read_fixture("a3_landscape.pdf");
    let res = read_drawing_bytes(Some(FORMAT_PDF_DRAWING), &bytes).expect("a3_landscape.pdf should decode");
    assert_eq!(res.format_id, FORMAT_PDF_DRAWING);
    assert!(res.warnings.is_empty(), "unexpected warnings: {:?}", res.warnings);
    assert_eq!(res.pages.len(), 1);
    let p = &res.pages[0];
    assert_eq!(p.index, 0);
    assert_eq!(p.width_cmm, 42_000);
    assert_eq!(p.height_cmm, 29_700);
    assert_eq!(p.rotation, 0);
    assert_eq!(p.box_used, BoxUsed::Media);
    assert!(p.raster.is_none());
}

#[test]
fn ac2_rotated_pdf_swaps_axes() {
    let bytes = read_fixture("rotated.pdf");
    let res = read_drawing_bytes(None, &bytes).expect("rotated.pdf should decode");
    assert_eq!(res.pages.len(), 1);
    let p = &res.pages[0];
    assert_eq!(p.width_cmm, 29_700);
    assert_eq!(p.height_cmm, 21_000);
    assert_eq!(p.rotation, 90);
    assert_eq!(p.box_used, BoxUsed::Media);
}

#[test]
fn ac3_three_pages_cropbox_on_page_2() {
    let bytes = read_fixture("three_pages.pdf");
    let res = read_drawing_bytes(Some(FORMAT_PDF_DRAWING), &bytes).expect("three_pages.pdf should decode");
    assert_eq!(res.pages.len(), 3);

    assert_eq!(res.pages[0].index, 0);
    assert_eq!(res.pages[0].width_cmm, 21_000);
    assert_eq!(res.pages[0].height_cmm, 29_700);
    assert_eq!(res.pages[0].box_used, BoxUsed::Media);

    assert_eq!(res.pages[1].index, 1);
    assert_eq!(res.pages[1].width_cmm, 15_000);
    assert_eq!(res.pages[1].height_cmm, 10_000);
    assert_eq!(res.pages[1].box_used, BoxUsed::Crop);

    assert_eq!(res.pages[2].index, 2);
    assert_eq!(res.pages[2].width_cmm, 21_590);
    assert_eq!(res.pages[2].height_cmm, 27_940);
    assert_eq!(res.pages[2].box_used, BoxUsed::Media);
}

#[test]
fn ac4_png_stated_300dpi_and_nodpi() {
    let scan = read_fixture("scan_300dpi.png");
    let res = read_drawing_bytes(Some(FORMAT_RASTER_DRAWING), &scan).expect("scan_300dpi.png should decode");
    assert_eq!(res.pages.len(), 1);
    let r = res.pages[0].raster.as_ref().expect("raster payload");
    assert_eq!(r.width, 3508);
    assert_eq!(r.height, 2480);
    assert_eq!(r.format, PixelFormat::Gray8);
    assert_eq!(r.dpi, Some((300, 300)));
    assert_eq!(r.pixels.len(), 3508 * 2480);
    assert_eq!(res.pages[0].width_cmm, 29_701);
    assert_eq!(res.pages[0].height_cmm, 20_997);

    let nodpi = read_fixture("nodpi.png");
    let res2 = read_drawing_bytes(None, &nodpi).expect("nodpi.png should decode");
    assert_eq!(res2.pages.len(), 1);
    let r2 = res2.pages[0].raster.as_ref().expect("raster payload");
    assert_eq!(r2.width, 64);
    assert_eq!(r2.height, 48);
    assert_eq!(r2.format, PixelFormat::Rgba8);
    assert_eq!(r2.dpi, None);
    assert_eq!(r2.pixels.len(), 64 * 48 * 4);
    assert_eq!(res2.pages[0].width_cmm, 0);
    assert_eq!(res2.pages[0].height_cmm, 0);
}

#[test]
fn ac5_tiff_g4_two_pages_checksums_match() {
    let tif = read_fixture("g4_two_pages.tif");
    let sha_text = String::from_utf8(read_fixture("g4_two_pages.sha256")).expect("utf8 sha256 file");
    let expected: Vec<&str> = sha_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().nth(1).expect("digest column"))
        .collect();
    assert_eq!(expected.len(), 2);

    let res = read_drawing_bytes(Some(FORMAT_RASTER_DRAWING), &tif).expect("g4_two_pages.tif should decode");
    assert_eq!(res.pages.len(), 2);
    for (idx, exp) in expected.iter().enumerate() {
        let p = &res.pages[idx];
        assert_eq!(p.index, idx as u32);
        let r = p.raster.as_ref().expect("raster");
        assert_eq!(r.width, 96);
        assert_eq!(r.height, 64);
        assert_eq!(r.format, PixelFormat::Gray8);
        assert_eq!(r.dpi, Some((300, 300)));
        assert_eq!(r.pixels_sha256().as_str(), *exp, "page {idx} digest mismatch");
    }
}

#[test]
fn ac6_encrypted_pdf_rejected_and_js_action_warns_without_executing() {
    let enc = read_fixture("encrypted.pdf");
    let err = read_drawing_bytes(Some(FORMAT_PDF_DRAWING), &enc).expect_err("encrypted.pdf must fail");
    assert_eq!(err.code(), "pdf_encrypted");
    assert_eq!(err, DrawError::PdfEncrypted);

    let js = read_fixture("js_action.pdf");
    let res = read_drawing_bytes(Some(FORMAT_PDF_DRAWING), &js).expect("js_action.pdf should succeed with warning");
    assert_eq!(res.pages.len(), 1);
    assert!(
        res.warnings.iter().any(|w| w.code == WARN_JS_IGNORED),
        "expected {WARN_JS_IGNORED}, got {:?}",
        res.warnings
    );
}

#[test]
fn ac7_pages201_exceeds_page_limit() {
    let big = read_fixture("pages201.pdf");
    let err = read_drawing_bytes(Some(FORMAT_PDF_DRAWING), &big).expect_err("201 pages must fail");
    assert_eq!(err.code(), "limit_exceeded");
    match err {
        DrawError::LimitExceeded { limit, .. } => assert_eq!(limit, "pages"),
        other => panic!("expected LimitExceeded(pages), got {other:?}"),
    }
}

// ------------------------------------------------------------------ Plugin ABI contract & detection

#[test]
fn plugin_contract_registration_detection_and_reading() {
    let reader = DrawPluginReader::new().expect("descriptor must validate");
    let mut reg = Registry::new();
    let registered = reader.register_formats(&mut reg).expect("register_formats should succeed");
    assert_eq!(registered.len(), 2);

    // Origin must be FirstParty for both pdf-drawing and raster-drawing.
    let pdf_id = FormatId::new(FORMAT_PDF_DRAWING).expect("format id");
    let ras_id = FormatId::new(FORMAT_RASTER_DRAWING).expect("format id");
    assert_eq!(reg.get(&pdf_id, Direction::Import).expect("pdf-drawing").origin(), Origin::FirstParty);
    assert_eq!(reg.get(&ras_id, Direction::Import).expect("raster-drawing").origin(), Origin::FirstParty);

    // Content detection via IO.REG.DETECT signatures.
    let a3 = read_fixture("a3_landscape.pdf");
    assert_eq!(reg.detect(&a3, a3.len() as u64, Some("sheet.pdf"), None).expect("detect pdf"), pdf_id);
    let png = read_fixture("nodpi.png");
    assert_eq!(reg.detect(&png, png.len() as u64, Some("scan.png"), None).expect("detect png"), ras_id);
    let tif = read_fixture("g4_two_pages.tif");
    assert_eq!(reg.detect(&tif, tif.len() as u64, Some("scan.tif"), None).expect("detect tif"), ras_id);

    // Read through FormatPluginReader directly for both PDF (with warning) and nodpi PNG.
    let empty_opts = CValue::Obj(BTreeMap::new());
    let js = read_fixture("js_action.pdf");
    let mut cur = Cursor::new(&js);
    let mut req = ReadRequest { format_id: &pdf_id, options: &empty_opts, stream: &mut cur };
    let recs = reader.read(&mut req).expect("plugin read js_action.pdf");
    assert!(recs.iter().any(|r| matches!(r, Record::DrawingPage(_))));
    assert!(recs.iter().any(|r| matches!(r, Record::Warning(w) if w.code.as_display().to_string() == WARN_JS_IGNORED)));

    let mut cur2 = Cursor::new(&png);
    let mut req2 = ReadRequest { format_id: &ras_id, options: &empty_opts, stream: &mut cur2 };
    let recs2 = reader.read(&mut req2).expect("plugin read nodpi.png");
    assert_eq!(recs2.len(), 1);
    match &recs2[0] {
        Record::DrawingPage(p) => {
            assert_eq!(p.dpi, 0);
            assert!(p.width_mm > 0.0 && p.height_mm > 0.0);
            assert!(p.metadata.iter().any(
                |kv| kv.key.as_display().to_string() == "resolution" && kv.value.as_display().to_string() == "none"
            ));
        }
        other => panic!("expected DrawingPage record, got {other:?}"),
    }

    // Adapter construction succeeds against HOST_CONTRACT_VERSION with Origin::FirstParty.
    let adapter = PluginReaderAdapter::new(reader, ras_id, Origin::FirstParty, HOST_CONTRACT_VERSION)
        .expect("PluginReaderAdapter should construct");
    assert_eq!(adapter.origin(), Origin::FirstParty);
}

// ------------------------------------------------------------------ PDF 1.5 xref/objstm, UserUnit, forms/files, cycle, perf

fn build_pdf(objects: &[(u32, String)], trailer_extra: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = BTreeMap::new();
    for (num, body) in objects {
        offsets.insert(*num, out.len());
        out.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let max_num = offsets.keys().copied().max().unwrap_or(0);
    let xref_pos = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max_num + 1).as_bytes());
    for i in 1..=max_num {
        if let Some(off) = offsets.get(&i) {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        } else {
            out.extend_from_slice(b"0000000000 65535 f \n");
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R {trailer_extra}>>\nstartxref\n{xref_pos}\n%%EOF\n", max_num + 1)
            .as_bytes(),
    );
    out
}

#[test]
fn pdf_user_unit_inherited_mediabox_and_security_warnings() {
    // 100 x 50 pt with UserUnit 2.0 => 200 x 100 pt => 7056 x 3528 cmm.
    // Includes EmbeddedFiles and AcroForm in Catalog to verify warning reporting.
    let pdf = build_pdf(
        &[
            (
                1,
                "<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles << /Names [] >> >> /AcroForm << /Fields [] >> >>"
                    .into(),
            ),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 50] >>".into()),
            (3, "<< /Type /Page /Parent 2 0 R /UserUnit 2.0 >>".into()),
        ],
        "",
    );
    let res = read_drawing_bytes(Some(FORMAT_PDF_DRAWING), &pdf).expect("userunit pdf");
    assert_eq!(res.pages.len(), 1);
    assert_eq!(res.pages[0].width_cmm, 7_056);
    assert_eq!(res.pages[0].height_cmm, 3_528);
    assert!(res.warnings.iter().any(|w| w.code == WARN_EMBEDDED_FILES_IGNORED));
    assert!(res.warnings.iter().any(|w| w.code == WARN_FORMS_IGNORED));
}

#[test]
fn pdf_cyclic_page_tree_returns_malformed_without_looping() {
    let pdf = build_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
            (3, "<< /Type /Pages /Kids [2 0 R] /Count 1 >>".into()),
        ],
        "",
    );
    let err = read_drawing_bytes(Some(FORMAT_PDF_DRAWING), &pdf).expect_err("cyclic tree must fail");
    assert_eq!(err.code(), "malformed");
}

#[test]
fn pdf_xref_stream_and_objstm_1_5() {
    // Construct a minimal PDF 1.5 with ObjStm (obj 4 holding objs 1, 2, 3) + uncompressed XRef stream (obj 5).
    let mut out = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let obj1 = b"<< /Type /Catalog /Pages 2 0 R >>";
    let obj2 = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
    let obj3 = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 72] >>";
    let off1 = 0usize;
    let off2 = obj1.len() + 1;
    let off3 = off2 + obj2.len() + 1;
    let header = format!("1 {off1} 2 {off2} 3 {off3}\n");
    let first = header.len();
    let mut objstm_body = header.into_bytes();
    objstm_body.extend_from_slice(obj1);
    objstm_body.push(b'\n');
    objstm_body.extend_from_slice(obj2);
    objstm_body.push(b'\n');
    objstm_body.extend_from_slice(obj3);

    let off_obj4 = out.len();
    out.extend_from_slice(
        format!("4 0 obj\n<< /Type /ObjStm /N 3 /First {first} /Length {} >>\nstream\n", objstm_body.len()).as_bytes(),
    );
    out.extend_from_slice(&objstm_body);
    out.extend_from_slice(b"\nendstream\nendobj\n");

    let off_obj5 = out.len();
    // XRef entries for objects 0..=5 with /W [1 4 1]:
    // 0: type 0 (free)
    // 1: type 2 (in objstm 4, idx 0)
    // 2: type 2 (in objstm 4, idx 1)
    // 3: type 2 (in objstm 4, idx 2)
    // 4: type 1 (offset off_obj4)
    // 5: type 1 (offset off_obj5)
    let mut xref_data = Vec::new();
    let push_entry = |v: &mut Vec<u8>, ty: u8, f2: u32, f3: u8| {
        v.push(ty);
        v.extend_from_slice(&f2.to_be_bytes());
        v.push(f3);
    };
    push_entry(&mut xref_data, 0, 0, 255);
    push_entry(&mut xref_data, 2, 4, 0);
    push_entry(&mut xref_data, 2, 4, 1);
    push_entry(&mut xref_data, 2, 4, 2);
    push_entry(&mut xref_data, 1, off_obj4 as u32, 0);
    push_entry(&mut xref_data, 1, off_obj5 as u32, 0);

    out.extend_from_slice(
        format!("5 0 obj\n<< /Type /XRef /Size 6 /W [1 4 1] /Root 1 0 R /Length {} >>\nstream\n", xref_data.len())
            .as_bytes(),
    );
    out.extend_from_slice(&xref_data);
    out.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{off_obj5}\n%%EOF\n").as_bytes());

    let res = read_drawing_bytes(Some(FORMAT_PDF_DRAWING), &out).expect("PDF 1.5 XRef+ObjStm should decode");
    assert_eq!(res.pages.len(), 1);
    // 72 pt == 1 inch == 25.4 mm == 2540 cmm.
    assert_eq!(res.pages[0].width_cmm, 2_540);
    assert_eq!(res.pages[0].height_cmm, 2_540);
}

#[test]
fn pdf_50_pages_meets_performance_budget() {
    let mut objs = Vec::with_capacity(52);
    objs.push((1, "<< /Type /Catalog /Pages 2 0 R >>".into()));
    let kids: Vec<String> = (3..53).map(|i| format!("{i} 0 R")).collect();
    objs.push((2, format!("<< /Type /Pages /Kids [{}] /Count 50 >>", kids.join(" "))));
    for i in 3..53 {
        objs.push((i, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.2756 841.8898] >>".into()));
    }
    let pdf = build_pdf(&objs, "");
    let t0 = Instant::now();
    let res = read_drawing_bytes(Some(FORMAT_PDF_DRAWING), &pdf).expect("50-page pdf");
    let dt = t0.elapsed();
    assert_eq!(res.pages.len(), 50);
    assert!(dt.as_millis() < 1000, "50-page PDF took {} ms (budget < 1000 ms)", dt.as_millis());
}

// ------------------------------------------------------------------ Raster limits & TIFF compression modes

#[test]
fn png_exceeding_400_megapixels_rejected_before_alloc() {
    // Craft a PNG header claiming 25000 x 25000 = 625 Mpx (> 400 Mpx).
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&25_000u32.to_be_bytes());
    ihdr.extend_from_slice(&25_000u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    png.extend_from_slice(&(ihdr.len() as u32).to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&ihdr);
    png.extend_from_slice(&0u32.to_be_bytes()); // CRC ignored before IHDR limit check or checked
                                                // Recompute real CRC32 so we definitely hit the pixel-count limit rather than CRC check.
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in b"IHDR".iter().chain(ihdr.iter()) {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    let crc = !crc;
    let n = png.len();
    png[n - 4..n].copy_from_slice(&crc.to_be_bytes());

    let err = read_drawing_bytes(Some(FORMAT_RASTER_DRAWING), &png).expect_err("625 Mpx must fail");
    assert_eq!(err.code(), "limit_exceeded");
    match err {
        DrawError::LimitExceeded { limit, .. } => assert_eq!(limit, "pixels"),
        other => panic!("expected LimitExceeded(pixels), got {other:?}"),
    }
}

fn make_be_tiff(width: u32, height: u32, compression: u16, strip: &[u8]) -> Vec<u8> {
    // Big-endian ("MM") 8-bit grayscale TIFF with explicit 150 DPI resolution.
    let mut out = Vec::new();
    out.extend_from_slice(b"MM\x00\x2A");
    out.extend_from_slice(&8u32.to_be_bytes()); // IFD at offset 8

    let entry_count: u16 = 11;
    let ifd_size = 2 + usize::from(entry_count) * 12 + 4;
    let xres_off = 8 + ifd_size;
    let yres_off = xres_off + 8;
    let strip_off = yres_off + 8;

    out.extend_from_slice(&entry_count.to_be_bytes());
    let mut entry = |tag: u16, ty: u16, count: u32, val: u32| {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&ty.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        if ty == 3 && count == 1 {
            out.extend_from_slice(&(val as u16).to_be_bytes());
            out.extend_from_slice(&0u16.to_be_bytes());
        } else {
            out.extend_from_slice(&val.to_be_bytes());
        }
    };
    entry(256, 4, 1, width);
    entry(257, 4, 1, height);
    entry(258, 3, 1, 8);
    entry(259, 3, 1, u32::from(compression));
    entry(262, 3, 1, 1); // BlackIsZero
    entry(273, 4, 1, strip_off as u32);
    entry(277, 3, 1, 1);
    entry(278, 4, 1, height);
    entry(279, 4, 1, strip.len() as u32);
    entry(282, 5, 1, xres_off as u32);
    entry(283, 5, 1, yres_off as u32);
    out.extend_from_slice(&0u32.to_be_bytes()); // next IFD = 0

    // RATIONAL 150/1 for XRes and YRes
    out.extend_from_slice(&150u32.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&150u32.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(strip);
    out
}

#[test]
fn tiff_big_endian_uncompressed_and_packbits_and_lzw() {
    let raw: Vec<u8> = (0..16u8).map(|i| i * 16).collect();
    // Uncompressed (compression = 1)
    let tif_raw = make_be_tiff(4, 4, 1, &raw);
    let r1 = read_drawing_bytes(Some(FORMAT_RASTER_DRAWING), &tif_raw).expect("uncompressed BE tiff");
    assert_eq!(r1.pages[0].raster.as_ref().expect("r").pixels, raw);
    assert_eq!(r1.pages[0].raster.as_ref().expect("r").dpi, Some((150, 150)));

    // PackBits (compression = 32773): 1 literal run of 16 bytes => [15, raw...]
    let mut pb = vec![15u8];
    pb.extend_from_slice(&raw);
    let tif_pb = make_be_tiff(4, 4, 32773, &pb);
    let r2 = read_drawing_bytes(Some(FORMAT_RASTER_DRAWING), &tif_pb).expect("packbits BE tiff");
    assert_eq!(r2.pages[0].raster.as_ref().expect("r").pixels, raw);

    // TIFF LZW (compression = 5, MSB-first 9-bit codes): CLEAR(256), literal bytes..., EOI(257).
    let mut bits = Vec::<u8>::new();
    let mut acc: u32 = 0;
    let mut nbits: u32 = 0;
    let mut emit9 = |code: u16| {
        acc = (acc << 9) | u32::from(code);
        nbits += 9;
        while nbits >= 8 {
            nbits -= 8;
            bits.push(((acc >> nbits) & 0xFF) as u8);
        }
    };
    emit9(256);
    for &b in &raw {
        emit9(u16::from(b));
    }
    emit9(257);
    if nbits > 0 {
        bits.push(((acc << (8 - nbits)) & 0xFF) as u8);
    }
    let tif_lzw = make_be_tiff(4, 4, 5, &bits);
    let r3 = read_drawing_bytes(Some(FORMAT_RASTER_DRAWING), &tif_lzw).expect("lzw BE tiff");
    assert_eq!(r3.pages[0].raster.as_ref().expect("r").pixels, raw);
}

// ------------------------------------------------------------------ AC-8 fuzz stress over seeds + IO.CORP

#[test]
fn ac8_fuzz_seeds_and_hostile_mutations_no_panic_or_hang() {
    let mut seeds: Vec<Vec<u8>> = Vec::new();

    // All drawing fixtures
    for name in [
        "a3_landscape.pdf",
        "rotated.pdf",
        "three_pages.pdf",
        "nodpi.png",
        "g4_two_pages.tif",
        "encrypted.pdf",
        "js_action.pdf",
    ] {
        seeds.push(read_fixture(name));
    }

    // Hostile corpus seeds from IO.CORP if present.
    let corp_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/corp/hostile");
    if let Ok(entries) = std::fs::read_dir(&corp_dir) {
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        paths.sort();
        for p in paths {
            if let Ok(b) = std::fs::read(&p) {
                seeds.push(b);
            }
        }
    }

    // Add edge-case empty/truncated/magic-only inputs.
    seeds.push(Vec::new());
    seeds.push(b"%PDF-1.7\n".to_vec());
    seeds.push(b"\x89PNG\r\n\x1a\n".to_vec());
    seeds.push(b"II\x2A\x00\x08\x00\x00\x00".to_vec());
    seeds.push(b"MM\x00\x2A\x00\x00\x00\x08".to_vec());

    // Deterministic xorshift64 mutations over all seeds.
    let mut state: u64 = 0x5EED_F1D0_D2A7_0008;
    let mut next_u64 = || -> u64 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    let t0 = Instant::now();
    for seed in &seeds {
        fuzz_read_all(seed);
        // Truncations
        for cut in [0usize, 1, 4, 8, 16, 32, 64, seed.len() / 2] {
            if cut < seed.len() {
                fuzz_read_all(&seed[..cut]);
            }
        }
        // 40 deterministic byte/slice mutations per seed
        if !seed.is_empty() {
            for _ in 0..40 {
                let mut mutated = seed.clone();
                let flips = ((next_u64() % 4) + 1) as usize;
                for _ in 0..flips {
                    let idx = (next_u64() as usize) % mutated.len();
                    mutated[idx] ^= (next_u64() as u8) | 1;
                }
                fuzz_read_all(&mutated);
            }
        }
    }
    let elapsed = t0.elapsed();
    assert!(elapsed.as_secs() < 15, "AC-8 fuzz budget exceeded: {elapsed:?}");
}
