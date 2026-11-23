//! Real-environment probes for Tier 1 OSes (Linux, macOS, Windows).
//!
//! Offline only (CONSTITUTION C7), zero `unsafe`, no panics, bounded reads, and never reads
//! user name, host name, machine serial numbers or MAC addresses (REQ-PLAT.DOCTOR.PROBE-003).

use super::{Arch, CpuInfo, Env, GpuSelection, OsInfo, Probed, RamInfo, UnknownReason, VolumeInfo, VolumeRole};
use std::path::PathBuf;
use std::time::Duration;

#[cfg(any(target_os = "linux", feature = "gpu"))]
use std::io::Read;
#[cfg(any(target_os = "linux", feature = "gpu"))]
use std::path::Path;

/// Largest `/proc` or `/etc` file read by a probe (1 MiB).
#[cfg(any(target_os = "linux", feature = "gpu"))]
const MAX_SYS_FILE_BYTES: usize = 1024 * 1024;

/// Real-machine [`Env`] implementation.
#[derive(Debug, Clone, Default)]
pub struct RealEnv {
    /// Directory on the install volume (only used to query free/total storage; never emitted).
    pub install_dir: Option<PathBuf>,
    /// Directory on the data volume (only used to query free/total storage; never emitted).
    pub data_dir: Option<PathBuf>,
    /// Optional path to `gpu-policy.toml` (used when compiled with `--features gpu`).
    pub gpu_policy_path: Option<PathBuf>,
    /// Optional path to `gpu-known-bad.toml` (used when compiled with `--features gpu`).
    pub gpu_known_bad_path: Option<PathBuf>,
}

impl RealEnv {
    pub fn new() -> Self {
        Self::default()
    }

    fn effective_install_dir(&self) -> PathBuf {
        if let Some(d) = &self.install_dir {
            return d.clone();
        }
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn effective_data_dir(&self) -> PathBuf {
        if let Some(d) = &self.data_dir {
            return d.clone();
        }
        std::env::temp_dir()
    }
}

impl Env for RealEnv {
    fn probe_os(&self) -> Probed<OsInfo> {
        probe_os_impl()
    }

    fn probe_arch(&self) -> Probed<Arch> {
        match Arch::parse(std::env::consts::ARCH) {
            Some(a) => Probed::Known(a),
            None => Probed::unknown(UnknownReason::Unsupported, "unrecognised architecture"),
        }
    }

    fn probe_cpu(&self) -> Probed<CpuInfo> {
        probe_cpu_impl()
    }

    fn probe_ram(&self) -> Probed<RamInfo> {
        probe_ram_impl()
    }

    fn probe_storage(&self) -> Probed<Vec<VolumeInfo>> {
        let install = self.effective_install_dir();
        let data = self.effective_data_dir();
        probe_storage_impl(&install, &data)
    }

    fn probe_display(&self) -> Probed<(u32, u32)> {
        probe_display_impl()
    }

