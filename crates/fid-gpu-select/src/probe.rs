//! Adapter probing (REQ-PLAT.GPU.DETECT-005): each hardware backend of the policy row is
//! enumerated on its own worker thread; the caller waits until one deadline for all of them.
//! A backend that fails is a `probe_failed` rejection, one that has not answered by the
//! deadline a `probe_timeout` rejection; nothing here returns an error or panics on the
//! caller's thread.
//!
//! A hung driver is contained: a [`Probe`] keeps one in-flight flag per backend, set while a
//! worker thread is inside the graphics layer. A backend whose earlier probe has still not
//! returned is reported `probe_timeout` at once, and no new thread is started for it, so a
//! driver that hangs for good costs one thread, not one per probe. Use the process-wide
//! [`Probe::wgpu`] for the real driver so every caller shares those flags.

use crate::{AdapterInfo, Cause, DeviceType, Rejection};
use fid_gpu_policy::policy::{Backend, PolicyRow};
use fid_gpu_policy::Version;
use std::collections::BTreeSet;
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// The probe deadline of REQ-005 [unverified: to be tuned].
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Lists the adapters of one backend. Implementations may block; [`Probe`] bounds them.
pub trait Prober: Send + Sync + 'static {
    fn enumerate(&self, backend: Backend) -> Result<Vec<AdapterInfo>, String>;
}

/// What a probe observed: the adapters, and one rejection per backend that failed or timed
/// out. It is the recorded input of [`crate::select`], so a selection can be replayed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProbeReport {
    pub adapters: Vec<AdapterInfo>,
    pub rejections: Vec<Rejection>,
}

impl ProbeReport {
    /// The backends whose probe did not answer by the deadline (or was still hung from an
    /// earlier probe), in policy order.
    pub fn timed_out(&self) -> Vec<Backend> {
        crate::probe_backends(&self.rejections, |c| matches!(c, Cause::ProbeTimeout))
    }

    /// The backends whose probe failed (an error, a panic, or a thread that could not start).
    pub fn failed(&self) -> Vec<Backend> {
        crate::probe_backends(&self.rejections, |c| matches!(c, Cause::ProbeFailed(_)))
    }
}

type InFlightSet = Arc<Mutex<BTreeSet<Backend>>>;

fn lock(set: &InFlightSet) -> MutexGuard<'_, BTreeSet<Backend>> {
    // The set holds no invariant a panic could break half-way; a poisoned lock is still usable.
    set.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Clears a backend's in-flight flag when its worker leaves the graphics layer, by return or
/// by panic.
struct InFlight {
    set: InFlightSet,
    backend: Backend,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        lock(&self.set).remove(&self.backend);
    }
}

enum Pending {
    Answer(mpsc::Receiver<Result<Vec<AdapterInfo>, String>>),
    NotStarted(String),
    StillHung,
}

/// A prober with its per-backend in-flight flags. Keep one per prober for the life of the
/// process (for wgpu, [`Probe::wgpu`]); a fresh `Probe` knows nothing of earlier hung probes.
pub struct Probe<P: Prober> {
    prober: Arc<P>,
    in_flight: InFlightSet,
}

impl<P: Prober> Probe<P> {
    pub fn new(prober: P) -> Probe<P> {
        Probe { prober: Arc::new(prober), in_flight: Arc::default() }
    }

    /// The backends whose probe thread is still inside the graphics layer.
    pub fn in_flight(&self) -> Vec<Backend> {
        lock(&self.in_flight).iter().copied().collect()
    }

