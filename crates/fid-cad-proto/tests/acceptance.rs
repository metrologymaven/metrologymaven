//! Acceptance criteria of spec node IO.CAD.WORKER.PROTO (AC-1 .. AC-5) and the
//! requirements behind them. AC-1 digests come from the independent reference script
//! `fixtures/io/cad/worker/proto/make_digests.py`; hostile frames are hand-written here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../../../fixtures/io/cad/worker/proto/samples.rs"]
mod samples;

use fid_cad_proto::{
    kind, negotiate, Cap, CrashReason, CurveKind, EncodeError, FailCode, FrameReader, HostSession, IgesOptions, PmiKind,
    ProtoError, ProtoLimits, ProtoVersion, RawFrame, Record, RecordBody, RecordKind, Request, Response, SourceRef,
    StepOptions, SurfaceKind, WireText, DEFAULT_FRAME_MAX, DEFAULT_RECORDS_MAX, DEFAULT_SESSION_MAX, HEADER_LEN,
};
use fid_io_reg::{Budget, IoError, LimitKind, LimitRequest, Position, Principal};
use samples::{Rng, Sample};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;

const V1: ProtoVersion = ProtoVersion { major: 1, minor: 0 };

fn budget() -> Budget {
    Budget::new(Principal::Script, &LimitRequest::default()).unwrap()
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn encode(s: &Sample) -> Vec<u8> {
    match s {
        Sample::Req(r) => r.encode().unwrap(),
        Sample::Resp(r) => r.encode().unwrap(),
    }
}

fn digests() -> Vec<(String, String)> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/cad/worker/proto/digests.txt");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split(' ');
            let id = it.next().unwrap().to_string();
            let digest = it.last().unwrap().to_string();
            (id, digest)
        })
        .collect()
}

fn rec(body: RecordBody) -> Record {
    Record { source: SourceRef::Step(1), name: None, body }
}

fn hello_ack(version: ProtoVersion) -> Vec<u8> {
    Response::HelloAck { version, reader: WireText::new("fid-step"), reader_version: WireText::new("0.0.1") }
        .encode()
        .unwrap()
}

fn records(seq: u32, records: Vec<Record>) -> Vec<u8> {
    Response::Records { seq, records }.encode().unwrap()
}

fn report(n: u64) -> Vec<u8> {
    Response::Report { records: n, skipped: 0, notes: vec![] }.encode().unwrap()
}

fn session(stream: Vec<u8>) -> HostSession<std::io::Cursor<Vec<u8>>> {
    HostSession::new(std::io::Cursor::new(stream), budget(), ProtoLimits::DEFAULT, V1)
}

/// Runs a session to its end; returns the responses or the first error.
fn run(stream: Vec<u8>) -> Result<Vec<Response>, ProtoError> {
    let mut s = session(stream);
    let mut out = Vec::new();
    while let Some(r) = s.next_response()? {
        out.push(r);
    }
    Ok(out)
}

fn invalid(e: &ProtoError) -> (u64, &'static str) {
    match e {
        ProtoError::InvalidMessage { frame, detail } => (*frame, detail),
        other => panic!("expected invalid_message, got {other:?}"),
    }
}

/// A well-formed B-rep: every index points at an earlier record of the right kind.
fn good_brep() -> Vec<Record> {
    vec![
        rec(RecordBody::Surface { kind: SurfaceKind::Plane, params: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0] }), // 0
        rec(RecordBody::Curve { kind: CurveKind::Line, params: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0] }),      // 1
        rec(RecordBody::Vertex { point: [0.0, 0.0, 0.0] }),                                                // 2
        rec(RecordBody::Vertex { point: [1.0, 0.0, 0.0] }),                                                // 3
        rec(RecordBody::Edge { curve: 1, start: 2, end: 3 }),                                              // 4
        rec(RecordBody::Pcurve { surface: 0, params: vec![0.5] }),                                         // 5
        rec(RecordBody::Coedge { edge: 4, reversed: false, pcurve: Some(5) }),                             // 6
        rec(RecordBody::Loop { coedges: vec![6] }),                                                        // 7
        rec(RecordBody::Face { surface: 0, same_sense: true, loops: vec![7] }),                            // 8
        rec(RecordBody::Shell { closed: false, faces: vec![8] }),                                          // 9
        rec(RecordBody::Body { shells: vec![9] }),                                                         // 10
        rec(RecordBody::Product { bodies: vec![10] }),                                                     // 11
        rec(RecordBody::Product { bodies: vec![] }),                                                       // 12
        rec(RecordBody::Instance { parent: 12, child: 11, transform: [1.0; 12] }),                         // 13
        rec(RecordBody::PmiItem { kind: PmiKind::Datum, value: Some(0.01), targets: vec![8, 4, 2] }),      // 14
        rec(RecordBody::Skipped { reason: fid_cad_proto::SkipReason::Unsupported }),                       // 15
    ]
}

