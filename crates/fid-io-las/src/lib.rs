//! LAS point-cloud files, pure Rust (spec node IO.CLOUD.LAS and its leaves).
//!
//! This crate currently holds IO.CLOUD.LAS.HDR: the uncompressed LAS structure of
//! versions 1.0 to 1.4 (ASPRS LAS specification, cited by designator only). It reads and
//! writes the public header block ([`LasHeader`]), VLRs and EVLRs with bounded, untrusted
//! payloads ([`Vlr`], [`BoundedBytes`]), and fixed-size point records in formats 0 to 10
//! with extra bytes ([`RawPointBatch`]: integer coordinates plus typed attribute columns).
//! Waveform data is counted, never imported ([`WaveformSummary`]). Scale and offset
//! arithmetic is `IO.CLOUD.LAS.QUANT`; compression is `IO.CLOUD.LAS.LAZ`.
//!
//! All input is hostile. [`LasReader::open`] checks every declared size, offset and count
//! against the real file length before trusting it (`malformed{field}`), requires the
//! declared point count times the record length to equal the bytes of point data
//! (`truncated_file{declared, actual}`), bounds each record payload (64 MiB by default)
//! and charges kept bytes to the IO.REG.LIMITS allocation limit. No code path panics.
//!
//! LAZ files set the top bits of the point format byte; this layer rejects them as
//! `malformed{point_data_format}` and leaves them to the LAZ layer.

mod bytes;
mod error;
mod format;
mod header;
mod points;
mod reader;
mod vlr;
mod writer;

pub use error::{CountUnit, LasError, LasField};
pub use format::{PointFormat, WAVE_PACKET_LEN};
pub use header::{standard_header_len, LasHeader, HEADER_LEN_1_0, HEADER_LEN_1_3, HEADER_LEN_1_4, SIGNATURE};
pub use points::{AttributeColumns, ExtraColumn, ExtraValues, RawPointBatch};
pub use reader::{read_slice, LasFile, LasReader, ReaderOptions, WaveformSummary, DEFAULT_BATCH_POINTS};
pub use vlr::{
    extra_layout, BoundedBytes, ExtraKind, ExtraSpec, RecordPayload, Vlr, VlrKind, DEFAULT_MAX_PAYLOAD,
    EVLR_HEADER_LEN, EXTRA_BYTES_DESCRIPTOR_LEN, RECORD_EXTRA_BYTES, RECORD_WAVEFORM_DATA, USER_LASF_PROJECTION,
    USER_LASF_SPEC, VLR_HEADER_LEN,
};
pub use writer::{write_file, LasWriter};

/// Reader options for fuzzing: small limits so a hostile input cannot ask for much memory.
pub fn fuzz_options() -> ReaderOptions {
    let request = fid_io_reg::LimitRequest::default().alloc(64 * 1024 * 1024).elements(1 << 20);
    ReaderOptions {
        limits: fid_io_reg::Limits::resolve(fid_io_reg::Principal::Script, &request)
            .unwrap_or(fid_io_reg::Limits::DEFAULT),
        max_record_payload: 1 << 20,
        batch_points: 4096,
    }
}
