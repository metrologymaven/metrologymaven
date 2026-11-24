//! Entry point for the `fid-mcp` MCP server (`AUTO.MCP.SRV`, `AUTO.MCP.SESS`,
//! `AUTO.MCP.DISC`, `AUTO.MCP.MODES.HEADLESS`).
//!
//! Pre-handshake errors (unknown profile, mode conflict, raw `--project` path) write only to
//! stderr, keep stdout empty, and exit with 2 (usage) or 3 (policy). Once validated, stdout
//! carries exclusively line-delimited JSON-RPC 2.0 responses and notifications.

use fid_canon::parse_with_limits;
use fid_mcp::disclosure::DisclosureState;
use fid_mcp::embedded::{embedded_manifest_bytes, embedded_profile_index, StandaloneMcpBus};
use fid_mcp::profiles::{parse_server_args, ProfileIndex, EXIT_USAGE, MAX_INDEX_BYTES};
use fid_mcp::session::{MemoryAuditSink, Session, SessionConfig};
use fid_mcp::ToolTable;
use fid_surface::projection::MAX_MANIFEST_BYTES;
use fid_surface::roots::is_handle;
use fid_surface::{project, Projection};
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::process::ExitCode;

const EXIT_POLICY: u8 = 3;
const EXIT_NOT_RUN: u8 = 6;

const HELP: &str = "\
fid-mcp: Metrology Maven Model Context Protocol server (AUTO.MCP)

USAGE:
    fid-mcp [--headless | --bridge] [--server <profile>] [--project <root_handle>]
            [--profiles-index <path>] [--manifest <path>] [--agent <name>]
            [--print-profile] [--help]
";

fn read_bounded(path: &str, max_bytes: usize, label: &str) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| format!("{label}: {path}: {e}"))?;
    let mut buf = Vec::new();
    let limit = (max_bytes as u64).saturating_add(1);
    file.take(limit).read_to_end(&mut buf).map_err(|e| format!("{label}: {path}: {e}"))?;
    if buf.len() > max_bytes {
        return Err(format!("{label}: {path}: exceeds maximum size of {max_bytes} bytes"));
    }
    Ok(buf)
}

fn load_manifest(path: Option<&str>) -> Result<(Vec<u8>, Projection), String> {
    let bytes = match path {
        Some(p) => read_bounded(p, MAX_MANIFEST_BYTES, "manifest_parse")?,
        None => embedded_manifest_bytes(),
    };
    let proj = project(&bytes).map_err(|e| e.to_string())?;
    Ok((bytes, proj))
}

fn load_index(path: Option<&str>, proj: &Projection) -> Result<ProfileIndex, String> {
    match path {
        Some(p) => {
            let bytes = read_bounded(p, MAX_INDEX_BYTES, "profile_parse")?;
            ProfileIndex::from_canonical_bytes(&bytes).map_err(|e| e.to_string())
        }
        None => embedded_profile_index(proj).map_err(|e| e.to_string()),
    }
}

fn run(args: &[String]) -> ExitCode {
    let parsed = match parse_server_args(args) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    if parsed.help {
        let _ = write!(std::io::stdout(), "{HELP}");
        return ExitCode::SUCCESS;
    }

    // REQ-AUTO.MCP.MODES.HEADLESS-003: raw --project path exits 3 before any MCP handshake.
    if let Some(proj_ref) = &parsed.project {
        if !is_handle(proj_ref) {
            let _ =
                writeln!(std::io::stderr(), "raw_path_refused: `--project` must be a `root:` handle, not a raw path");
            return ExitCode::from(EXIT_POLICY);
        }
    }

    if parsed.bridge {
        let _ = writeln!(
            std::io::stderr(),
            "app_unreachable: bridge mode could not reach an authenticated desktop app within 5 s"
        );
        return ExitCode::from(EXIT_NOT_RUN);
    }

    let (manifest_bytes, projection) = match load_manifest(parsed.manifest.as_deref()) {
        Ok(pair) => pair,
        Err(msg) => {
            let _ = writeln!(std::io::stderr(), "{msg}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let index = match load_index(parsed.profiles_index.as_deref(), &projection) {
        Ok(idx) => idx,
        Err(msg) => {
            let _ = writeln!(std::io::stderr(), "{msg}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let profile = match index.select(parsed.server.as_deref()) {
        Ok(p) => p.clone(),
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    if parsed.print_profile {
        let _ = writeln!(
            std::io::stdout(),
            "profile={} served={} front_door={}",
            profile.name,
            profile.served.len(),
            profile.front_door.len()
        );
        return ExitCode::SUCCESS;
    }

    let manifest_val = match parse_with_limits(&manifest_bytes, MAX_MANIFEST_BYTES, 64) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "manifest_parse: {e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let disclosure = match DisclosureState::new(&projection, &profile) {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let table = match ToolTable::from_projection_and_manifest(&projection, &manifest_val) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let cfg = SessionConfig {
        agent_name: parsed.agent.unwrap_or_else(|| "fid-agent".to_string()),
        mode: "headless".to_string(),
        mode_reason: if parsed.headless { "flag" } else { "default" }.to_string(),
        read_only_holder: None,
    };

    let bus = StandaloneMcpBus::new(parsed.project);
    let mut session = Session::new(cfg, disclosure, table, bus, MemoryAuditSink::default());

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match session.run_stdio(stdin.lock(), stdout.lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "io_error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    run(&args)
}