// ------------------------------------------------------------------ AC-1 / REQ-005

#[test]
fn ac1_seeded_messages_round_trip_and_match_committed_digests() {
    let samples = samples::samples();
    assert_eq!(samples.len(), samples::COUNT);
    let expected = digests();
    assert_eq!(expected.len(), samples::COUNT + 1, "digests.txt: one line per message plus `all`");
    let mut whole = Sha256::new();
    let mut b = budget();
    for (i, s) in samples.iter().enumerate() {
        let bytes = encode(s);
        // Byte-identical across runs: encoding twice gives the same bytes.
        assert_eq!(bytes, encode(s), "message {i}");
        whole.update(&bytes);
        assert_eq!(expected[i].0, i.to_string());
        assert_eq!(hex(&Sha256::digest(&bytes)), expected[i].1, "digest of message {i}");
        // Decode the frame we just wrote.
        let len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert_eq!(len + HEADER_LEN, bytes.len());
        let (k, payload) = (bytes[4], &bytes[HEADER_LEN..]);
        let at = Position::byte(0);
        let back = match s {
            Sample::Req(_) => Sample::Req(Request::decode(k, payload, i as u64, at, &mut b).unwrap()),
            Sample::Resp(_) => Sample::Resp(Response::decode(k, payload, i as u64, at, &mut b).unwrap()),
        };
        assert_eq!(&back, s, "round trip of message {i}");
    }
    assert_eq!(expected[samples::COUNT], ("all".to_string(), hex(&whole.finalize())));
}

#[test]
fn ac1_frame_reader_reads_back_the_concatenated_stream() {
    let samples = samples::samples();
    let stream: Vec<u8> = samples.iter().flat_map(encode).collect();
    let mut r = FrameReader::new(stream.as_slice(), budget(), ProtoLimits::DEFAULT);
    let mut n = 0u64;
    while let Some(RawFrame { frame, kind: k, payload }) = r.next_raw().unwrap() {
        assert_eq!(frame, n);
        let i = n as usize;
        let mut b = budget();
        let back = match &samples[i] {
            Sample::Req(_) => Sample::Req(Request::decode(k, payload, frame, Position::byte(0), &mut b).unwrap()),
            Sample::Resp(_) => Sample::Resp(Response::decode(k, payload, frame, Position::byte(0), &mut b).unwrap()),
        };
        assert_eq!(back, samples[i]);
        n += 1;
    }
    assert_eq!(n, samples::COUNT as u64);
    assert_eq!(r.session_bytes(), stream.len() as u64);
}

#[test]
fn req005_write_to_matches_to_bytes() {
    let f =
        Request::LoadStep { bytes: b"ISO-10303-21;".to_vec(), options: StepOptions { pmi: true } }.to_frame().unwrap();
    let mut out = Vec::new();
    f.write_to(&mut out).unwrap();
    assert_eq!(out, f.to_bytes().unwrap());
}

#[test]
fn req005_encoder_refuses_what_the_decoder_rejects() {
    let nan = Response::Records { seq: 0, records: vec![rec(RecordBody::Vertex { point: [0.0, f64::NAN, 0.0] })] };
    assert_eq!(nan.encode(), Err(EncodeError::NonFinite));
    let inf = Response::Records {
        seq: 0,
        records: vec![rec(RecordBody::PmiItem { kind: PmiKind::Note, value: Some(f64::INFINITY), targets: vec![] })],
    };
    assert_eq!(inf.encode(), Err(EncodeError::NonFinite));
    assert_eq!(Response::Progress { done: 2, total: 1 }.encode(), Err(EncodeError::ProgressPastTotal));
}

