//! Linux power-loss mode (REQ-CORE.STO.SQL.CRASH-004).
//!
//! The store lives on an ext4 file system on a device-mapper `flakey` target (the kernel's
//! `dm-flakey`). After each kill, [`PowerCut::cut`] switches the target to `drop_writes`
//! (suspended with `--nolockfs`, so the file system is not frozen and nothing is synced), then
//! unmounts: every write the kernel had not yet sent to the device, which is every write not
//! flushed by `fsync`/`fdatasync` or earlier write-back, is silently discarded. The target is
//! then switched back to pass writes through and the file system is mounted again (ext4
//! replays its journal), which is what the disk holds after a power cut. This is the recipe
//! of the xfstests `dmflakey` helpers [unverified that every runner kernel ships dm-flakey].
//!
//! Only the commands that need root (`dmsetup`, `mount`, `umount`, `sync`) run with
//! privileges, through `sudo -n`, each bounded by [`COMMAND_TIMEOUT`]; the harness and the
//! writer run as the invoking user on a mount it owns. Windows and macOS have no power-loss
//! mode (NODE.md, Open questions).

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The device-mapper target and the mount point the store lives on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerCut {
    /// Name of the `flakey` target (`/dev/mapper/<name>`).
    pub dm_name: String,
    /// The backing block device (for example a loop device).
    pub backing: PathBuf,
    /// Size of the backing device in 512-byte sectors.
    pub sectors: u64,
    /// Where `/dev/mapper/<name>` is mounted.
    pub mount: PathBuf,
}

/// Longest one privileged command may run before it is killed and the cut fails.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs `sudo -n <program> <args>` with a [`COMMAND_TIMEOUT`]. Only these commands run with
/// privileges; the harness itself runs as the invoking user.
fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let mut child = Command::new("sudo")
        .arg("-n")
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("sudo {program}: {e}"))?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < COMMAND_TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} {args:?}: no exit within {COMMAND_TIMEOUT:?}"));
            }
            Err(e) => return Err(format!("{program}: {e}")),
        }
    };
    if status.success() {
        return Ok(());
    }
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        // Bounded: the child has exited, so this reads what is left in the pipe.
        let _ = (&mut e).take(4096).read_to_string(&mut err);
    }
    Err(format!("{program} {args:?}: {status} {}", err.trim()))
}

impl PowerCut {
    /// The table that passes every I/O through, or (with `drop`) drops every write.
    pub fn table(&self, drop: bool) -> String {
        let dev = self.backing.display();
        if drop {
            format!("0 {} flakey {dev} 0 0 180 1 drop_writes", self.sectors)
        } else {
            format!("0 {} flakey {dev} 0 180 0", self.sectors)
        }
    }

    fn load(&self, drop: bool) -> Result<(), String> {
        run("dmsetup", &["suspend", "--nolockfs", &self.dm_name])?;
        let table = self.table(drop);
        let loaded = run("dmsetup", &["load", &self.dm_name, "--table", &table]);
        // Resume even if the load failed, so the device is never left suspended.
        let resumed = run("dmsetup", &["resume", &self.dm_name]);
        loaded.and(resumed)
    }

    fn mapper(&self) -> String {
        format!("/dev/mapper/{}", self.dm_name)
    }

    /// Flushes everything written so far (after a new store is created, so the cut tests the
    /// writer's commits and not the setup).
    pub fn settle(&self) -> Result<(), String> {
        run("sync", &[])
    }

    /// Discards every unflushed write and remounts.
    pub fn cut(&self) -> Result<(), String> {
        self.load(true)?;
        let mount = self.mount.display().to_string();
        let unmounted = run("umount", &[&mount]);
        let restored = self.load(false);
        unmounted.and(restored)?;
        run("mount", &[&self.mapper(), &mount])
    }
}
