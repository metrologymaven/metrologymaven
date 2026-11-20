//! The writer child and the killing parent (REQ-CORE.STO.SQL.CRASH-001 .. -005).
//!
//! Protocol on the writer's stdout, one line each: `READY <next seq>` once the store is open,
//! `ACK <seq>` after each commit returned success (flushed before the next commit starts),
//! optional adapter lines, and `DONE` if the writer reaches its commit budget. The parent waits
//! for `READY` (every fourth kill: only for the spawn), sleeps the seeded delay of the kill, kills the writer (`SIGKILL` on Unix,
//! `TerminateProcess` on Windows, both through `std::process::Child::kill`), reaps it, reads
//! every `ACK` it wrote before dying, optionally cuts power, and checks the reopened store.

use crate::adapter::{Ack, CrashAdapter};
use crate::failure::{CrashFailure, FailureKind};
use crate::plan::{KillInstant, KillPlan};
use crate::power::PowerCut;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Longest wait for a line from a writer, and for its stdout to close after the kill.
pub const WATCHDOG: Duration = Duration::from_secs(120);
/// Most lines kept from one writer for a failure report.
const KEPT_LINES: usize = 50;
/// Longest a writer that reached its commit budget waits to be killed.
const DONE_WAIT: Duration = Duration::from_secs(600);

/// Writer side: opens the store, prints `READY`, then commits `next, next + 1, ...` and
/// acknowledges each after `commit_next` returned, for at most `max_commits` commits; then
/// prints `DONE` and waits (bounded) to be killed. `after_ack` may print extra lines.
pub fn run_writer(
    adapter: &mut dyn CrashAdapter,
    root: &Path,
    max_commits: u64,
    out: &mut dyn Write,
    after_ack: &mut dyn FnMut(u64, &mut dyn Write) -> std::io::Result<()>,
) -> Result<(), String> {
    let next = adapter.open_writer(root)?;
    let io = |e: std::io::Error| format!("stdout: {e}");
    writeln!(out, "READY {next}").map_err(io)?;
    out.flush().map_err(io)?;
    let mut ack = Ack::new(out);
    for seq in (next..).take(usize::try_from(max_commits).unwrap_or(usize::MAX)) {
        adapter.commit_next(seq, &mut ack)?;
        if ack.last() != Some(seq) {
            ack.send(seq).map_err(io)?;
        }
        after_ack(seq, ack.out()).map_err(io)?;
    }
    let out = ack.out();
    writeln!(out, "DONE").map_err(io)?;
    out.flush().map_err(io)?;
    let start = Instant::now();
    while start.elapsed() < DONE_WAIT {
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// A running writer whose stdout lines arrive through a channel, so every wait is bounded.
/// Dropping it kills and reaps the process.
pub struct Writer {
    child: std::process::Child,
    lines: Receiver<String>,
    seen: Vec<String>,
    acks: Vec<u64>,
}

impl Writer {
    /// Starts `program args... --root <root>` with a piped stdout.
    pub fn spawn(program: &[OsString], root: &Path) -> Result<Writer, String> {
        let (exe, args) = program.split_first().ok_or("empty writer command")?;
        let mut child = Command::new(exe)
            .args(args)
            .arg("--root")
            .arg(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("spawn writer: {e}"))?;
        let out = child.stdout.take().ok_or("writer has no stdout")?;
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Writer { child, lines, seen: Vec::new(), acks: Vec::new() })
    }

    /// The process id.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    fn note(&mut self, line: &str) {
        if let Some(rest) = line.strip_prefix("ACK ") {
            // A torn line cannot occur (each line is one small write), and an unparsable one is
            // never counted as an acknowledgement.
            if let Ok(seq) = rest.trim().parse() {
                self.acks.push(seq);
            }
        }
        if self.seen.len() < KEPT_LINES {
            self.seen.push(line.to_string());
        }
    }

    /// Reads lines until one starts with `prefix` and returns it.
    pub fn wait_for(&mut self, prefix: &str) -> Result<String, String> {
        loop {
            match self.lines.recv_timeout(WATCHDOG) {
                Ok(line) => {
                    self.note(&line);
                    if line.starts_with(prefix) {
                        return Ok(line);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("no {prefix} from the writer within {WATCHDOG:?}; it printed {:?}", self.seen))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let status = self.child.wait().ok();
                    return Err(format!("writer exited ({status:?}) before {prefix}; it printed {:?}", self.seen));
                }
            }
        }
    }

    /// Kills and reaps the writer, then reads everything it wrote before dying. Returns every
    /// sequence number it acknowledged and every line it printed (up to a bound).
    pub fn kill(mut self) -> Result<(Vec<u64>, Vec<String>), String> {
        // An error here means the process already exited; reaping below tells.
        let _ = self.child.kill();
        self.child.wait().map_err(|e| format!("reap writer: {e}"))?;
        loop {
            match self.lines.recv_timeout(WATCHDOG) {
                Ok(line) => self.note(&line),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("the killed writer's stdout stayed open for {WATCHDOG:?}"))
                }
            }
        }
        Ok((std::mem::take(&mut self.acks), std::mem::take(&mut self.seen)))
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Files of a store that opening it for writing can change: the database and its companions
/// and the lock record. Blobs are only read by the check.
const MUTABLE_FILES: [&str; 5] = ["project.db", "project.db-wal", "project.db-shm", "lock", "format"];

/// Copies the files of `root` that a write open may change into `image` (replacing it).
fn snapshot(root: &Path, image: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(image);
    std::fs::create_dir_all(image).map_err(|e| format!("{}: {e}", image.display()))?;
    for name in MUTABLE_FILES {
        let from = root.join(name);
        match std::fs::copy(&from, image.join(name)) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", from.display())),
        }
    }
    Ok(())
}