#[test]
fn req005_hand_checked_encoding() {
    // Hello 1.2: length 4, kind 0x01, major 1, minor 2.
    assert_eq!(
        Request::Hello { version: ProtoVersion { major: 1, minor: 2 } }.encode().unwrap(),
        [4, 0, 0, 0, 0x01, 1, 0, 2, 0]
    );
    assert_eq!(Request::Shutdown.encode().unwrap(), [0, 0, 0, 0, 0x04]);
    // Failed{Internal, "é"}: code 2, text length 2, UTF-8 C3 A9.
    assert_eq!(
        Response::Failed { code: FailCode::Internal, detail: WireText::new("\u{e9}") }.encode().unwrap(),
        [7, 0, 0, 0, 0x85, 2, 2, 0, 0, 0, 0xC3, 0xA9]
    );
    // One Skipped record from IGES DE 5: seq 3, count 1, tag 14, source 1 + u32 5, no name, reason 2.
    let r = Record {
        source: SourceRef::Iges(5),
        name: None,
        body: RecordBody::Skipped { reason: fid_cad_proto::SkipReason::Filtered },
    };
    assert_eq!(
        Response::Records { seq: 3, records: vec![r] }.encode().unwrap(),
        [16, 0, 0, 0, 0x83, 3, 0, 0, 0, 1, 0, 0, 0, 14, 1, 5, 0, 0, 0, 0, 2]
    );
}

// ------------------------------------------------------------------ REQ-001

#[test]
fn req001_source_travels_as_bytes_and_requests_name_no_path() {
    let src = b"ISO-10303-21;\nHEADER;".to_vec();
    let bytes = Request::LoadStep { bytes: src.clone(), options: StepOptions { pmi: false } }.encode().unwrap();
    // header (5) + pmi flag (1) + length (4) + the file bytes verbatim.
    assert_eq!(&bytes[HEADER_LEN..HEADER_LEN + 1], &[0]);
    assert_eq!(&bytes[HEADER_LEN + 1..HEADER_LEN + 5], &(src.len() as u32).to_le_bytes());
    assert_eq!(&bytes[HEADER_LEN + 5..], src.as_slice());
    let iges = Request::LoadIges { bytes: src.clone(), options: IgesOptions {} }.encode().unwrap();
    assert_eq!(&iges[HEADER_LEN + 4..], src.as_slice());
    // Exhaustive: adding a request variant breaks this match, forcing a review that the
    // new variant carries no path or address. No request field is text.
    for r in [Request::Hello { version: V1 }, Request::Shutdown] {
        match r {
            Request::Hello { version: ProtoVersion { .. } } => {}
            Request::LoadStep { bytes: _, options: StepOptions { pmi: _ } } => {}
            Request::LoadIges { bytes: _, options: IgesOptions {} } => {}
            Request::Shutdown => {}
        }
    }
}

#[test]
fn worker_side_decodes_requests_and_rejects_response_kinds() {
    let mut stream = Request::Hello { version: V1 }.encode().unwrap();
    stream.extend(Request::LoadIges { bytes: vec![1, 2, 3], options: IgesOptions {} }.encode().unwrap());
    stream.extend(Request::Shutdown.encode().unwrap());
    stream.extend(hello_ack(V1));
    let mut r = FrameReader::new(stream.as_slice(), budget(), ProtoLimits::DEFAULT);
    assert_eq!(r.next_request().unwrap(), Some((0, Request::Hello { version: V1 })));
    assert!(matches!(r.next_request().unwrap(), Some((1, Request::LoadIges { .. }))));
    assert_eq!(r.next_request().unwrap(), Some((2, Request::Shutdown)));
    let e = r.next_request().unwrap_err();
    assert_eq!(invalid(&e), (3, "unknown_request_kind"));
    // A bad bool in LoadStep.
    let mut b = budget();
    let e = Request::decode(kind::LOAD_STEP, &[2, 0, 0, 0, 0], 0, Position::byte(0), &mut b).unwrap_err();
    assert_eq!(invalid(&e), (0, "bad_bool"));
}

// ------------------------------------------------------------------ AC-2 / REQ-002

/// Yields `head`, then zeros forever, counting what it handed out.
struct Endless {
    head: Vec<u8>,
    given: u64,
}

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        for b in buf.iter_mut() {
            *b = self.head.get(self.given as usize).copied().unwrap_or(0);
            self.given += 1;
        }
        Ok(buf.len())
    }
}

