//! Reads resolved dependency versions from `Cargo.lock` text, so the report states the exact
//! egui, wgpu and winit versions that were built (REQ-VIZ.SHELL.SPKTREE-005).

/// Largest lockfile text accepted.
pub const MAX_LOCK_BYTES: usize = 8 * 1024 * 1024;

/// The versions of package `name` in the lockfile, in file order (a name can appear more
/// than once). Empty when the text is too large or the package is absent.
pub fn versions(lock: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if lock.len() > MAX_LOCK_BYTES {
        return out;
    }
    let want = format!("name = \"{name}\"");
    let mut in_pkg = false;
    let mut matched = false;
    for line in lock.lines() {
        let line = line.trim_end_matches('\r');
        if line == "[[package]]" {
            in_pkg = true;
            matched = false;
        } else if line.starts_with('[') {
            in_pkg = false;
        } else if in_pkg && line == want {
            matched = true;
        } else if in_pkg && matched {
            if let Some(v) = line.strip_prefix("version = \"").and_then(|r| r.strip_suffix('"')) {
                out.push(v.to_string());
                matched = false;
            }
        }
    }
    out
}

/// `versions` joined with `" / "`, or `"unknown"`.
pub fn version_text(lock: &str, name: &str) -> String {
    let v = versions(lock, name);
    if v.is_empty() {
        "unknown".to_string()
    } else {
        v.join(" / ")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn hand_written_lock() {
        let lock = "version = 4\n\n[[package]]\nname = \"egui\"\nversion = \"0.33.3\"\n\n[[package]]\n\
                    name = \"egui-wgpu\"\nversion = \"0.33.3\"\n\n[[package]]\nname = \"winit\"\r\n\
                    version = \"0.29.0\"\r\n\n[[package]]\nname = \"winit\"\nversion = \"0.30.13\"\n";
        assert_eq!(versions(lock, "egui"), ["0.33.3"]);
        assert_eq!(version_text(lock, "winit"), "0.29.0 / 0.30.13");
        assert_eq!(version_text(lock, "wgpu"), "unknown");
    }
}
