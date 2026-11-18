//! `board check-claims`: the register against the board and the sources.

use crate::board::{parse_board, Board, Outcome};
use crate::register::{parse_register, Claim, Register, MAX_INPUT_BYTES};
use crate::scan::{parse_phrases, scan_text, Tag, SOURCE_EXTENSIONS};
use crate::{display_path, Code, Finding};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

/// The inputs of one check.
#[derive(Debug, Clone)]
pub struct Inputs<'a> {
    /// The claim register, e.g. `agent/claims.json`.
    pub register: &'a Path,
    /// A scoreboard bundle directory (holding `board.json`) or a `board.json` file.
    pub board: &'a Path,
    /// The phrase list; `None` means `claim-phrases.txt` next to the register.
    pub phrases: Option<&'a Path>,
    /// Source files or directories (scanned recursively for `.md`, `.markdown`, `.html`, `.htm`).
    pub sources: &'a [PathBuf],
}

/// The outcome of a check.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Sorted findings; empty when the check passes.
    pub findings: Vec<Finding>,
    pub claims: usize,
    pub tags: usize,
    pub files: usize,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Runs the check. Reads only the given local files.
pub fn check_claims(inp: &Inputs<'_>) -> Report {
    let mut report = Report::default();
    let reg_file = display_path(inp.register);
    let register = read(inp.register, &reg_file, Code::InvalidRegister).and_then(|b| parse_register(&b, &reg_file));

    let board_path = if inp.board.is_dir() { inp.board.join("board.json") } else { inp.board.to_path_buf() };
    let board_file = display_path(&board_path);
    let board = read(&board_path, &board_file, Code::InvalidBoard).and_then(|b| parse_board(&b, &board_file));

    let phrases_path = match inp.phrases {
        Some(p) => p.to_path_buf(),
        None => inp.register.parent().unwrap_or_else(|| Path::new("")).join("claim-phrases.txt"),
    };
    let phrases_file = display_path(&phrases_path);
    let phrases = read(&phrases_path, &phrases_file, Code::InvalidPhrases).and_then(|b| {
        String::from_utf8(b)
            .map(|t| parse_phrases(&t))
            .map_err(|_| vec![Finding::new(Code::InvalidPhrases, None, &phrases_file, 0, "not valid UTF-8")])
    });

    let (register, board, phrases) = match (register, board, phrases) {
        (Ok(r), Ok(b), Ok(p)) => (r, b, p),
        (r, b, p) => {
            for e in [r.err(), b.err(), p.err()].into_iter().flatten() {
                report.findings.extend(e);
            }
            report.findings.sort();
            return report;
        }
    };

    let mut tags = Vec::new();
    for root in inp.sources {
        match collect_sources(root) {
            Ok(files) => {
                for (path, shown) in files {
                    report.files += 1;
                    match read_text(&path, &shown) {
                        Ok(text) => {
                            let (t, f) = scan_text(&text, &shown, &phrases);
                            tags.extend(t);
                            report.findings.extend(f);
                        }
                        Err(f) => report.findings.push(f),
                    }
                }
            }
            Err(f) => report.findings.push(f),
        }
    }
    tags.sort();
    report.claims = register.claims.len();
    report.tags = tags.len();

    for t in &tags {
        if register.get(&t.id).is_none() {
            report.findings.push(Finding::new(
                Code::UnknownClaim,
                Some(&t.id),
                &t.file,
                t.line,
                format!("tag names claim {:?}, which is not in the register {}", t.id, register.file),
            ));
        }
    }
    for c in &register.claims {
        evaluate(c, &register, &board, &tags, &mut report.findings);
    }
    report.findings.sort();
    report.findings.dedup();
    report
}