#[test]
fn ac2_header_declaring_2_pow_31_bytes_fails_before_allocating() {
    assert_eq!(DEFAULT_FRAME_MAX, 64 * 1024 * 1024);
    assert_eq!(DEFAULT_SESSION_MAX, 8 * 1024 * 1024 * 1024);
    let head = vec![0x00, 0x00, 0x00, 0x80, kind::RECORDS]; // 2^31, little-endian
    let mut r = FrameReader::new(Endless { head, given: 0 }, budget(), ProtoLimits::DEFAULT);
    let e = r.next_response().unwrap_err();
    assert_eq!(
        e,
        ProtoError::FrameTooLarge { frame: 0, declared: 1 << 31, cap: Cap::Frame, limit: u64::from(DEFAULT_FRAME_MAX) }
    );
    assert_eq!(e.reason(), Some(CrashReason::FrameTooLarge));
    assert_eq!(e.code(), "reader_crashed");
    assert_eq!(e.to_io_error(), IoError::ReaderCrashed);
    assert!(e.to_string().starts_with("reader_crashed{reason: frame_too_large, frame: 0"));
    // Only the header was read and nothing was allocated for the payload.
    assert_eq!(r.budget().bytes_used(), HEADER_LEN as u64);
    assert!(r.budget().alloc_used() < 1 << 20, "tracked allocation {}", r.budget().alloc_used());
    assert_eq!(r.budget().alloc_used(), 0);
    // The reader stays failed.
    assert_eq!(r.next_response().unwrap_err(), e);
}

#[test]
fn req002_session_cap_counts_headers_and_payloads() {
    let frame = Response::Progress { done: 1, total: 2 }.encode().unwrap(); // 5 + 16 bytes
    let stream: Vec<u8> = [frame.clone(), frame.clone(), frame].concat();
    let limits = ProtoLimits::DEFAULT.session_max(42); // exactly two frames
    let mut r = FrameReader::new(stream.as_slice(), budget(), limits);
    assert!(r.next_response().unwrap().is_some());
    assert!(r.next_response().unwrap().is_some());
    let e = r.next_response().unwrap_err();
    assert_eq!(e, ProtoError::FrameTooLarge { frame: 2, declared: 16, cap: Cap::Session, limit: 42 });
}

#[test]
fn req002_limits_can_only_be_lowered() {
    let l = ProtoLimits::DEFAULT.frame_max(u32::MAX).session_max(u64::MAX).records_max(u64::MAX);
    assert_eq!(l, ProtoLimits::DEFAULT);
    assert_eq!(l.records_cap(), DEFAULT_RECORDS_MAX);
    let l = ProtoLimits::DEFAULT.frame_max(10);
    assert_eq!(l.frame_cap(), 10);
    let big = Response::Failed { code: FailCode::Parse, detail: WireText::new("0123456789") }.encode().unwrap();
    let e = FrameReader::new(big.as_slice(), budget(), l).next_response().unwrap_err();
    assert_eq!(e, ProtoError::FrameTooLarge { frame: 0, declared: 15, cap: Cap::Frame, limit: 10 });
}

#[test]
fn req002_lying_length_on_short_stream_allocates_only_what_arrives() {
    // Declares the full 64 MiB, then the stream ends after 100 bytes.
    let mut stream = DEFAULT_FRAME_MAX.to_le_bytes().to_vec();
    stream.push(kind::RECORDS);
    stream.extend([0u8; 100]);
    let mut r = FrameReader::new(stream.as_slice(), budget(), ProtoLimits::DEFAULT);
    let e = r.next_response().unwrap_err();
    assert_eq!(e, ProtoError::UnexpectedEof { frame: 0 });
    assert_eq!(e.reason(), Some(CrashReason::UnexpectedEof));
    assert!(r.budget().alloc_used() <= 8192, "tracked allocation {}", r.budget().alloc_used());
}

#[test]
fn req002_truncated_header_is_unexpected_eof_and_clean_end_is_none() {
    let mut r = FrameReader::new(&[1u8, 0][..], budget(), ProtoLimits::DEFAULT);
    assert_eq!(r.next_response().unwrap_err(), ProtoError::UnexpectedEof { frame: 0 });
    let mut r = FrameReader::new(&[][..], budget(), ProtoLimits::DEFAULT);
    assert_eq!(r.next_response().unwrap(), None);
}

#[test]
fn req002_count_larger_than_payload_is_rejected_before_allocating() {
    // Records with a count of 2^32 - 1 and nothing behind it.
    let payload = [0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF];
    let mut b = budget();
    let e = Response::decode(kind::RECORDS, &payload, 4, Position::byte(0), &mut b).unwrap_err();
    assert_eq!(invalid(&e), (4, "count_exceeds_payload"));
    assert_eq!(b.alloc_used(), 0);
}

