//! `lab loop`: measure, evaluate, derive signals, tell someone, record.
//!
//! Hooks start it on every turn with `--background --if-older N`, so the
//! foreground part only spawns a detached copy of itself; the copy checks the
//! age of the last run, takes a lock, and does the work.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::Project;
use crate::evaluate::Evaluation;
use crate::lessons::Lesson;
use crate::signals::{self, Candidate, MeasureFailure, Signal, State};

pub const SCHEMA: &str = "rf.loop/1";
/// A lock younger than this belongs to a running loop.
const LOCK_STALE: jiff::SignedDuration = jiff::SignedDuration::from_mins(30);
const MEASURE_TIMEOUT: Duration = Duration::from_secs(600);
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(60);
const TAIL_LINES: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasureResult {
    pub argv: Vec<String>,
    pub ok: bool,
    /// `exit 0`, `exit 2`, `killed by a signal`, `timed out after 600 s`, `could not start: …`.
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SendStatus {
    /// Sent to the notify command, which exited 0.
    Notified,
    /// Waiting to be sent: no notify command is configured.
    Pending,
    /// The notify command failed or timed out; retried on the next loop.
    Failed,
    /// Already sent and not changed enough to send again.
    Unchanged,
}

impl SendStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notified => "notified",
            Self::Pending => "pending",
            Self::Failed => "failed",
            Self::Unchanged => "unchanged",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalStatus {
    pub id: String,
    pub kind: String,
    pub status: SendStatus,
}

/// `last.json`: the latest loop's results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub schema: String,
    pub started: String,
    pub finished: String,
    pub measure: Vec<MeasureResult>,
    pub evaluations: Vec<Evaluation>,
    pub signals: Vec<SignalStatus>,
    pub errors: Vec<String>,
}

/// What the loop measures lessons with, computed after the measure commands ran.
pub struct Analysis {
    pub lessons: Vec<Lesson>,
    pub evaluations: Vec<Evaluation>,
    pub candidates: Vec<Candidate>,
}

fn last_path(dir: &Path) -> PathBuf {
    dir.join("last.json")
}

/// The latest loop record; `None` when the loop has never finished.
pub fn read_record(project: &Project) -> Result<Option<Record>, String> {
    let path = last_path(&project.loop_dir());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let r: Record = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if r.schema != SCHEMA {
        return Err(format!("{}: not a {SCHEMA} record", path.display()));
    }
    Ok(Some(r))
}

fn create_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

fn open_log(dir: &Path) -> Result<std::fs::File, String> {
    let path = dir.join("loop.log");
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Starts `lab` again with the same arguments minus `--background`, detached
/// in its own process group with its output appended to `loop.log`, and
/// returns without waiting for it.
pub fn background(project: &Project) -> Result<ExitCode, String> {
    use std::os::unix::process::CommandExt as _;
    let dir = project.loop_dir();
    create_dir(&dir)?;
    let log = open_log(&dir)?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot find the lab executable: {e}"))?;
    let args: Vec<std::ffi::OsString> = std::env::args_os()
        .skip(1)
        .filter(|a| a != "--background")
        .collect();
    Command::new(&exe)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err)
        .process_group(0)
        .spawn()
        .map_err(|e| format!("could not start {} in the background: {e}", exe.display()))?;
    Ok(ExitCode::SUCCESS)
}

/// The lock file, removed when dropped.
struct Lock(PathBuf);

#[derive(Serialize, Deserialize)]
struct LockBody {
    pid: u32,
    started: String,
}

impl Lock {
    /// Takes the lock, or `None` when a loop younger than 30 minutes holds it.
    /// An older lock is left by a loop that died, and is taken over.
    fn acquire(dir: &Path, now: jiff::Timestamp) -> Result<Option<Self>, String> {
        let path = dir.join("lock");
        for attempt in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    let lock = Self(path.clone());
                    let body = serde_json::to_string(&LockBody {
                        pid: std::process::id(),
                        started: now.to_string(),
                    })
                    .map_err(|e| e.to_string())?;
                    f.write_all(body.as_bytes())
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    return Ok(Some(lock));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let started = lock_started(&path);
                    let stale = started.is_some_and(|t| now.duration_since(t) >= LOCK_STALE);
                    if attempt > 0 || !stale {
                        return Ok(None);
                    }
                    println!(
                        "taking over a lock from {} (older than 30 minutes)",
                        started.map(|t| t.to_string()).unwrap_or_default()
                    );
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(format!("{}: {e}", path.display())),
                    }
                }
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        }
        Ok(None)
    }
}