/// Checks one register entry against the board; reports at each tag citing it, or at the
/// register entry when no source cites it.
fn evaluate(c: &Claim, register: &Register, board: &Board, tags: &[Tag], out: &mut Vec<Finding>) {
    let mut problems: Vec<(Code, String)> = Vec::new();
    let what = format!("{} of model {:?} on {} {:?}", c.metric.as_str(), c.model, c.scope.kind.as_str(), c.scope.id);
    if c.release != board.release {
        problems.push((
            Code::EntryMissing,
            format!("claim names release {:?} but the board is release {:?}", c.release, board.release),
        ));
    } else {
        if matches!(board.verdict, Outcome::Block | Outcome::Incomplete) {
            problems.push((
                Code::ClaimOnBlockedRelease,
                format!("release {:?} has red-team verdict `{}`", board.release, board.verdict.as_str()),
            ));
        }
        match board.models.get(&c.model) {
            None => problems.push((Code::EntryMissing, format!("the board has no model {:?}", c.model))),
            Some(m) => match m.scope(c.scope.kind).get(&c.scope.id) {
                None => problems.push((
                    Code::EntryMissing,
                    format!("model {:?} has no {} {:?} on the board", c.model, c.scope.kind.as_str(), c.scope.id),
                )),
                Some(fig) => match fig.figure(c.metric) {
                    Err(why) => problems.push((Code::EntryMissing, format!("{what}: {why}"))),
                    Ok(f) => {
                        let ord = f.cmp_exact(c.threshold);
                        let (met, op) =
                            if c.metric.is_rate() { (ord != Ordering::Less, ">=") } else { (ord != Ordering::Greater, "<=") };
                        if !met {
                            problems.push((
                                Code::ThresholdNotMet,
                                format!("{what} is {f} on the board; the claim needs {op} {}", c.threshold),
                            ));
                        }
                    }
                },
            },
        }
    }
    let sites: Vec<&Tag> = tags.iter().filter(|t| t.id == c.id).collect();
    for (code, detail) in problems {
        if sites.is_empty() {
            out.push(Finding::new(code, Some(&c.id), &register.file, c.line, format!("{detail} (claim not cited)")));
        } else {
            for s in &sites {
                out.push(Finding::new(
                    code,
                    Some(&c.id),
                    &s.file,
                    s.line,
                    format!("{detail} (register {}:{})", register.file, c.line),
                ));
            }
        }
    }
}

fn read(path: &Path, shown: &str, code: Code) -> Result<Vec<u8>, Vec<Finding>> {
    read_one(path, shown, code).map_err(|f| vec![f])
}

fn read_one(path: &Path, shown: &str, code: Code) -> Result<Vec<u8>, Finding> {
    let meta = fs::metadata(path).map_err(|e| Finding::new(code, None, shown, 0, format!("cannot read: {e}")))?;
    if meta.len() > MAX_INPUT_BYTES as u64 {
        return Err(Finding::new(code, None, shown, 0, "file larger than 16 MiB"));
    }
    fs::read(path).map_err(|e| Finding::new(code, None, shown, 0, format!("cannot read: {e}")))
}

fn read_text(path: &Path, shown: &str) -> Result<String, Finding> {
    let bytes = read_one(path, shown, Code::InvalidSource)?;
    String::from_utf8(bytes).map_err(|_| Finding::new(Code::InvalidSource, None, shown, 0, "not valid UTF-8"))
}

/// The source files under `root` (or `root` itself if it is a file), sorted by relative path,
/// each with the path shown in findings. Symbolic links are not followed.
fn collect_sources(root: &Path) -> Result<Vec<(PathBuf, String)>, Finding> {
    let shown_root = display_path(root);
    let meta = fs::metadata(root)
        .map_err(|e| Finding::new(Code::InvalidSource, None, &shown_root, 0, format!("cannot read: {e}")))?;
    if meta.is_file() {
        return Ok(vec![(root.to_path_buf(), shown_root)]);
    }
    let mut out = Vec::new();
    walk(root, &shown_root, &mut out)?;
    Ok(out)
}

fn walk(dir: &Path, shown: &str, out: &mut Vec<(PathBuf, String)>) -> Result<(), Finding> {
    let fail = |e: std::io::Error| Finding::new(Code::InvalidSource, None, shown, 0, format!("cannot read: {e}"));
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for e in fs::read_dir(dir).map_err(fail)? {
        let e = e.map_err(fail)?;
        entries.push((e.file_name().to_string_lossy().into_owned(), e.path()));
    }
    entries.sort();
    for (name, path) in entries {
        let child = format!("{shown}/{name}");
        let ft = fs::symlink_metadata(&path).map_err(fail)?.file_type();
        if ft.is_dir() {
            walk(&path, &child, out)?;
        } else if ft.is_file() {
            let ext = path.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if SOURCE_EXTENSIONS.contains(&ext.as_str()) {
                out.push((path, child));
            }
        }
    }
    Ok(())
}