    fn probe_gpu(&self, deadline: Duration) -> Probed<GpuSelection> {
        probe_gpu_impl(self, deadline)
    }
}

fn round_2dp(v: f64) -> f64 {
    if !v.is_finite() || v < 0.0 {
        0.0
    } else {
        (v * 100.0).round() / 100.0
    }
}

fn kib_to_gib(kib: u64) -> f64 {
    round_2dp((kib as f64) / (1024.0 * 1024.0))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn bytes_to_gib(bytes: u64) -> f64 {
    round_2dp((bytes as f64) / (1024.0 * 1024.0 * 1024.0))
}

/// Safe CPU instruction-set level detection using Rust stdlib feature macros.
pub(crate) fn detect_cpu_level() -> String {
    #[cfg(target_arch = "x86_64")]
    {
        let v2 = std::arch::is_x86_feature_detected!("cmpxchg16b")
            && std::arch::is_x86_feature_detected!("popcnt")
            && std::arch::is_x86_feature_detected!("sse3")
            && std::arch::is_x86_feature_detected!("ssse3")
            && std::arch::is_x86_feature_detected!("sse4.1")
            && std::arch::is_x86_feature_detected!("sse4.2");
        let v3 = v2
            && std::arch::is_x86_feature_detected!("avx")
            && std::arch::is_x86_feature_detected!("avx2")
            && std::arch::is_x86_feature_detected!("bmi1")
            && std::arch::is_x86_feature_detected!("bmi2")
            && std::arch::is_x86_feature_detected!("f16c")
            && std::arch::is_x86_feature_detected!("fma")
            && std::arch::is_x86_feature_detected!("lzcnt")
            && std::arch::is_x86_feature_detected!("movbe")
            && std::arch::is_x86_feature_detected!("xsave");
        let v4 = v3
            && std::arch::is_x86_feature_detected!("avx512f")
            && std::arch::is_x86_feature_detected!("avx512bw")
            && std::arch::is_x86_feature_detected!("avx512cd")
            && std::arch::is_x86_feature_detected!("avx512dq")
            && std::arch::is_x86_feature_detected!("avx512vl");
        if v4 {
            "x86-64-v4".to_string()
        } else if v3 {
            "x86-64-v3".to_string()
        } else if v2 {
            "x86-64-v2".to_string()
        } else {
            "x86-64-v1".to_string()
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("fp16") && std::arch::is_aarch64_feature_detected!("dotprod") {
            "armv8.2".to_string()
        } else {
            "armv8.0".to_string()
        }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        "unknown".to_string()
    }
}

#[cfg(any(target_os = "linux", feature = "gpu"))]
fn read_text_bounded(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    file.take(MAX_SYS_FILE_BYTES as u64 + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() > MAX_SYS_FILE_BYTES {
        return Err("system file exceeded size bound".to_string());
    }
    String::from_utf8(buf).map_err(|_| "system file is not UTF-8".to_string())
}

fn fallback_cores() -> Option<u32> {
    std::thread::available_parallelism().ok().and_then(|n| u32::try_from(n.get()).ok()).filter(|&n| n > 0)
}

// ------------------------------------------------------------------ Linux probes

#[cfg(target_os = "linux")]
fn probe_os_impl() -> Probed<OsInfo> {
    let text = match read_text_bounded(Path::new("/etc/os-release")) {
        Ok(t) => t,
        Err(e) => return Probed::unknown(UnknownReason::Error, format!("cannot read os-release: {e}")),
    };
    let mut id: Option<String> = None;
    let mut version: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("ID=") {
            id = Some(unquote_shell(rest).to_ascii_lowercase());
        } else if let Some(rest) = trimmed.strip_prefix("VERSION_ID=") {
            version = Some(unquote_shell(rest));
        }
    }
    let family = id.filter(|s| !s.is_empty()).unwrap_or_else(|| "linux".to_string());
    match version.filter(|s| !s.is_empty()) {
        Some(v) => Probed::Known(OsInfo { family, version: v }),
        None => Probed::unknown(UnknownReason::Error, "VERSION_ID missing in os-release"),
    }
}

#[cfg(target_os = "linux")]
fn unquote_shell(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 {
        if let Some(inner) = t.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
            return inner.to_string();
        }
        if let Some(inner) = t.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
            return inner.to_string();
        }
    }
    t.to_string()
}

#[cfg(target_os = "linux")]
fn probe_cpu_impl() -> Probed<CpuInfo> {
    use std::collections::BTreeSet;
    let text = read_text_bounded(Path::new("/proc/cpuinfo")).unwrap_or_default();
    let mut model: Option<String> = None;
    let mut pairs: BTreeSet<(String, String)> = BTreeSet::new();
    let mut cur_phys: Option<String> = None;
    let mut cur_core: Option<String> = None;
    let mut cpu_cores_field: Option<u32> = None;

    let flush = |phys: &mut Option<String>, core: &mut Option<String>, set: &mut BTreeSet<(String, String)>| {
        if let (Some(p), Some(c)) = (phys.take(), core.take()) {
            set.insert((p, c));
        } else {
            *phys = None;
            *core = None;
        }
    };

    for line in text.lines() {
        if line.trim().is_empty() {
            flush(&mut cur_phys, &mut cur_core, &mut pairs);
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let key = k.trim();
        let val = v.trim();
        match key {
            "model name" | "Model" | "Hardware" if model.is_none() && !val.is_empty() => {
                model = Some(val.to_string());
            }
            "physical id" => cur_phys = Some(val.to_string()),
            "core id" => cur_core = Some(val.to_string()),
            "cpu cores" if cpu_cores_field.is_none() => {
                cpu_cores_field = val.parse::<u32>().ok().filter(|&n| n > 0);
            }
            _ => {}
        }
    }
    flush(&mut cur_phys, &mut cur_core, &mut pairs);

    let physical_cores =
        if !pairs.is_empty() { u32::try_from(pairs.len()).ok() } else { cpu_cores_field.or_else(fallback_cores) };
    let Some(cores) = physical_cores.filter(|&n| n > 0) else {
        return Probed::unknown(UnknownReason::Error, "could not determine physical core count");
    };
    Probed::Known(CpuInfo {
        model: model.unwrap_or_else(|| "unknown".to_string()),
        physical_cores: cores,
        instruction_set_level: detect_cpu_level(),
    })
}

#[cfg(target_os = "linux")]
fn probe_ram_impl() -> Probed<RamInfo> {
    let text = match read_text_bounded(Path::new("/proc/meminfo")) {
        Ok(t) => t,
        Err(e) => return Probed::unknown(UnknownReason::Error, format!("cannot read meminfo: {e}")),
    };
    let mut total_kib: Option<u64> = None;
    let mut avail_kib: Option<u64> = None;
    let mut free_kib: Option<u64> = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let num = v.split_whitespace().next().and_then(|s| s.parse::<u64>().ok());
        match k.trim() {
            "MemTotal" => total_kib = num,
            "MemAvailable" => avail_kib = num,
            "MemFree" => free_kib = num,
            _ => {}
        }
    }
    let Some(total) = total_kib else {
        return Probed::unknown(UnknownReason::Error, "MemTotal missing in meminfo");
    };
    let avail = avail_kib.or(free_kib).unwrap_or(0);
    Probed::Known(RamInfo { total_gib: kib_to_gib(total), available_gib: kib_to_gib(avail) })
}

#[cfg(target_os = "linux")]
fn probe_display_impl() -> Probed<(u32, u32)> {
    let has_x11 = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
    let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    if !has_x11 && !has_wayland {
        return Probed::unknown(UnknownReason::NoDisplay, "no DISPLAY or WAYLAND_DISPLAY set");
    }
    let out = match std::process::Command::new("xrandr").arg("--current").output() {
        Ok(o) => o,
        Err(_) => return Probed::unknown(UnknownReason::NoDisplay, "display query tool unavailable"),
    };
    if !out.status.success() {
        return Probed::unknown(UnknownReason::NoDisplay, "cannot open display");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    match parse_xrandr_resolution(&text) {
        Some(res) => Probed::Known(res),
        None => Probed::unknown(UnknownReason::NoDisplay, "no active display mode found"),
    }
}

#[cfg(target_os = "linux")]
fn parse_xrandr_resolution(text: &str) -> Option<(u32, u32)> {
    for line in text.lines() {
        if !line.contains('*') {
            continue;
        }
        let first = line.split_whitespace().next()?;
        let (w_s, h_s) = first.split_once('x')?;
        let w = w_s.parse::<u32>().ok()?;
        let h = h_s.parse::<u32>().ok()?;
        if w > 0 && h > 0 {
            return Some((w, h));
        }
    }
    None
}

// ------------------------------------------------------------------ macOS probes

#[cfg(target_os = "macos")]
fn run_stdout(prog: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new(prog).args(args).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("{prog} exited non-zero"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn probe_os_impl() -> Probed<OsInfo> {
    match run_stdout("sw_vers", &["-productVersion"]) {
        Ok(v) if !v.is_empty() => Probed::Known(OsInfo { family: "macos".to_string(), version: v }),
        Ok(_) => Probed::unknown(UnknownReason::Error, "empty macOS version"),
        Err(e) => Probed::unknown(UnknownReason::Error, e),
    }
}

#[cfg(target_os = "macos")]
fn probe_cpu_impl() -> Probed<CpuInfo> {
    let model = run_stdout("sysctl", &["-n", "machdep.cpu.brand_string"])
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Apple Silicon".to_string());
    let cores = run_stdout("sysctl", &["-n", "hw.physicalcpu"])
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&n| n > 0)
        .or_else(fallback_cores);
    let Some(physical_cores) = cores else {
        return Probed::unknown(UnknownReason::Error, "could not determine physical core count");
    };
    Probed::Known(CpuInfo { model, physical_cores, instruction_set_level: detect_cpu_level() })
}

#[cfg(target_os = "macos")]
fn probe_ram_impl() -> Probed<RamInfo> {
    let total_bytes = match run_stdout("sysctl", &["-n", "hw.memsize"]).ok().and_then(|s| s.parse::<u64>().ok()) {
        Some(b) => b,
        None => return Probed::unknown(UnknownReason::Error, "could not read hw.memsize"),
    };
    let page_size =
        run_stdout("sysctl", &["-n", "hw.pagesize"]).ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(4096);
    let mut avail_pages: u64 = 0;
    if let Ok(vm) = run_stdout("vm_stat", &[]) {
        for line in vm.lines() {
            if line.starts_with("Pages free:")
                || line.starts_with("Pages inactive:")
                || line.starts_with("Pages speculative:")
            {
                if let Some((_, val)) = line.split_once(':') {
                    let digits: String = val.chars().filter(char::is_ascii_digit).collect();
                    if let Ok(p) = digits.parse::<u64>() {
                        avail_pages = avail_pages.saturating_add(p);
                    }
                }
            }
        }
    }
    let avail_bytes = avail_pages.saturating_mul(page_size).min(total_bytes);
    Probed::Known(RamInfo { total_gib: bytes_to_gib(total_bytes), available_gib: bytes_to_gib(avail_bytes) })
}

#[cfg(target_os = "macos")]
fn probe_display_impl() -> Probed<(u32, u32)> {
    let text = match run_stdout("system_profiler", &["SPDisplaysDataType"]) {
        Ok(t) => t,
        Err(_) => return Probed::unknown(UnknownReason::NoDisplay, "display profiler unavailable"),
    };
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Resolution:") {
            let nums: Vec<u32> = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<u32>().ok())
                .collect();
            if nums.len() >= 2 && nums[0] > 0 && nums[1] > 0 {
                return Probed::Known((nums[0], nums[1]));
            }
        }
    }
    Probed::unknown(UnknownReason::NoDisplay, "no active display found")
}