    /// Probes every hardware backend of `row`, in parallel, with one deadline of `timeout`
    /// from the call. Adapters come back in policy backend order, each backend's in the order
    /// its probe listed them. A backend still in flight from an earlier call is `probe_timeout`
    /// at once and is not probed again.
    pub fn run(&self, row: &PolicyRow, timeout: Duration) -> ProbeReport {
        // `None`: a timeout too large to represent waits without a deadline (no overflow panic).
        let deadline = Instant::now().checked_add(timeout);
        let mut pending = Vec::new();
        for &b in row.backends.iter().filter(|b| **b != Backend::Software) {
            if !lock(&self.in_flight).insert(b) {
                pending.push((b, Pending::StillHung));
                continue;
            }
            let guard = InFlight { set: Arc::clone(&self.in_flight), backend: b };
            let (tx, rx) = mpsc::channel();
            let p = Arc::clone(&self.prober);
            // If the spawn fails, the closure and its guard are dropped, which clears the flag.
            let spawned = std::thread::Builder::new().name(format!("gpu-probe-{b}")).spawn(move || {
                // The flag is cleared before the answer is sent, so a caller that has the
                // answer never sees the backend as still in flight.
                let answer = {
                    let _guard = guard;
                    p.enumerate(b)
                };
                let _ = tx.send(answer);
            });
            pending.push((
                b,
                match spawned {
                    Ok(_) => Pending::Answer(rx),
                    Err(e) => Pending::NotStarted(e.to_string()),
                },
            ));
        }
        let mut report = ProbeReport::default();
        for (b, p) in pending {
            let reject = |cause| Rejection { adapter: None, backend: b, cause };
            let rx = match p {
                Pending::Answer(rx) => rx,
                Pending::NotStarted(e) => {
                    report.rejections.push(reject(Cause::ProbeFailed(format!("cannot start the probe thread: {e}"))));
                    continue;
                }
                Pending::StillHung => {
                    report.rejections.push(reject(Cause::ProbeTimeout));
                    continue;
                }
            };
            let answer = match deadline {
                Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            match answer {
                Ok(Ok(list)) => report.adapters.extend(list.into_iter().filter(|a| a.backend == b)),
                Ok(Err(e)) => report.rejections.push(reject(Cause::ProbeFailed(e))),
                Err(mpsc::RecvTimeoutError::Timeout) => report.rejections.push(reject(Cause::ProbeTimeout)),
                // The thread ended without an answer (it panicked inside the graphics layer).
                Err(mpsc::RecvTimeoutError::Disconnected) => report
                    .rejections
                    .push(reject(Cause::ProbeFailed("the probe thread stopped without an answer".into()))),
            }
        }
        report
    }

    /// [`Probe::run`], except that an allowed `software` override probes nothing: a user whose
    /// GPU driver crashes or hangs can start without touching it (REQ-004).
    pub fn run_with_override(
        &self,
        row: &PolicyRow,
        override_backend: Option<Backend>,
        timeout: Duration,
    ) -> ProbeReport {
        if override_backend == Some(Backend::Software) && row.backends.contains(&Backend::Software) {
            return ProbeReport::default();
        }
        self.run(row, timeout)
    }
}

impl Probe<WgpuProber> {
    /// The process-wide probe of the real driver, so every caller shares its in-flight flags.
    pub fn wgpu() -> &'static Probe<WgpuProber> {
        static WGPU: OnceLock<Probe<WgpuProber>> = OnceLock::new();
        WGPU.get_or_init(|| Probe::new(WgpuProber))
    }
}

/// The real prober: wgpu adapters of one backend, enumerated without a surface.
#[derive(Debug, Clone, Copy, Default)]
pub struct WgpuProber;

