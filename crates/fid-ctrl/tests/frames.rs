//! Framing and schema against the seed corpus `fixtures/ctrl/frames/` (verdicts from
//! `make_frames.py`, an independent Python reading of the rules), plus bound edge cases.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_ctrl::frame::{Frame, LineReader};
use fid_ctrl::msg::{classify_stream, negotiate, parse_request, reply_limit, Reply, Verdict, MAX_MESSAGE_BYTES};
use fid_ctrl::WireError;
use std::io::Read;
use std::path::{Path, PathBuf};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ctrl/frames")
}

#[test]
fn corpus_verdicts_match_the_python_reference() {
    let expected = std::fs::read_to_string(corpus().join("expected.txt")).unwrap();
    let mut n = 0;
    for row in expected.lines() {
        let mut parts = row.split(' ');
        let name = parts.next().unwrap();
        let want: Vec<&str> = parts.collect();
        let bytes = std::fs::read(corpus().join(name)).unwrap();
        let got: Vec<String> = classify_stream(&bytes, MAX_MESSAGE_BYTES).iter().map(|v| v.to_string()).collect();
        assert_eq!(got, want, "{name}");
        n += 1;
    }
    let files = std::fs::read_dir(corpus())
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "jsonl"))
        .count();
    assert_eq!(n, files, "every corpus file has an expected row");
    assert!(n >= 40);
}

#[test]
fn line_bound_is_exact() {
    let mut ok = vec![b'x'; 10];
    ok.push(b'\n');
    assert_eq!(LineReader::new(&ok[..]).next_line(10).unwrap(), Frame::Line(vec![b'x'; 10]));
    let mut over = vec![b'x'; 11];
    over.push(b'\n');
    assert_eq!(LineReader::new(&over[..]).next_line(10).unwrap(), Frame::TooLarge);
    assert_eq!(classify_stream(&over, 10), vec![Verdict::TooLarge]);
    // Without a newline: limit bytes are a partial line, limit + 1 are too large.
    assert_eq!(LineReader::new(&[b'x'; 10][..]).next_line(10).unwrap(), Frame::PartialEof);
    assert_eq!(LineReader::new(&[b'x'; 11][..]).next_line(10).unwrap(), Frame::TooLarge);
    assert_eq!(LineReader::new(&b""[..]).next_line(10).unwrap(), Frame::Eof);
}

/// A source that would never end; counts what was pulled from it.
struct Endless {
    pulled: usize,
}

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        buf.fill(b'a');
        self.pulled += buf.len();
        Ok(buf.len())
    }
}

#[test]
fn reader_pulls_at_most_limit_plus_one_bytes_before_deciding() {
    let mut r = LineReader::new(Endless { pulled: 0 });
    assert_eq!(r.next_line(MAX_MESSAGE_BYTES).unwrap(), Frame::TooLarge);
    assert_eq!(r.get_mut().pulled, MAX_MESSAGE_BYTES + 1);
    // Discarding is bounded too.
    let mut r = LineReader::new(Endless { pulled: 0 });
    assert!(!r.discard_line(1000).unwrap());
    assert!(r.get_mut().pulled <= 1000);
}

#[test]
fn lines_after_an_oversized_line_are_not_read() {
    let mut s = vec![b'y'; 20];
    s.extend_from_slice(b"\n{\"v\":1,\"id\":1,\"op\":\"ping\",\"params\":{}}\n");
    assert_eq!(classify_stream(&s, 16), vec![Verdict::TooLarge]);
}

#[test]
fn request_fields_are_typed() {
    let r = parse_request(br#"{"v":1,"id":77,"op":"jobs.cancel","params":{"job":"j1"}}"#).unwrap();
    assert_eq!((r.v, r.id, r.op.as_str()), (1, 77, "jobs.cancel"));
}

#[test]
fn negotiation_picks_the_highest_common_version() {
    assert_eq!(negotiate(&[1]), Some(1));
    assert_eq!(negotiate(&[3, 1, 2]), Some(1));
    assert_eq!(negotiate(&[99]), None);
    assert_eq!(negotiate(&[]), None);
}

#[test]
fn replies_round_trip_and_are_canonical() {
    let r = Reply::error(Some(5), WireError::VersionUnsupported, "no");
    let line = r.encode().unwrap();
    assert_eq!(
        line,
        b"{\"event\":{\"code\":\"version_unsupported\",\"kind\":\"error\",\"message\":\"no\",\"versions\":[1]},\"id\":5,\"v\":1}\n"
    );
    assert_eq!(Reply::decode(&line[..line.len() - 1], "ping").unwrap(), r);
    let none = Reply::error(None, WireError::MalformedMessage, "x").encode().unwrap();
    assert!(none.windows(9).any(|w| w == b"\"id\":null"));
    assert!(matches!(Reply::decode(b"{\"v\":1,\"id\":1}", "ping"), Err(WireError::MalformedMessage)));
    assert!(matches!(
        Reply::decode(b"{\"v\":1,\"id\":1,\"event\":{},\"envelope\":{}}", "ping"),
        Err(WireError::MalformedMessage)
    ));
}

#[test]
fn screenshot_replies_get_the_larger_bound() {
    assert_eq!(reply_limit("ui.screenshot"), 16 * 1024 * 1024);
    assert_eq!(reply_limit("ping"), 1024 * 1024);
}
