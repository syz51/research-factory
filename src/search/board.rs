//! The cache of the board export that feeds the `board` scope
//! (docs/state-machines.md 3.6).
//!
//! A query never runs the export: it reads `board.json`, the last good output
//! of the export, beside the search index. When that copy is older than
//! `[lab.search] board_max_age`, or missing, the query starts one detached
//! refresh and answers without waiting for it. `lab search --refresh-board`
//! runs the export in the foreground.
//!
//! One refresh runs at a time: it holds an advisory lock on `board.lock`. A
//! query takes the lock and hands it to the refresh it starts as that
//! process's standard input, so the lock stays held from the query's check
//! until the refresh ends, and a second query cannot start a second one.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read as _, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::Project;
use crate::lesson_loop::{Ended, wait_for};

/// How long a refresh lets the export run.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(120);
/// After a failed refresh, a query starts the next one no sooner than this.
pub const RETRY_AFTER: Duration = Duration::from_secs(60);
const SCHEMA: &str = "rf.board-cache/1";

/// What the cache file holds.
#[derive(Serialize, Deserialize)]
struct Stored {
    schema: String,
    /// The command that printed `tasks`; a cache of another command is none.
    command: Vec<String>,
    tasks: Vec<Value>,
}

/// The cached export of the configured command.
pub struct Cache {
    pub tasks: Vec<Value>,
    pub age: Duration,
}

/// Whether a query started a refresh.
pub enum Refresh {
    Started,
    /// Another refresh holds the lock.
    Running,
    /// The last refresh failed less than `RETRY_AFTER` ago.
    Waiting,
}

/// The board export command: `[lab.search] board_export`, else
/// `[lab.archive] board_export`.
pub fn command(project: &Project) -> Option<Vec<String>> {
    project.lab.search.board_export.clone().or_else(|| {
        project
            .lab
            .archive
            .as_ref()
            .and_then(|a| a.board_export.clone())
    })
}

/// How old the cache may be before a query refreshes it.
pub fn max_age(project: &Project) -> Duration {
    Duration::from_secs(project.lab.search.board_max_age.saturating_mul(60))
}

/// The directory of the search index and this cache, one for each checkout.
pub fn dir(project: &Project) -> PathBuf {
    project.state_dir.join("lab").join("search")
}

/// The cached export of `command`, or `None` when there is none, it does not
/// read, or another command wrote it.
pub fn read(dir: &Path, command: &[String]) -> Option<Cache> {
    let path = dir.join("board.json");
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    let stored: Stored = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    (stored.schema == SCHEMA && stored.command == command).then(|| Cache {
        tasks: stored.tasks,
        // A time in the future is no age.
        age: SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default(),
    })
}

/// Why the last refresh failed, when it failed after the last good one, and
/// how long ago.
pub fn last_error(dir: &Path) -> Option<(String, Duration)> {
    let path = dir.join("board.error");
    let text = std::fs::read_to_string(&path).ok()?;
    let at = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    let ago = SystemTime::now().duration_since(at).unwrap_or_default();
    Some((text.trim().to_string(), ago)).filter(|(t, _)| !t.is_empty())
}

/// An age in seconds below one minute, in whole minutes below two hours, else
/// in hours and minutes.
pub fn age_text(age: Duration) -> String {
    match age.as_secs() {
        s if s < 60 => format!("{s} s"),
        s if s < 2 * 3600 => format!("{} min", s / 60),
        s => format!("{} h {} min", s / 3600, s / 60 % 60),
    }
}

fn open_lock(dir: &Path) -> Result<File, String> {
    super::index::create_dir(dir)?;
    let path = dir.join("board.lock");
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// For whoever looks: which process refreshes.
fn note_holder(lock: &mut File, pid: u32) {
    let _ = lock.set_len(0).and_then(|()| writeln!(lock, "pid {pid}"));
}

/// Starts a detached refresh unless one runs or the last one failed less
/// than `RETRY_AFTER` ago, so an export that keeps failing does not run back
/// to back. The refresh is `lab search --refresh-board --board-child` with
/// this project's config, in its own process group, with its output in
/// `board.log`.
pub fn start(project: &Project, dir: &Path) -> Result<Refresh, String> {
    if last_error(dir).is_some_and(|(_, ago)| ago < RETRY_AFTER) {
        return Ok(Refresh::Waiting);
    }
    let mut lock = open_lock(dir)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(Refresh::Running),
        Err(TryLockError::Error(e)) => return Err(format!("board.lock: {e}")),
    }
    let log_path = dir.join("board.log");
    let log = File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot find the lab executable: {e}"))?;
    let mut cmd = Command::new(&exe);
    if let Some(config) = &project.config {
        cmd.arg("--config").arg(config);
    }
    // The child's standard input is the locked file: the lock belongs to the
    // open file, so it stays held while the child runs, and ends with it.
    let held = lock.try_clone().map_err(|e| format!("board.lock: {e}"))?;
    let mut child = cmd
        .args(["search", "--refresh-board", "--board-child"])
        .current_dir(&project.root)
        .stdin(held)
        .stdout(log)
        .stderr(err)
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{} did not start: {e}", exe.display()))?;
    note_holder(&mut lock, child.id());
    // Reap it, so a long-lived `lab mcp` leaves no zombie behind.
    std::thread::spawn(move || child.wait());
    Ok(Refresh::Started)
}