/// When the lock was taken: its recorded time, else its modification time.
fn lock_started(path: &Path) -> Option<jiff::Timestamp> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<LockBody>(&t).ok())
        .and_then(|b| b.started.parse().ok())
        .or_else(|| {
            let m = std::fs::metadata(path).ok()?.modified().ok()?;
            jiff::Timestamp::try_from(m).ok()
        })
}

impl Drop for Lock {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.0)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("lab: {}: {e}", self.0.display());
        }
    }
}

/// How a child process ended.
enum Ended {
    Exit(std::process::ExitStatus),
    TimedOut,
}

fn wait_for(child: &mut std::process::Child, timeout: Duration) -> std::io::Result<Ended> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Ended::Exit(status));
        }
        if start.elapsed() >= timeout {
            // Already gone is fine; the wait below reaps it either way.
            let _ = child.kill();
            child.wait()?;
            return Ok(Ended::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn describe(status: std::process::ExitStatus) -> String {
    status
        .code()
        .map_or_else(|| "killed by a signal".into(), |c| format!("exit {c}"))
}

fn tail(text: &str, lines: usize, max_chars: usize) -> String {
    let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let t = all[all.len().saturating_sub(lines)..].join("\n");
    let n = t.chars().count();
    if n <= max_chars {
        t
    } else {
        t.chars().skip(n - max_chars).collect()
    }
}

/// Runs one measure command with its output in `out_path`, then appends that
/// output to the log.
fn run_measure(
    project: &Project,
    argv: &[String],
    out_path: &Path,
    log: &mut std::fs::File,
) -> (MeasureResult, String) {
    let _ = writeln!(log, "-- measure: {}", argv.join(" "));
    let result = (|| -> Result<(bool, String), String> {
        let out =
            std::fs::File::create(out_path).map_err(|e| format!("{}: {e}", out_path.display()))?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let mut child = Command::new(project.resolve_program(&argv[0]))
            .args(&argv[1..])
            .current_dir(&project.root)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()
            .map_err(|e| format!("could not start: {e}"))?;
        Ok(
            match wait_for(&mut child, MEASURE_TIMEOUT).map_err(|e| e.to_string())? {
                Ended::Exit(s) => (s.success(), describe(s)),
                Ended::TimedOut => (
                    false,
                    format!("timed out after {} s", MEASURE_TIMEOUT.as_secs()),
                ),
            },
        )
    })();
    let output = std::fs::read_to_string(out_path).unwrap_or_default();
    let _ = std::fs::remove_file(out_path);
    let _ = log.write_all(output.as_bytes());
    let (ok, status) = result.unwrap_or_else(|e| (false, e));
    let _ = writeln!(log, "-- {status}");
    (
        MeasureResult {
            argv: argv.to_vec(),
            ok,
            status,
        },
        tail(&output, TAIL_LINES, 2000),
    )
}

/// Sends one signal: its JSON on the notify command's stdin. `Ok` holds the
/// command's stdout.
fn notify(
    project: &Project,
    argv: &[String],
    signal: &Signal,
    log: &mut std::fs::File,
) -> Result<String, String> {
    let json = serde_json::to_string(signal).map_err(|e| e.to_string())? + "\n";
    let mut child = Command::new(project.resolve_program(&argv[0]))
        .args(&argv[1..])
        .current_dir(&project.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start `{}`: {e}", argv[0]))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let writer = std::thread::spawn(move || stdin.write_all(json.as_bytes()));
    let (tx, rx) = std::sync::mpsc::channel();
    for (n, mut pipe) in [
        (
            0,
            Box::new(child.stdout.take().expect("stdout is piped"))
                as Box<dyn std::io::Read + Send>,
        ),
        (1, Box::new(child.stderr.take().expect("stderr is piped"))),
    ] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            let _ = tx.send((n, buf));
        });
    }
    drop(tx);
    let ended = wait_for(&mut child, NOTIFY_TIMEOUT).map_err(|e| e.to_string())?;
    let _ = writer.join();
    // A process the notify command left running may hold the pipes open.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    for _ in 0..2 {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok((0, b)) => out = b,
            Ok((_, b)) => err = b,
            Err(_) => break,
        }
    }
    let (out, err) = (
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    );
    if !err.trim().is_empty() {
        let _ = writeln!(log, "-- notify {}: {}", signal.id, err.trim_end());
    }
    match ended {
        Ended::Exit(s) if s.success() => Ok(out),
        Ended::Exit(s) => Err(format!("{}: {}", describe(s), tail(&err, 10, 500))),
        Ended::TimedOut => Err(format!(
            "timed out after {} s: {}",
            NOTIFY_TIMEOUT.as_secs(),
            tail(&err, 10, 500)
        )),
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    write_atomic(
        path,
        &(serde_json::to_string_pretty(value).map_err(|e| e.to_string())? + "\n"),
    )
}