// ------------------------------------------------------------------ Windows probes

#[cfg(target_os = "windows")]
fn run_powershell(script: &str) -> Result<String, String> {
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("powershell command failed".to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "windows")]
fn probe_os_impl() -> Probed<OsInfo> {
    if let Ok(out) = std::process::Command::new("cmd").args(["/C", "ver"]).output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(ver) = parse_windows_ver(&text) {
                return Probed::Known(OsInfo { family: "windows".to_string(), version: ver });
            }
        }
    }
    match run_powershell("[System.Environment]::OSVersion.Version.ToString()") {
        Ok(v) if !v.is_empty() => {
            let three = v.split('.').take(3).collect::<Vec<_>>().join(".");
            Probed::Known(OsInfo { family: "windows".to_string(), version: three })
        }
        Ok(_) => Probed::unknown(UnknownReason::Error, "empty Windows version"),
        Err(e) => Probed::unknown(UnknownReason::Error, e),
    }
}

#[cfg(target_os = "windows")]
fn parse_windows_ver(text: &str) -> Option<String> {
    for tok in text.split(|c: char| c.is_whitespace() || matches!(c, '[' | ']')) {
        let parts: Vec<&str> = tok.split('.').collect();
        if parts.len() >= 3 && parts.iter().take(3).all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
            return Some(format!("{}.{}.{}", parts[0], parts[1], parts[2]));
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn probe_cpu_impl() -> Probed<CpuInfo> {
    // Avoid spawning two slow PowerShell/CIM processes that can exceed DEFAULT_DEADLINE
    // on CI runners: query the registry or PROCESSOR_IDENTIFIER and use available_parallelism.
    let model = run_stdout(
        "reg",
        &["query", r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0", "/v", "ProcessorNameString"],
    )
    .ok()
    .and_then(|out| {
        out.lines()
            .find(|l| l.contains("ProcessorNameString"))
            .and_then(|l| l.split("REG_SZ").nth(1))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    })
    .or_else(|| std::env::var("PROCESSOR_IDENTIFIER").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
    .unwrap_or_else(|| "unknown".to_string());
    let Some(physical_cores) = fallback_cores() else {
        return Probed::unknown(UnknownReason::Error, "could not determine physical core count");
    };
    Probed::Known(CpuInfo { model, physical_cores, instruction_set_level: detect_cpu_level() })
}

#[cfg(target_os = "windows")]
fn probe_ram_impl() -> Probed<RamInfo> {
    let script = "$o=Get-CimInstance Win32_OperatingSystem; \"$($o.TotalVisibleMemorySize) $($o.FreePhysicalMemory)\"";
    let text = match run_powershell(script) {
        Ok(t) => t,
        Err(e) => return Probed::unknown(UnknownReason::Error, e),
    };
    let mut parts = text.split_whitespace();
    let total = parts.next().and_then(|s| s.parse::<u64>().ok());
    let free = parts.next().and_then(|s| s.parse::<u64>().ok());
    match (total, free) {
        (Some(t), Some(f)) => Probed::Known(RamInfo { total_gib: kib_to_gib(t), available_gib: kib_to_gib(f) }),
        _ => Probed::unknown(UnknownReason::Error, "unexpected RAM probe output"),
    }
}

#[cfg(target_os = "windows")]
fn probe_display_impl() -> Probed<(u32, u32)> {
    let script =
        "Add-Type -AssemblyName System.Windows.Forms; $b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \"$($b.Width)x$($b.Height)\"";
    let text = match run_powershell(script) {
        Ok(t) => t,
        Err(_) => return Probed::unknown(UnknownReason::NoDisplay, "no primary display available"),
    };
    let Some((w_s, h_s)) = text.split_once('x') else {
        return Probed::unknown(UnknownReason::NoDisplay, "no primary display resolution");
    };
    match (w_s.parse::<u32>().ok(), h_s.parse::<u32>().ok()) {
        (Some(w), Some(h)) if w > 0 && h > 0 => Probed::Known((w, h)),
        _ => Probed::unknown(UnknownReason::NoDisplay, "no active display"),
    }
}

// ------------------------------------------------------------------ Other OS fallback

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn probe_os_impl() -> Probed<OsInfo> {
    Probed::unknown(UnknownReason::Unsupported, "OS version probe unsupported on this platform")
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn probe_cpu_impl() -> Probed<CpuInfo> {
    match fallback_cores() {
        Some(cores) => Probed::Known(CpuInfo {
            model: "unknown".to_string(),
            physical_cores: cores,
            instruction_set_level: detect_cpu_level(),
        }),
        None => Probed::unknown(UnknownReason::Unsupported, "CPU probe unsupported on this platform"),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn probe_ram_impl() -> Probed<RamInfo> {
    Probed::unknown(UnknownReason::Unsupported, "RAM probe unsupported on this platform")
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn probe_display_impl() -> Probed<(u32, u32)> {
    Probed::unknown(UnknownReason::Unsupported, "display probe unsupported on this platform")
}

// ------------------------------------------------------------------ Storage probe (no paths emitted)

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn df_volume(role: VolumeRole, dir: &std::path::Path) -> Result<VolumeInfo, String> {
    let out = std::process::Command::new("df")
        .args(["-k", "-P"])
        .arg(dir)
        .output()
        .map_err(|_| "df unavailable".to_string())?;
    if !out.status.success() {
        return Err("df failed".to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let data_line = text.lines().nth(1).ok_or_else(|| "unexpected df output".to_string())?;
    let cols: Vec<&str> = data_line.split_whitespace().collect();
    if cols.len() < 4 {
        return Err("unexpected df columns".to_string());
    }
    let total_kib = cols[1].parse::<u64>().map_err(|_| "invalid df total".to_string())?;
    let avail_kib = cols[3].parse::<u64>().map_err(|_| "invalid df available".to_string())?;
    Ok(VolumeInfo { role, free_gib: kib_to_gib(avail_kib), total_gib: kib_to_gib(total_kib) })
}

#[cfg(target_os = "windows")]
fn df_volume(role: VolumeRole, dir: &std::path::Path) -> Result<VolumeInfo, String> {
    let dir_s = dir.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$r=(Get-Item -LiteralPath '{dir_s}').Root.Name; $d=[System.IO.DriveInfo]::new($r); \"$($d.TotalSize) $($d.AvailableFreeSpace)\""
    );
    let text = run_powershell(&script)?;
    let mut parts = text.split_whitespace();
    let total = parts.next().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| "invalid total".to_string())?;
    let free = parts.next().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| "invalid free".to_string())?;
    Ok(VolumeInfo { role, free_gib: bytes_to_gib(free), total_gib: bytes_to_gib(total) })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn df_volume(_role: VolumeRole, _dir: &std::path::Path) -> Result<VolumeInfo, String> {
    Err("storage probe unsupported".to_string())
}

fn probe_storage_impl(install_dir: &std::path::Path, data_dir: &std::path::Path) -> Probed<Vec<VolumeInfo>> {
    let mut vols = Vec::new();
    let mut last_err: Option<String> = None;
    for (role, dir) in [(VolumeRole::Install, install_dir), (VolumeRole::Data, data_dir)] {
        match df_volume(role, dir) {
            Ok(v) => vols.push(v),
            Err(e) => last_err = Some(e),
        }
    }
    if vols.is_empty() {
        Probed::unknown(UnknownReason::Error, last_err.unwrap_or_else(|| "storage probe failed".to_string()))
    } else {
        Probed::Known(vols)
    }
}

// ------------------------------------------------------------------ GPU probe

#[cfg(not(feature = "gpu"))]
fn probe_gpu_impl(_env: &RealEnv, _deadline: Duration) -> Probed<GpuSelection> {
    Probed::unknown(UnknownReason::NotBuilt, "built without the gpu feature")
}

#[cfg(feature = "gpu")]
fn probe_gpu_impl(env: &RealEnv, deadline: Duration) -> Probed<GpuSelection> {
    use fid_gpu_policy::policy::{
        known_bad_or_empty, validate_known_bad, validate_policy, Backend, GpuOs, KnownBad, PolicyRow,
    };
    use fid_gpu_policy::Arch as PolicyArch;
    use std::collections::BTreeMap;

    let gpu_os = if cfg!(target_os = "windows") {
        GpuOs::Windows
    } else if cfg!(target_os = "macos") {
        GpuOs::Macos
    } else if cfg!(target_os = "linux") {
        GpuOs::Linux
    } else {
        return Probed::unknown(UnknownReason::Unsupported, "GPU policy unsupported on this OS");
    };
    let policy_arch = match std::env::consts::ARCH {
        "x86_64" => PolicyArch::X86_64,
        "aarch64" => PolicyArch::Aarch64,
        _ => return Probed::unknown(UnknownReason::Unsupported, "GPU policy unsupported on this arch"),
    };

    let row = env
        .gpu_policy_path
        .as_deref()
        .and_then(|p| read_text_bounded(p).ok())
        .and_then(|t| validate_policy(t.as_bytes()).value)
        .and_then(|pol| pol.row(gpu_os, policy_arch).cloned())
        .unwrap_or_else(|| PolicyRow {
            os: gpu_os,
            arch: policy_arch,
            backends: vec![Backend::Software],
            min: BTreeMap::new(),
        });

    let known_bad: KnownBad = env
        .gpu_known_bad_path
        .as_deref()
        .and_then(|p| read_text_bounded(p).ok())
        .map(|t| known_bad_or_empty(validate_known_bad(t.as_bytes())).0)
        .unwrap_or(KnownBad { schema_version: 1, entries: Vec::new() });

    let report = fid_gpu_select::Probe::wgpu().run(&row, deadline);
    let sel = fid_gpu_select::select(&report.adapters, &report.rejections, &row, &known_bad, None);
    Probed::Known(GpuSelection::from(&sel))
}