/// The wgpu backend probed for a policy backend (`None`: not probed through wgpu).
fn wgpu_backend(b: Backend) -> Option<wgpu::Backends> {
    match b {
        Backend::Vulkan => Some(wgpu::Backends::VULKAN),
        Backend::Metal => Some(wgpu::Backends::METAL),
        Backend::Dx12 => Some(wgpu::Backends::DX12),
        // `software` is the fallback, never probed; backends added later are not probed until
        // they are mapped here.
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

fn device_type(t: wgpu::DeviceType) -> DeviceType {
    match t {
        wgpu::DeviceType::DiscreteGpu => DeviceType::Discrete,
        wgpu::DeviceType::IntegratedGpu => DeviceType::Integrated,
        wgpu::DeviceType::VirtualGpu => DeviceType::Virtual,
        wgpu::DeviceType::Cpu => DeviceType::Cpu,
        wgpu::DeviceType::Other => DeviceType::Other,
    }
}

/// The lowest API version wgpu 27 guarantees for an adapter it lists (a proxy; wgpu never
/// reports the version itself):
/// - DX12: wgpu-hal creates the device at `D3D_FEATURE_LEVEL_11_0` and hides an adapter that
///   cannot, so 11.0; it sets `VERTEX_WRITABLE_STORAGE` only from feature level 11.1, so 11.1
///   when that feature is present. Feature level 12.x is not visible.
/// - Vulkan: 1.0 (wgpu lists 1.0 devices); nothing in the downlevel flags, limits or features
///   tells 1.0 from 1.1 reliably.
/// - Anything else: unknown.
fn api_floor(b: Backend, features: wgpu::Features) -> Option<Version> {
    match b {
        Backend::Dx12 if features.contains(wgpu::Features::VERTEX_WRITABLE_STORAGE) => Version::parse("11.1"),
        Backend::Dx12 => Version::parse("11.0"),
        Backend::Vulkan => Version::parse("1.0"),
        _ => None,
    }
}

/// The driver version in a driver string: the first dotted numeric token only (`"31.0.15.4601"`,
/// `"Mesa 24.0.5"`, and `"4.6.0"` from `"4.6.0 NVIDIA 560.94"`). `None` (unknown, the safe
/// side) when there is no dotted token, when the first one touches a letter (AMDVLK's
/// `"2024.Q2.1"`), or when it is not a [`Version`] (`"24.0.5.1.2"` has five components). A
/// later token is never used in its place (`"(LLVM 17.0.6)"` is not the Mesa version).
pub fn driver_version_of(s: &str) -> Option<Version> {
    let c: Vec<char> = s.chars().collect();
    let numeric = |ch: char| ch.is_ascii_digit() || ch == '.';
    let mut i = 0;
    while i < c.len() {
        if !numeric(c[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < c.len() && numeric(c[i]) {
            i += 1;
        }
        let token: String = c[start..i].iter().collect();
        let body = token.trim_end_matches('.');
        if !body.contains('.') || !body.bytes().any(|b| b.is_ascii_digit()) {
            continue;
        }
        let before = start.checked_sub(1).and_then(|j| c.get(j)).copied();
        let after = c.get(i).copied();
        if before.is_some_and(|ch| ch.is_alphabetic()) || after.is_some_and(|ch| ch.is_alphabetic()) {
            return None;
        }
        return Version::parse(body);
    }
    None
}

impl Prober for WgpuProber {
    fn enumerate(&self, backend: Backend) -> Result<Vec<AdapterInfo>, String> {
        let Some(wanted) = wgpu_backend(backend) else {
            return Err(format!("{backend} is not probed through wgpu"));
        };
        if !wgpu::Instance::enabled_backend_features().contains(wanted) {
            return Err(format!("{backend} is not compiled into this build"));
        }
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wanted, ..Default::default() });
        Ok(instance
            .enumerate_adapters(wanted)
            .iter()
            .map(|a| {
                let i = a.get_info();
                AdapterInfo {
                    backend,
                    name: i.name.clone(),
                    vendor_id: i.vendor,
                    device_id: i.device,
                    device_type: device_type(i.device_type),
                    // `driver_info` is the version string where the driver gives one; `driver`
                    // is read only when it is empty, never in place of one that did not parse.
                    driver_version: if i.driver_info.trim().is_empty() {
                        driver_version_of(&i.driver)
                    } else {
                        driver_version_of(&i.driver_info)
                    },
                    api_version: None,
                    api_floor: api_floor(backend, a.features()),
                }
            })
            .collect())
    }
}