/// A harness run.
#[derive(Debug, Clone)]
pub struct Config {
    pub seed: u64,
    /// Kills in the run.
    pub kills: u64,
    /// Upper bound of the seeded delay between `READY` and the kill.
    pub max_delay_us: u64,
    /// A new store every this many kills (keeps each check short).
    pub rotate_every: u64,
    /// Folder for the stores; created if missing, and each store in it is removed when
    /// replaced (the last one is kept for inspection).
    pub work_dir: PathBuf,
    /// The writer command (program and arguments); `--root <store>` is appended.
    pub writer: Vec<OsString>,
    /// Re-run one instant instead of the schedule: `attempts` tries, each on a new store.
    pub only: Option<KillInstant>,
    pub attempts: u64,
    /// Linux power-loss mode: drop unflushed writes after each kill.
    pub power: Option<PowerCut>,
}

impl Config {
    pub fn new(seed: u64, kills: u64, work_dir: PathBuf, writer: Vec<OsString>) -> Config {
        Config {
            seed,
            kills,
            max_delay_us: 30_000,
            rotate_every: 100,
            work_dir,
            writer,
            only: None,
            attempts: 1,
            power: None,
        }
    }
}

/// What a passing run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub kills: u64,
    pub stores: u64,
    /// Acknowledged commits, over all stores.
    pub acked: u64,
    /// Rows found by the last check of each store, summed.
    pub recovered: u64,
}

fn refused(detail: String) -> CrashFailure {
    CrashFailure::new(FailureKind::StoreRefused, detail)
}

/// Runs the schedule (or the one instant of `config.only`) and checks the store after every
/// kill. The first failed check ends the run; its [`CrashFailure`] carries the seed and the
/// instant, and is also written to `log` with how to reproduce it.
pub fn run(config: &Config, adapter: &dyn CrashAdapter, log: &mut dyn Write) -> Result<Summary, CrashFailure> {
    let plan = KillPlan { seed: config.seed, max_delay_us: config.max_delay_us };
    let instants: Vec<KillInstant> = match config.only {
        Some(i) => (0..config.attempts.max(1)).map(|_| i).collect(),
        None => (0..config.kills).map(|k| plan.instant(k)).collect(),
    };
    let rotate = if config.only.is_some() { 1 } else { config.rotate_every.max(1) };
    std::fs::create_dir_all(&config.work_dir)
        .map_err(|e| refused(format!("work dir {}: {e}", config.work_dir.display())))?;
    let mut summary = Summary::default();
    let mut root = PathBuf::new();
    let mut acked: Vec<u64> = Vec::new();
    let mut last_n = 0;
    for (i, instant) in instants.iter().enumerate() {
        let fail = |f: CrashFailure, log: &mut dyn Write| {
            let f = f.at(config.seed, *instant);
            let _ = writeln!(log, "FAIL {f}");
            #[cfg(feature = "bus-errors")]
            let _ = writeln!(log, "bus error: {}", f.to_bus_error());
            let _ = writeln!(log, "reproduce: --seed {} --instant {instant}", config.seed);
            f
        };
        if i as u64 % rotate == 0 {
            summary.recovered += last_n;
            if !root.as_os_str().is_empty() {
                let _ = std::fs::remove_dir_all(&root);
            }
            root = config.work_dir.join(format!("store-{i}"));
            let _ = std::fs::remove_dir_all(&root);
            adapter.create(&root).map_err(|e| fail(refused(e), log))?;
            if let Some(p) = &config.power {
                p.settle().map_err(|e| fail(refused(format!("power-loss setup: {e}")), log))?;
            }
            acked.clear();
            summary.stores += 1;
        }
        let mut w = Writer::spawn(&config.writer, &root).map_err(|e| fail(refused(e), log))?;
        // Every fourth kill is timed from the spawn, so it can land while the writer is still
        // starting: opening the store, recovering the WAL, taking over the stale lock.
        if !instant.from_spawn() {
            w.wait_for("READY").map_err(|e| fail(refused(e), log))?;
        }
        std::thread::sleep(Duration::from_micros(instant.delay_us));
        let (acks, _) = w.kill().map_err(|e| fail(refused(e), log))?;
        if let Some(p) = &config.power {
            p.cut().map_err(|e| fail(refused(format!("power cut: {e}")), log))?;
        }
        summary.acked += acks.len() as u64;
        acked.extend(acks);
        // The check opens the store for writing (WAL recovery, checkpoint, lock takeover), so
        // the post-crash database files are copied first and kept if the check fails.
        let image = config.work_dir.join("post-crash");
        let saved = snapshot(&root, &image);
        match adapter.check(&root, &acked) {
            Ok(n) => last_n = n,
            Err(f) => {
                let f = fail(f, log);
                match saved {
                    Ok(()) => {
                        let _ = writeln!(
                            log,
                            "post-crash database files kept in {} (blobs are in {}, which the check only reads)",
                            image.display(),
                            root.display()
                        );
                    }
                    Err(e) => {
                        let _ = writeln!(log, "post-crash database files could not be kept: {e}");
                    }
                }
                return Err(f);
            }
        }
        summary.kills += 1;
    }
    summary.recovered += last_n;
    let _ = writeln!(
        log,
        "ok: seed {}, {} kills on {} stores, {} acknowledged commits, {} rows recovered",
        config.seed, summary.kills, summary.stores, summary.acked, summary.recovered
    );
    Ok(summary)
}