#[test]
fn req002_allocation_is_charged_to_the_budget() {
    let small = Budget::new(Principal::Script, &LimitRequest::default().alloc(100)).unwrap();
    let stream = records(0, vec![rec(RecordBody::Curve { kind: CurveKind::Other, params: vec![0.0; 64] })]);
    let mut r = FrameReader::new(stream.as_slice(), small, ProtoLimits::DEFAULT);
    let e = r.next_response().unwrap_err();
    assert!(
        matches!(e, ProtoError::Harness(IoError::LimitExceeded { limit: LimitKind::Alloc, value: 100, .. })),
        "{e:?}"
    );
    assert_eq!(e.code(), "limit_exceeded");
}

#[test]
fn req002_record_cap_is_limit_exceeded() {
    let stream = [hello_ack(V1), records(0, good_brep())].concat();
    let mut s = HostSession::new(std::io::Cursor::new(stream), budget(), ProtoLimits::DEFAULT.records_max(3), V1);
    assert!(s.next_response().unwrap().is_some());
    let e = s.next_response().unwrap_err();
    assert!(
        matches!(e, ProtoError::Harness(IoError::LimitExceeded { limit: LimitKind::Elements, value: 3, .. })),
        "{e:?}"
    );
}

// ------------------------------------------------------------------ AC-3 / REQ-003

#[test]
fn ac3_face_with_out_of_range_loop_index() {
    let face = rec(RecordBody::Face { surface: 0, same_sense: true, loops: vec![7] });
    let surface = rec(RecordBody::Surface { kind: SurfaceKind::Plane, params: vec![] });
    let stream = [hello_ack(V1), records(0, vec![surface, face])].concat();
    let e = run(stream).unwrap_err();
    assert_eq!(e, ProtoError::InvalidMessage { frame: 1, detail: "index_out_of_range" });
    assert_eq!(e.reason(), Some(CrashReason::InvalidMessage));
    assert_eq!(e.frame(), Some(1));
    assert_eq!(e.code(), "reader_crashed");
}

#[test]
fn ac3_valid_session_is_accepted() {
    let brep = good_brep();
    let n = brep.len() as u64;
    let (a, b) = brep.split_at(7);
    let stream = [
        hello_ack(ProtoVersion { major: 1, minor: 4 }),
        Response::Progress { done: 0, total: 10 }.encode().unwrap(),
        records(0, a.to_vec()),
        records(1, b.to_vec()), // indices across batches
        report(n),
    ]
    .concat();
    let mut s = session(stream);
    let mut got = Vec::new();
    while let Some(r) = s.next_response().unwrap() {
        got.push(r);
    }
    assert_eq!(got.len(), 5);
    assert!(s.is_done());
    assert_eq!(s.negotiated(), Some(V1));
    assert_eq!(s.table().len() as u64, n);
    assert_eq!(s.table()[13], RecordKind::Instance);
}

#[test]
fn req003_session_validation_failures_name_the_frame() {
    let surface = || rec(RecordBody::Surface { kind: SurfaceKind::Plane, params: vec![] });
    let cases: Vec<(Vec<u8>, u64, &str)> = vec![
        // Index to a record of the wrong kind (pcurve must point at a surface).
        (
            [
                hello_ack(V1),
                records(
                    0,
                    vec![
                        rec(RecordBody::Vertex { point: [0.0; 3] }),
                        rec(RecordBody::Pcurve { surface: 0, params: vec![] }),
                    ],
                ),
            ]
            .concat(),
            1,
            "index_wrong_kind",
        ),
        // Index to itself (not earlier).
        (
            [hello_ack(V1), records(0, vec![rec(RecordBody::Loop { coedges: vec![0] })])].concat(),
            1,
            "index_out_of_range",
        ),
        // Instance of a product in itself.
        (
            [
                hello_ack(V1),
                records(
                    0,
                    vec![
                        rec(RecordBody::Product { bodies: vec![] }),
                        rec(RecordBody::Instance { parent: 0, child: 0, transform: [0.0; 12] }),
                    ],
                ),
            ]
            .concat(),
            1,
            "instance_self_reference",
        ),
        // Batches out of order.
        ([hello_ack(V1), records(0, vec![surface()]), records(2, vec![])].concat(), 2, "records_out_of_order"),
        // Report count disagrees with the table.
        ([hello_ack(V1), records(0, vec![surface()]), report(2)].concat(), 2, "report_count_mismatch"),
        // No handshake first.
        (report(0), 0, "expected_hello_ack"),
        // A second handshake.
        ([hello_ack(V1), hello_ack(V1)].concat(), 1, "unexpected_hello_ack"),
        // Anything after the final message.
        ([hello_ack(V1), report(0), report(0)].concat(), 2, "frame_after_end"),
    ];
    for (stream, frame, detail) in cases {
        let e = run(stream).unwrap_err();
        assert_eq!(invalid(&e), (frame, detail));
    }
}

