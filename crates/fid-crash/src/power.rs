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
//! The commands (`dmsetup`, `mount`, `umount`, `sync`) need root; the CI job runs the harness
//! with `sudo`. Windows and macOS have no power-loss mode (stated in the PR and NODE.md).

use std::path::PathBuf;
use std::process::Command;

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

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{program} {args:?}: {} {}", out.status, String::from_utf8_lossy(&out.stderr).trim()))
    }
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
