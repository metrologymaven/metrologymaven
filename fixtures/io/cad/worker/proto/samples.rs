// Seeded message generator for IO.CAD.WORKER.PROTO AC-1 (seed 7, 1,000 messages). CC-BY-4.0.
//
// Included by crates/fid-cad-proto/tests/acceptance.rs. make_digests.py next to this file
// implements the same generator and, independently of the Rust crate, the wire encoding,
// and writes digests.txt. The two generators must call the RNG in exactly the same order:
// fields are drawn in wire order, one `let` per draw.

#![allow(dead_code)]

use fid_cad_proto::{
    CurveKind, FailCode, IgesOptions, PmiKind, ProtoVersion, Record, RecordBody, Request, Response, SkipReason,
    SourceRef, StepOptions, SurfaceKind, WireText,
};

pub const SEED: u64 = 7;
pub const COUNT: usize = 1000;
pub const KIND_NAMES: [&str; 9] =
    ["hello", "load_step", "load_iges", "shutdown", "hello_ack", "progress", "records", "report", "failed"];
pub const ALPHABET: [&str; 12] =
    ["a", "Z", "0", " ", "\\", "\"", "\u{e9}", "\u{4e2d}", "\u{1f600}", "\u{202e}", "\n", "\u{0}"];

/// SplitMix64.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn flag(&mut self) -> bool {
        self.below(2) == 1
    }
    fn f(&mut self) -> f64 {
        let x = (self.next() >> 11) as i64 - (1i64 << 52);
        x as f64 / 1024.0
    }
    fn idx(&mut self) -> u32 {
        self.below(1 << 32) as u32
    }
    fn text(&mut self) -> WireText {
        let n = self.below(9);
        let mut s = String::new();
        for _ in 0..n {
            let c = self.below(ALPHABET.len() as u64) as usize;
            s.push_str(ALPHABET[c]);
        }
        WireText::new(&s)
    }
    fn bytes(&mut self) -> Vec<u8> {
        let n = self.below(64);
        (0..n).map(|_| self.below(256) as u8).collect()
    }
    fn version(&mut self) -> ProtoVersion {
        let major = self.below(4) as u16;
        let minor = self.below(65536) as u16;
        ProtoVersion { major, minor }
    }
    fn fvec(&mut self, max: u64) -> Vec<f64> {
        let n = self.below(max + 1);
        (0..n).map(|_| self.f()).collect()
    }
    fn ivec(&mut self, max: u64) -> Vec<u32> {
        let n = self.below(max + 1);
        (0..n).map(|_| self.idx()).collect()
    }
    fn record(&mut self) -> Record {
        let tag = self.below(15);
        let source = if self.below(2) == 0 { SourceRef::Step(self.next()) } else { SourceRef::Iges(self.idx()) };
        let name = if self.below(2) == 0 { None } else { Some(self.text()) };
        let body = match tag {
            0 => {
                let kind = SurfaceKind::ALL[self.below(7) as usize];
                let params = self.fvec(6);
                RecordBody::Surface { kind, params }
            }
            1 => {
                let kind = CurveKind::ALL[self.below(5) as usize];
                let params = self.fvec(6);
                RecordBody::Curve { kind, params }
            }
            2 => {
                let surface = self.idx();
                let params = self.fvec(6);
                RecordBody::Pcurve { surface, params }
            }
            3 => {
                let x = self.f();
                let y = self.f();
                let z = self.f();
                RecordBody::Vertex { point: [x, y, z] }
            }
            4 => {
                let curve = self.idx();
                let start = self.idx();
                let end = self.idx();
                RecordBody::Edge { curve, start, end }
            }
            5 => {
                let edge = self.idx();
                let reversed = self.flag();
                let pcurve = if self.below(2) == 0 { None } else { Some(self.idx()) };
                RecordBody::Coedge { edge, reversed, pcurve }
            }
            6 => RecordBody::Loop { coedges: self.ivec(5) },
            7 => {
                let surface = self.idx();
                let same_sense = self.flag();
                let loops = self.ivec(3);
                RecordBody::Face { surface, same_sense, loops }
            }
            8 => {
                let closed = self.flag();
                let faces = self.ivec(5);
                RecordBody::Shell { closed, faces }
            }
            9 => RecordBody::Body { shells: self.ivec(3) },
            10 => RecordBody::Product { bodies: self.ivec(3) },
            11 => {
                let parent = self.idx();
                let child = self.idx();
                let mut transform = [0.0; 12];
                for t in transform.iter_mut() {
                    *t = self.f();
                }
                RecordBody::Instance { parent, child, transform }
            }
            12 => {
                let kind = PmiKind::ALL[self.below(4) as usize];
                let value = if self.below(2) == 0 { None } else { Some(self.f()) };
                let targets = self.ivec(4);
                RecordBody::PmiItem { kind, value, targets }
            }
            13 => {
                let entity_type = self.below(65536) as u16;
                let form = self.below(65536) as u16;
                let params = self.fvec(6);
                RecordBody::IgesEntity { entity_type, form, params }
            }
            _ => RecordBody::Skipped { reason: SkipReason::ALL[self.below(3) as usize] },
        };
        Record { source, name, body }
    }
}

/// One generated message.
#[derive(Debug, Clone, PartialEq)]
pub enum Sample {
    Req(Request),
    Resp(Response),
}

/// Message `i` has kind `i % 9` (in `KIND_NAMES` order).
pub fn samples() -> Vec<Sample> {
    let mut r = Rng::new(SEED);
    let mut out = Vec::with_capacity(COUNT);
    for i in 0..COUNT {
        let s = match i % 9 {
            0 => Sample::Req(Request::Hello { version: r.version() }),
            1 => {
                let pmi = r.flag();
                let bytes = r.bytes();
                Sample::Req(Request::LoadStep { bytes, options: StepOptions { pmi } })
            }
            2 => Sample::Req(Request::LoadIges { bytes: r.bytes(), options: IgesOptions {} }),
            3 => Sample::Req(Request::Shutdown),
            4 => {
                let version = r.version();
                let reader = r.text();
                let reader_version = r.text();
                Sample::Resp(Response::HelloAck { version, reader, reader_version })
            }
            5 => {
                let total = r.next() >> 1;
                let done = r.next() % (total + 1);
                Sample::Resp(Response::Progress { done, total })
            }
            6 => {
                let seq = r.idx();
                let n = r.below(8);
                let records = (0..n).map(|_| r.record()).collect();
                Sample::Resp(Response::Records { seq, records })
            }
            7 => {
                let records = r.next();
                let skipped = r.next();
                let n = r.below(4);
                let notes = (0..n).map(|_| r.text()).collect();
                Sample::Resp(Response::Report { records, skipped, notes })
            }
            _ => {
                let code = FailCode::ALL[r.below(4) as usize];
                let detail = r.text();
                Sample::Resp(Response::Failed { code, detail })
            }
        };
        out.push(s);
    }
    out
}
