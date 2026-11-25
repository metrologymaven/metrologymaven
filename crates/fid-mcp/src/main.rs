//! Thin CLI entry point for `fid-mcp` server profile selection (spec node `AUTO.MCP.SRV`).
//!
//! Validates `--server <profile>` (default: `core`) against the profile index before any MCP
//! handshake. On an unknown profile, writes `unknown_profile` listing the known profiles to
//! stderr, writes nothing to stdout, and exits with code 2 (REQ-AUTO.MCP.SRV-007, -008).

use fid_mcp::profiles::{parse_server_args, ProfileIndex, EXIT_USAGE, MAX_INDEX_BYTES};
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::process::ExitCode;

/// Minimal default index embedded when no `--profiles-index` path is provided.
const DEFAULT_EMBEDDED_INDEX: &str = r#"{"format":1,"manifest_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000","profiles":[{"front_door":["tools.search_tools@1","tools.describe_tool@1","tools.list_domains@1","tools.get_workflow@1"],"name":"core","served":["tools.describe_tool@1","tools.get_workflow@1","tools.list_domains@1","tools.search_tools@1"],"title":"Metrology Maven Core"}],"surface":"mcp"}"#;

fn load_index(path: Option<&str>) -> Result<ProfileIndex, String> {
    let bytes = match path {
        Some(p) => {
            let file = File::open(p).map_err(|e| format!("profile_parse: {p}: {e}"))?;
            let mut buf = Vec::new();
            let limit = (MAX_INDEX_BYTES as u64).saturating_add(1);
            file.take(limit).read_to_end(&mut buf).map_err(|e| format!("profile_parse: {p}: {e}"))?;
            if buf.len() > MAX_INDEX_BYTES {
                return Err(format!("profile_parse: {p}: exceeds maximum index size of {MAX_INDEX_BYTES} bytes"));
            }
            buf
        }
        None => DEFAULT_EMBEDDED_INDEX.as_bytes().to_vec(),
    };
    ProfileIndex::from_canonical_bytes(&bytes).map_err(|e| e.to_string())
}

fn run(args: &[String]) -> ExitCode {
    let parsed = match parse_server_args(args) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let index = match load_index(parsed.profiles_index.as_deref()) {
        Ok(idx) => idx,
        Err(msg) => {
            let _ = writeln!(std::io::stderr(), "{msg}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let profile = match index.select(parsed.server.as_deref()) {
        Ok(p) => p,
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

    // Session runtime belongs to AUTO.MCP.SESS; fail cleanly on stderr without writing MCP traffic.
    let _ = writeln!(
        std::io::stderr(),
        "selected profile `{}` ({} served, {} front-door); MCP session transport is implemented in AUTO.MCP.SESS",
        profile.name,
        profile.served.len(),
        profile.front_door.len()
    );
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    run(&args)
}
