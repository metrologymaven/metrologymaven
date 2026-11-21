//! Adapter probing (REQ-PLAT.GPU.DETECT-005): each hardware backend of the policy row is
//! enumerated on its own worker thread; the caller waits until one deadline for all of them.
//! A backend that fails is a `probe_failed` rejection, one that has not answered by the
//! deadline a `probe_timeout` rejection (its thread is left to finish on its own and its
//! answer is dropped); nothing here returns an error or panics on the caller's thread.

use crate::{AdapterInfo, Cause, DeviceType, Rejection};
use fid_gpu_policy::policy::{Backend, PolicyRow};
use fid_gpu_policy::Version;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The probe deadline of REQ-005 [unverified: to be tuned].
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Lists the adapters of one backend. Implementations may block; [`probe`] bounds them.
pub trait Prober: Send + Sync + 'static {
    fn enumerate(&self, backend: Backend) -> Result<Vec<AdapterInfo>, String>;
}

/// What [`probe`] observed: the adapters, and one rejection per backend that failed or timed
/// out. It is the recorded input of [`crate::select`], so a selection can be replayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub adapters: Vec<AdapterInfo>,
    pub rejections: Vec<Rejection>,
}

/// Probes every hardware backend of `row` through `prober`, in parallel, with one deadline of
/// `timeout` from the call. Adapters come back in policy backend order, each backend's in the
/// order its probe listed them.
pub fn probe<P: Prober>(prober: Arc<P>, row: &PolicyRow, timeout: Duration) -> ProbeReport {
    // `None`: a timeout too large to represent waits without a deadline (no overflow panic).
    let deadline = Instant::now().checked_add(timeout);
    let mut pending = Vec::new();
    for &b in row.backends.iter().filter(|b| **b != Backend::Software) {
        let (tx, rx) = mpsc::channel();
        let p = Arc::clone(&prober);
        let spawned = std::thread::Builder::new().name(format!("gpu-probe-{b}")).spawn(move || {
            let _ = tx.send(p.enumerate(b));
        });
        pending.push((b, spawned.map(|_| rx).map_err(|e| e.to_string())));
    }
    let mut report = ProbeReport { adapters: Vec::new(), rejections: Vec::new() };
    for (b, rx) in pending {
        let reject = |cause| Rejection { adapter: None, backend: b, cause };
        let rx = match rx {
            Ok(rx) => rx,
            Err(e) => {
                report.rejections.push(reject(Cause::ProbeFailed(format!("cannot start the probe thread: {e}"))));
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
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                report.rejections.push(reject(Cause::ProbeFailed("the probe thread stopped without an answer".into())))
            }
        }
    }
    report
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

/// The first dotted number of a driver string (`"31.0.15.4601"`, `"Mesa 24.0.5"`), as a
/// [`Version`]; `None` when there is none.
pub fn driver_version_of(s: &str) -> Option<Version> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|t| t.trim_matches('.'))
        .filter(|t| t.contains('.'))
        .find_map(Version::parse)
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
                    driver_version: driver_version_of(&i.driver_info).or_else(|| driver_version_of(&i.driver)),
                    api_version: None,
                }
            })
            .collect())
    }
}