/// `lab loop` in the foreground.
pub fn run(
    project: &Project,
    if_older: Option<u64>,
    jsonl: bool,
    analyse: impl FnOnce() -> Result<Analysis, String>,
) -> Result<ExitCode, String> {
    let dir = project.loop_dir();
    create_dir(&dir)?;
    let now = jiff::Timestamp::now();
    if let Some(minutes) = if_older
        && let Ok(Some(last)) = read_record(project)
        && let Ok(finished) = last.finished.parse::<jiff::Timestamp>()
        && now.duration_since(finished) < jiff::SignedDuration::from_mins(minutes as i64)
    {
        return Ok(ExitCode::SUCCESS);
    }
    let Some(_lock) = Lock::acquire(&dir, now)? else {
        println!("another loop is running");
        return Ok(ExitCode::SUCCESS);
    };
    let started = crate::finding::now();
    let mut log = open_log(&dir)?;
    let _ = writeln!(log, "== lab loop {started} (pid {})", std::process::id());
    let cfg = project
        .lab
        .lesson_loop
        .clone()
        .unwrap_or(crate::config::LoopConfig {
            measure: Vec::new(),
            notify: None,
            renotify_hours: 24,
        });
    let mut measures = Vec::new();
    let mut failures = Vec::new();
    for (i, argv) in cfg.measure.iter().enumerate() {
        let (m, out) = run_measure(project, argv, &dir.join("measure.out"), &mut log);
        if !m.ok {
            failures.push(MeasureFailure {
                index: i,
                argv: argv.clone(),
                status: m.status.clone(),
                tail: out,
            });
        }
        measures.push(m);
    }
    let mut record = Record {
        schema: SCHEMA.into(),
        started: started.clone(),
        finished: String::new(),
        measure: measures,
        evaluations: Vec::new(),
        signals: Vec::new(),
        errors: Vec::new(),
    };
    let analysis = match analyse() {
        Ok(a) => a,
        Err(e) => {
            // Record the error, so the brief shows it and `--if-older` backs off.
            record.errors.push(e.clone());
            record.finished = crate::finding::now();
            save_json(&last_path(&dir), &record)?;
            let _ = writeln!(log, "-- error: {e}");
            return Err(e);
        }
    };
    let now = jiff::Timestamp::now();
    let now = now.round(jiff::Unit::Second).unwrap_or(now);
    let at = now.to_string();
    let mut sigs = signals::derive(
        &project.name,
        &project.root,
        &at,
        &analysis.lessons,
        &analysis.evaluations,
        &analysis.candidates,
        &failures,
    );
    let state_path = dir.join("signals.json");
    let mut state = State::load(&state_path)?;
    let send = state.plan(
        &mut sigs,
        now,
        jiff::SignedDuration::from_hours(cfg.renotify_hours as i64),
    );
    let mut notify_failed = false;
    for (s, send) in sigs.iter().zip(send) {
        let status = match (send, &cfg.notify) {
            (false, _) => SendStatus::Unchanged,
            (true, None) => SendStatus::Pending,
            (true, Some(argv)) => match notify(project, argv, s, &mut log) {
                Ok(out) => {
                    let reference = out
                        .lines()
                        .map(str::trim)
                        .find(|l| !l.is_empty())
                        .map(|l| l.chars().take(200).collect::<String>());
                    state.sent(&s.id, now, reference);
                    SendStatus::Notified
                }
                Err(e) => {
                    notify_failed = true;
                    record.errors.push(format!("notify {}: {e}", s.id));
                    state.failed(&s.id, e);
                    SendStatus::Failed
                }
            },
        };
        record.signals.push(SignalStatus {
            id: s.id.clone(),
            kind: s.kind.clone(),
            status,
        });
    }
    // The signals as sent carry their stored ref, including one just returned.
    for s in &mut sigs {
        s.reference = state.signals.get(&s.id).and_then(|e| e.reference.clone());
    }
    save_json(&state_path, &state)?;
    record.evaluations = analysis.evaluations;
    record.finished = crate::finding::now();
    save_json(&last_path(&dir), &record)?;
    let failed_measures = record.measure.iter().filter(|m| !m.ok).count();
    let _ = writeln!(
        log,
        "-- done: {} evaluation(s), {} signal(s), {failed_measures} failed measure(s)",
        record.evaluations.len(),
        sigs.len()
    );
    if jsonl {
        for s in &sigs {
            println!("{}", serde_json::to_string(s).map_err(|e| e.to_string())?);
        }
    } else {
        print!("{}", summary(&record, &sigs));
    }
    Ok(if failed_measures > 0 || notify_failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn summary(r: &Record, sigs: &[Signal]) -> String {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    let mut out = String::new();
    let failed = r.measure.iter().filter(|m| !m.ok).count();
    let mut verdicts: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &r.evaluations {
        *verdicts.entry(e.verdict.as_str()).or_default() += 1;
    }
    let mut statuses: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &r.signals {
        *statuses.entry(s.status.as_str()).or_default() += 1;
    }
    let join = |m: &BTreeMap<&str, usize>| {
        if m.is_empty() {
            "none".to_string()
        } else {
            m.iter()
                .map(|(k, v)| format!("{v} {k}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    let _ = writeln!(
        out,
        "Ran {} measure command(s), {failed} failed. Evaluated {} lesson(s): {}.",
        r.measure.len(),
        r.evaluations.len(),
        join(&verdicts)
    );
    let _ = writeln!(out, "Signals: {}.", join(&statuses));
    for (s, st) in sigs.iter().zip(&r.signals) {
        if st.status != SendStatus::Unchanged {
            let _ = writeln!(out, "- [{}] {}", st.status.as_str(), s.title);
        }
    }
    for e in &r.errors {
        let _ = writeln!(out, "Error: {e}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_lock_blocks_and_a_stale_one_is_taken_over() {
        let dir = tempfile::tempdir().unwrap();
        let now = jiff::Timestamp::now();
        let held = Lock::acquire(dir.path(), now).unwrap().unwrap();
        assert!(Lock::acquire(dir.path(), now).unwrap().is_none(), "held");
        let later = now + jiff::SignedDuration::from_mins(31);
        let taken = Lock::acquire(dir.path(), later).unwrap();
        assert!(taken.is_some(), "stale after 30 minutes");
        std::mem::forget(held); // its file now belongs to `taken`
        drop(taken);
        assert!(!dir.path().join("lock").exists(), "removed when dropped");
    }

    #[test]
    fn tails_are_bounded() {
        assert_eq!(tail("a\n\nb\nc\n", 2, 100), "b\nc");
        assert_eq!(tail("abcdef", 5, 3), "def");
    }
}
