//! Reference reader for the Result Package conformance runner `fixtures/pkg/run-conformance`
//! (spec node `TRUST.PKG.FIXTURE`, AC-2).
//!
//! `fid-pkg-conformance <package>` opens the package with [`PackageReader`], verifies every
//! member against the manifest, and prints one line of JSON on standard output:
//! `{"result":"accepted"}`, or `{"error_code":"pkg.<code>","reason":"<tag>","result":"rejected"}`.
//! It exits 0 when accepted, 1 when rejected and 2 on a usage error. Signatures are not checked.

use fid_pkg::reader::{PackageReader, ReadError};
use std::fs::File;
use std::io::BufReader;
use std::process::ExitCode;

fn check(path: &str) -> Result<(), ReadError> {
    let file = File::open(path).map_err(|e| ReadError::Io(e.to_string()))?;
    PackageReader::open(BufReader::new(file))?.verify_all()
}

/// JSON string body for the ASCII tags the reader uses; anything else is escaped as `\uXXXX`.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out.push('"');
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("usage: fid-pkg-conformance <package.zip>");
        return ExitCode::from(2);
    };
    match check(path) {
        Ok(()) => {
            println!("{{\"result\":\"accepted\"}}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!(
                "{{\"error_code\":{},\"reason\":{},\"result\":\"rejected\"}}",
                json_str(e.code()),
                json_str(e.reason())
            );
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
