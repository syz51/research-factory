//! `lab loop`: measure, evaluate, derive signals, tell someone, record.
//!
//! Hooks start it on every turn with `--background --if-older N`, so the
//! foreground part only spawns a detached copy of itself; the copy checks the
//! age of the last run, takes a lock, and does the work.

use std::io::{Read as _, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::Project;
use crate::evaluate::Evaluation;
use crate::lessons::Lesson;
use crate::signals::{self, MeasureFailure, Signal, State, Uncovered};

pub const SCHEMA: &str = "rf.loop/1";
const MEASURE_TIMEOUT: Duration = Duration::from_secs(600);
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(60);
const TAIL_LINES: usize = 20;
/// `loop.log` is moved to `loop.log.1` once it is larger than this.
const LOG_ROTATE_BYTES: u64 = 1 << 20;

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
    pub uncovered: Uncovered,
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
    let dir = project.loop_dir();
    create_dir(&dir)?;
    let path = dir.join("loop.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
        let old = dir.join("loop.log.1");
        std::fs::rename(&path, &old).map_err(|e| format!("{}: {e}", old.display()))?;
    }
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

/// An exclusive lock on `<loop dir>/lock`, held while the loop runs. The
/// operating system releases it when the process exits, however it exits, so
/// a loop that died leaves no stale lock behind.
struct Lock {
    _file: std::fs::File,
}

impl Lock {
    /// Takes the lock, or `None` when another loop holds it.
    fn acquire(dir: &Path) -> Result<Option<Self>, String> {
        let path = dir.join("lock");
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        match f.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(format!("{}: {e}", path.display()));
            }
        }
        // For whoever looks: which process holds it.
        f.set_len(0)
            .and_then(|()| writeln!(f, "pid {}", std::process::id()))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Some(Self { _file: f }))
    }
}

/// How a child process ended.
enum Ended {
    Exit(std::process::ExitStatus),
    TimedOut,
}

/// Waits for a child started in its own process group (`process_group(0)`);
/// on timeout kills the whole group, so programs a wrapper such as `sh`
/// started do not outlive it.
fn wait_for(child: &mut std::process::Child, timeout: Duration) -> std::io::Result<Ended> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Ended::Exit(status));
        }
        if start.elapsed() >= timeout {
            let group = format!("-{}", child.id());
            let killed = Command::new("/bin/kill")
                .args(["-KILL", "--", &group])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if !killed.is_ok_and(|s| s.success()) {
                // The group is already gone, or kill failed: at least the child.
                let _ = child.kill();
            }
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
            .process_group(0)
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
    let output = match std::fs::read(out_path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => format!("(could not read the output in {}: {e})", out_path.display()),
    };
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
        .process_group(0)
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
    match writer.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = writeln!(log, "-- notify {}: writing the signal: {e}", signal.id);
        }
        Err(_) => {
            let _ = writeln!(log, "-- notify {}: the writer thread panicked", signal.id);
        }
    }
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

fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())? + "\n";
    crate::archive::write_atomically(path, text.as_bytes())
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
    let Some(_lock) = Lock::acquire(&dir)? else {
        println!("another loop is running");
        return Ok(ExitCode::SUCCESS);
    };
    // Checked under the lock, so a loop that just finished is seen.
    let now = jiff::Timestamp::now();
    if let Some(minutes) = if_older
        && let Ok(Some(last)) = read_record(project)
        && let Ok(finished) = last.finished.parse::<jiff::Timestamp>()
        && now.duration_since(finished) < jiff::SignedDuration::from_mins(minutes as i64)
    {
        return Ok(ExitCode::SUCCESS);
    }
    let started = crate::finding::now();
    let mut log = open_log(&dir)?;
    let _ = writeln!(log, "== lab loop {started} (pid {})", std::process::id());
    let cfg = project.lab.lesson_loop.clone().unwrap_or_default();
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
    let state_path = dir.join("signals.json");
    let sigs = match evaluate_and_send(
        project,
        &cfg,
        analyse,
        &failures,
        &state_path,
        &mut record,
        &mut log,
    ) {
        Ok(sigs) => sigs,
        Err(e) => {
            // Record the error, so the brief shows it and `--if-older` backs off.
            record.errors.push(e.clone());
            record.finished = crate::finding::now();
            let _ = writeln!(log, "-- error: {e}");
            save_json(&last_path(&dir), &record)?;
            return Err(e);
        }
    };
    record.finished = crate::finding::now();
    save_json(&last_path(&dir), &record)?;
    let notify_failed = record
        .signals
        .iter()
        .any(|s| s.status == SendStatus::Failed);
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