#[test]
fn req003_eof_before_the_final_message() {
    let e = run([hello_ack(V1), records(0, vec![])].concat()).unwrap_err();
    assert_eq!(e, ProtoError::UnexpectedEof { frame: 2 });
    let ok =
        run([hello_ack(V1), Response::Failed { code: FailCode::Parse, detail: WireText::new("x") }.encode().unwrap()]
            .concat());
    assert_eq!(ok.unwrap().len(), 2);
}

#[test]
fn req003_schema_failures_name_the_frame() {
    let vertex = records(0, vec![rec(RecordBody::Vertex { point: [1.0, 2.0, 3.0] })]);
    // Offsets into `vertex`: header 5, seq 4, count 4, tag 1, source tag 1, u64 8, name tag 1.
    let point = HEADER_LEN + 4 + 4 + 1 + 1 + 8 + 1;
    let patch = |at: usize, with: &[u8]| {
        let mut v = vertex.clone();
        v[at..at + with.len()].copy_from_slice(with);
        v
    };
    let mut trailing = vertex.clone();
    trailing[0] += 1;
    trailing.push(0);
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (patch(point, &f64::NAN.to_bits().to_le_bytes()), "non_finite_float"),
        (patch(point + 8, &f64::NEG_INFINITY.to_bits().to_le_bytes()), "non_finite_float"),
        (patch(HEADER_LEN + 8, &[15]), "unknown_record_kind"),
        (patch(HEADER_LEN + 9, &[2]), "unknown_source_kind"),
        (patch(HEADER_LEN + 18, &[2]), "bad_option_tag"),
        (patch(4, &[0x86]), "unknown_response_kind"),
        (patch(4, &[kind::HELLO]), "unknown_response_kind"),
        (trailing, "trailing_bytes"),
    ];
    for (bytes, detail) in cases {
        let e = run([hello_ack(V1), bytes].concat()).unwrap_err();
        assert_eq!(invalid(&e), (1, detail));
    }
    // Enum ranges and other fields, checked on single payloads.
    let mut b = budget();
    let mut dec = |k: u8, p: &[u8]| Response::decode(k, p, 9, Position::byte(0), &mut b).unwrap_err();
    assert_eq!(invalid(&dec(kind::FAILED, &[4, 0, 0, 0, 0])), (9, "unknown_fail_code"));
    assert_eq!(invalid(&dec(kind::FAILED, &[0, 1, 0, 0, 0, 0xFF])), (9, "bad_utf8"));
    assert_eq!(
        invalid(&dec(kind::PROGRESS, &[[2u8, 0, 0, 0, 0, 0, 0, 0], [1, 0, 0, 0, 0, 0, 0, 0]].concat())),
        (9, "progress_past_total")
    );
    assert_eq!(invalid(&dec(kind::HELLO_ACK, &[1, 0])), (9, "payload_truncated"));
    let surface = |code: u8| [vec![0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0], vec![code, 0, 0, 0, 0]].concat();
    assert_eq!(invalid(&dec(kind::RECORDS, &surface(7))), (9, "unknown_surface_kind"));
    let skipped = [0, 0, 0, 0, 1, 0, 0, 0, 14, 1, 0, 0, 0, 0, 0, 3];
    assert_eq!(invalid(&dec(kind::RECORDS, &skipped)), (9, "unknown_skip_reason"));
    let coedge = [0, 0, 0, 0, 1, 0, 0, 0, 5, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0];
    assert_eq!(invalid(&dec(kind::RECORDS, &coedge)), (9, "bad_bool"));
}

// ------------------------------------------------------------------ AC-4 / REQ-004