/// What a refresh wrote.
pub struct Refreshed {
    pub tasks: usize,
    pub took: Duration,
}

impl std::fmt::Display for Refreshed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Board export cached: {} task(s) in {:.1} s.",
            self.tasks,
            self.took.as_secs_f64()
        )
    }
}

/// Runs the export and writes the cache atomically. A failed export keeps
/// the old cache, records why in `board.error` and returns the error.
///
/// `child`: the detached refresh, whose standard input holds the lock; it
/// does nothing when the cache became fresh since its query looked.
/// Otherwise this waits for a running refresh, then runs the export anyway.
pub fn refresh(project: &Project, child: bool) -> Result<Option<Refreshed>, String> {
    let dir = dir(project);
    let Some(command) = command(project) else {
        return Err(
            "no board export command is configured ([lab.search] board_export or \
             [lab.archive] board_export)"
                .into(),
        );
    };
    // Held until this function returns.
    let _lock = if child {
        if read(&dir, &command).is_some_and(|c| c.age < max_age(project)) {
            return Ok(None);
        }
        None
    } else {
        let mut lock = open_lock(&dir)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                eprintln!("lab: waiting for the running board refresh");
                lock.lock().map_err(|e| format!("board.lock: {e}"))?;
            }
            Err(TryLockError::Error(e)) => return Err(format!("board.lock: {e}")),
        }
        note_holder(&mut lock, std::process::id());
        Some(lock)
    };
    let started = Instant::now();
    let error_path = dir.join("board.error");
    match export(project, &command, REFRESH_TIMEOUT) {
        Ok(tasks) => {
            let n = tasks.len();
            let text = serde_json::to_vec(&Stored {
                schema: SCHEMA.into(),
                command,
                tasks,
            })
            .map_err(|e| e.to_string())?;
            let path = dir.join("board.json");
            let tmp = dir.join(format!("board.json.{}.tmp", std::process::id()));
            std::fs::write(&tmp, text)
                .and_then(|()| std::fs::rename(&tmp, &path))
                .map_err(|e| {
                    let _ = std::fs::remove_file(&tmp);
                    format!("{}: {e}", path.display())
                })?;
            match std::fs::remove_file(&error_path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!("{}: {e}", error_path.display()));
                }
                _ => {}
            }
            Ok(Some(Refreshed {
                tasks: n,
                took: started.elapsed(),
            }))
        }
        Err(why) => {
            std::fs::write(&error_path, format!("{why}\n"))
                .map_err(|e| format!("{}: {e}", error_path.display()))?;
            let kept = match read(&dir, &command) {
                Some(c) => format!("the cached copy from {} ago stays", age_text(c.age)),
                None => "no cached copy exists".into(),
            };
            Err(format!("the board export failed: {why}; {kept}"))
        }
    }
}

/// Runs the board export and reads its JSON: an array of tasks, or an
/// object with a `tasks` array. On timeout it kills the export's whole
/// process group.
fn export(project: &Project, command: &[String], timeout: Duration) -> Result<Vec<Value>, String> {
    let (program, args) = command.split_first().ok_or("the command is empty")?;
    let mut child = Command::new(project.resolve_program(program))
        .args(args)
        .current_dir(&project.root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("`{program}` did not start: {e}"))?;
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = tx.send(stdout.read_to_end(&mut text).map(|_| text));
    });
    let status = match wait_for(&mut child, timeout).map_err(|e| format!("`{program}`: {e}"))? {
        Ended::TimedOut => {
            return Err(format!(
                "`{program}` did not finish in {} seconds",
                timeout.as_secs()
            ));
        }
        Ended::Exit(status) => status,
    };
    if !status.success() {
        return Err(format!("`{program}` failed ({status})"));
    }
    // A program it started in the background may hold the output open.
    let text = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| format!("`{program}` exited, but its output stayed open"))?
        .map_err(|e| format!("`{program}`: {e}"))?;
    let value: Value = serde_json::from_slice(&text)
        .map_err(|e| format!("`{program}` did not print JSON: {e}"))?;
    match value {
        Value::Array(list) => Ok(list),
        Value::Object(mut map) => match map.remove("tasks") {
            Some(Value::Array(list)) => Ok(list),
            _ => Err(format!("`{program}` printed no array of tasks")),
        },
        _ => Err(format!("`{program}` printed no array of tasks")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_age_reads_in_seconds_minutes_or_hours() {
        assert_eq!(age_text(Duration::from_secs(42)), "42 s");
        assert_eq!(age_text(Duration::from_secs(23 * 60 + 5)), "23 min");
        assert_eq!(
            age_text(Duration::from_secs(3 * 3600 + 7 * 60)),
            "3 h 7 min"
        );
    }

    #[test]
    fn a_cache_of_another_command_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let stored = serde_json::json!({"schema": SCHEMA, "command": ["cat", "a.json"],
            "tasks": [{"id": "t-1"}]});
        std::fs::write(dir.path().join("board.json"), stored.to_string()).unwrap();
        let mine = ["cat".to_string(), "a.json".to_string()];
        assert_eq!(read(dir.path(), &mine).unwrap().tasks.len(), 1);
        assert!(read(dir.path(), &["cat".to_string(), "b.json".to_string()]).is_none());
        std::fs::write(dir.path().join("board.json"), "not json").unwrap();
        assert!(read(dir.path(), &mine).is_none());
    }
}