/// Evaluates, derives the signals, sends the due ones and saves the dedupe
/// state, filling in `record`. Returns the signals as sent.
fn evaluate_and_send(
    project: &Project,
    cfg: &crate::config::LoopConfig,
    analyse: impl FnOnce() -> Result<Analysis, String>,
    failures: &[MeasureFailure],
    state_path: &Path,
    record: &mut Record,
    log: &mut std::fs::File,
) -> Result<Vec<Signal>, String> {
    let analysis = analyse()?;
    let now = jiff::Timestamp::now();
    let now = now.round(jiff::Unit::Second).unwrap_or(now);
    let at = now.to_string();
    let mut sigs = signals::derive(
        &project.name,
        &project.root,
        &at,
        &analysis.lessons,
        &analysis.evaluations,
        &analysis.uncovered,
        failures,
    );
    record.evaluations = analysis.evaluations;
    let mut state = State::load(state_path)?;
    let send = state.plan(
        &mut sigs,
        now,
        jiff::SignedDuration::from_hours(cfg.renotify_hours as i64),
    );
    for (s, send) in sigs.iter().zip(send) {
        let status = match (send, &cfg.notify) {
            (false, _) => SendStatus::Unchanged,
            (true, None) => SendStatus::Pending,
            (true, Some(argv)) => match notify(project, argv, s, log) {
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
    save_json(state_path, &state)?;
    Ok(sigs)
}

fn summary(r: &Record, sigs: &[Signal]) -> String {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    let mut out = String::new();
    let failed = r.measure.iter().filter(|m| !m.ok).count();
    let mut verdicts: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &r.evaluations {
        // Check runs without a measured interval are described, not judged.
        let label = if e.method == Some(crate::evaluate::Method::Descriptive)
            && e.verdict == crate::evaluate::Verdict::Collecting
        {
            "described"
        } else {
            e.verdict.as_str()
        };
        *verdicts.entry(label).or_default() += 1;
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
    fn the_lock_is_exclusive_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let held = Lock::acquire(dir.path()).unwrap().unwrap();
        assert!(Lock::acquire(dir.path()).unwrap().is_none(), "held");
        drop(held);
        // Another test's child can briefly share the descriptor between fork
        // and exec (it is close-on-exec), so allow a moment.
        let start = Instant::now();
        while Lock::acquire(dir.path()).unwrap().is_none() {
            assert!(start.elapsed() < Duration::from_secs(5), "released");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_timeout_kills_the_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild");
        // `sh` starts a long sleep in the background, records its pid and waits.
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "sleep 60 & echo $! > '{}'; wait",
                pid_file.display()
            ))
            .process_group(0)
            .spawn()
            .unwrap();
        let start = Instant::now();
        while !pid_file.exists()
            || std::fs::read_to_string(&pid_file)
                .unwrap()
                .trim()
                .is_empty()
        {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "sh did not start"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let ended = wait_for(&mut child, Duration::from_millis(200)).unwrap();
        assert!(matches!(ended, Ended::TimedOut));
        let grandchild = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .to_string();
        // Signal 0 only checks for the process; allow a moment for the kill to land.
        let start = Instant::now();
        loop {
            let alive = Command::new("/bin/kill")
                .args(["-0", &grandchild])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success();
            if !alive {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "grandchild survived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn tails_are_bounded() {
        assert_eq!(tail("a\n\nb\nc\n", 2, 100), "b\nc");
        assert_eq!(tail("abcdef", 5, 3), "def");
    }
}