#[test]
fn ac4_major_version_mismatch() {
    let worker = ProtoVersion { major: 2, minor: 0 };
    let e = run(hello_ack(worker)).unwrap_err();
    assert_eq!(e, ProtoError::ProtocolMismatch { host: V1, worker });
    assert_eq!(e.reason(), Some(CrashReason::ProtocolMismatch));
    assert_eq!(e.code(), "reader_crashed");
    assert_eq!(V1.major_label(), "1.x");
    assert_eq!(worker.major_label(), "2.x");
    assert_eq!(e.to_string(), "reader_crashed{reason: protocol_mismatch, host: \"1.x\", worker: \"2.x\"}");
}

#[test]
fn req004_same_major_negotiates_the_lower_minor() {
    let host = ProtoVersion { major: 1, minor: 3 };
    assert_eq!(negotiate(host, ProtoVersion { major: 1, minor: 1 }), Ok(ProtoVersion { major: 1, minor: 1 }));
    assert_eq!(negotiate(host, ProtoVersion { major: 1, minor: 9 }), Ok(host));
    assert!(negotiate(host, ProtoVersion { major: 0, minor: 3 }).is_err());
    assert_eq!(ProtoVersion::CURRENT, V1);
}

// ------------------------------------------------------------------ untrusted text

#[test]
fn worker_text_is_untrusted() {
    let name = WireText::new("evil\u{202e}txt.exe\n");
    let stream = [
        hello_ack(V1),
        records(
            0,
            vec![Record {
                source: SourceRef::Step(9),
                name: Some(name.clone()),
                body: RecordBody::Skipped { reason: fid_cad_proto::SkipReason::Malformed },
            }],
        ),
    ]
    .concat();
    let mut s = session(stream);
    s.next_response().unwrap();
    let Some(Response::Records { records, .. }) = s.next_response().unwrap() else { panic!("records") };
    let got = records[0].name.as_ref().unwrap();
    assert_eq!(got, &name);
    assert_eq!(got.to_untrusted().as_display().to_string(), "evil\\u{202e}txt.exe\\u{a}");
    assert_eq!(format!("{got:?}"), "UntrustedText(\"evil\\\\u{202e}txt.exe\\\\u{a}\")");
}

// ------------------------------------------------------------------ AC-5

/// The fuzz entry (same as `fuzz/fuzz_targets/cad_proto_session.rs`): small limits, run
/// the host session and the worker-side request decoder over `data`.
fn fuzz_one(data: &[u8]) -> (u64, u64) {
    let small = || Budget::new(Principal::Script, &LimitRequest::default().bytes(16 << 20).alloc(4 << 20)).unwrap();
    let limits = ProtoLimits::DEFAULT.frame_max(1 << 20).records_max(1 << 16);
    let mut s = HostSession::new(data, small(), limits, V1);
    for _ in 0..1024 {
        match s.next_response() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    let used = s.reader().budget().alloc_used();
    let mut r = FrameReader::new(data, small(), limits);
    for _ in 0..1024 {
        match r.next_request() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    (used, r.budget().alloc_used())
}

#[test]
fn ac5_mutated_streams_never_panic_or_pass_the_caps() {
    let mut rng = Rng::new(0x5EED);
    let samples = samples::samples();
    let valid = [hello_ack(V1), records(0, good_brep()), report(good_brep().len() as u64)].concat();
    let mut seeds: Vec<Vec<u8>> = samples.iter().take(200).map(encode).collect();
    seeds.push(valid.clone());
    seeds.push(samples.iter().flat_map(encode).collect());
    let mut runs = 0;
    for seed in &seeds {
        for _ in 0..20 {
            let mut v = seed.clone();
            let edits = 1 + rng.below(4);
            for _ in 0..edits {
                if v.is_empty() {
                    break;
                }
                let at = rng.below(v.len() as u64) as usize;
                match rng.below(4) {
                    0 => v[at] ^= 1 << rng.below(8),
                    1 => v[at] = rng.below(256) as u8,
                    2 => v.truncate(at),
                    _ => v.insert(at, rng.below(256) as u8),
                }
            }
            let (a, b) = fuzz_one(&v);
            assert!(a <= 4 << 20 && b <= 4 << 20);
            runs += 1;
        }
    }
    assert!(runs >= 4000);
    // The unmutated valid session is accepted.
    let mut s = HostSession::new(valid.as_slice(), budget(), ProtoLimits::DEFAULT, V1);
    while s.next_response().unwrap().is_some() {}
    assert!(s.is_done());
}
