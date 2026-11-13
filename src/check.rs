//! `lab check`: run the configured checkers, store their findings, and compare
//! them with the accepted baseline.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::config::{CheckSpec, Project};
use crate::finding::{self, Finding, Severity};

pub const RUN_SCHEMA: &str = "rf.labcheck/1";
pub const ITEM_SCHEMA: &str = "rf.labcheck.item/1";
pub const MEASURE_ITEM_SCHEMA: &str = "rf.labcheck.measure/1";

/// An `rf.measure/1` record: how many units of text a checker checked, the
/// denominator of its findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measure {
    pub schema: String,
    pub tool: String,
    /// What is counted, such as `citing-sentence`.
    pub unit: String,
    pub count: u64,
    /// The sorted, distinct first 16 hex digits of the SHA-256 of each
    /// unit's normalized text; they let lab count new units exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
}

impl Measure {
    /// Checks a checker's `rf.measure/1` record.
    fn parse(value: serde_json::Value) -> Result<Self, String> {
        let m: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if !finding::version_1(&m.schema, "rf.measure/") {
            return Err(format!("unsupported schema `{}`", m.schema));
        }
        if m.tool.is_empty() || m.unit.is_empty() {
            return Err("an rf.measure/1 record needs a tool and a unit".into());
        }
        if let Some(keys) = &m.keys {
            if let Some(k) = keys.iter().find(|k| {
                k.len() != 16 || !k.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            }) {
                return Err(format!("key `{k}` is not 16 lowercase hex digits"));
            }
            if keys.windows(2).any(|w| w[0] >= w[1]) {
                return Err("keys are not sorted and distinct".into());
            }
            if keys.len() as u64 > m.count {
                return Err(format!("{} keys for a count of {}", keys.len(), m.count));
            }
        }
        Ok(m)
    }
}

/// A stored `rf.measure/1` record with the check that printed it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasureItem {
    pub schema: String,
    pub check: String,
    pub measure: Measure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckOutcome {
    pub name: String,
    pub command: Vec<String>,
    /// Exit code, or `None` when the program could not start or was killed.
    pub exit: Option<i32>,
    /// `ok` when the checker ran to completion (exit 0 or 1), else `failed`.
    pub status: String,
    pub findings: usize,
    /// Non-empty stdout lines that were neither `rf.finding/1` nor valid
    /// `rf.measure/1` records.
    pub unparsed: usize,
    pub duration_ms: u128,
    /// The checker's version (see `versions`), or `None` when unknown and
    /// in runs stored before lab 0.9.0.
    #[serde(default)]
    pub version: Option<String>,
    /// A replayed run's inputs read from the current work tree, since no
    /// commit holds them (`uncommitted_inputs` in `[[lab.checks]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncommitted_inputs: Vec<String>,
}

impl CheckOutcome {
    pub fn ran(&self) -> bool {
        self.status == "ok"
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunHeader {
    pub schema: String,
    pub at: String,
    /// The `HEAD` commit of the git repository holding the project root when
    /// the run started; `None` outside git, before the first commit, and in
    /// runs stored before lab 0.7.0.
    #[serde(default)]
    pub commit: Option<String>,
    /// Whether files under the project root differed from `commit` (tracked
    /// changes or untracked files, leaving out the state directory), since
    /// checks read the work tree; `None` when `commit` is.
    #[serde(default)]
    pub dirty: Option<bool>,
    /// A run of a past commit by `lab check --at`, with today's checkers in
    /// a temporary worktree; its `at` is the commit's time (since lab 0.9.0).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replayed: bool,
    pub checks: Vec<CheckOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub schema: String,
    pub check: String,
    pub fingerprint: String,
    pub finding: Finding,
}

/// One stored `lab check` run.
#[derive(Debug)]
pub struct Run {
    pub path: PathBuf,
    pub header: RunHeader,
    pub items: Vec<Item>,
    /// `rf.measure/1` records the checks printed (since lab 0.8.0).
    pub measures: Vec<MeasureItem>,
}

impl Run {
    /// The stored run's file name, which identifies it exactly (its `at` has
    /// whole seconds and is the start time).
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn load(path: &Path) -> Result<Self, String> {
        Ok(Self::read(path, true)?.expect("replayed runs are kept"))
    }

    /// Loads a stored run; `None` for a replayed one unless `replays`.
    fn read(path: &Path, replays: bool) -> Result<Option<Self>, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut lines = text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty());
        let (_, first) = lines
            .next()
            .ok_or_else(|| format!("{}: empty check run", path.display()))?;
        let header: RunHeader =
            serde_json::from_str(first).map_err(|e| format!("{}:1: {e}", path.display()))?;
        if header.schema != RUN_SCHEMA {
            return Err(format!("{}:1: not a {RUN_SCHEMA} record", path.display()));
        }
        if header.replayed && !replays {
            return Ok(None);
        }
        // Lines of another schema are records a later lab version added;
        // skipping them keeps a shared run directory readable.
        let mut items = Vec::new();
        let mut measures = Vec::new();
        for (i, line) in lines {
            let at = |e: serde_json::Error| format!("{}:{}: {e}", path.display(), i + 1);
            let value: serde_json::Value = serde_json::from_str(line).map_err(at)?;
            match value.get("schema").and_then(|s| s.as_str()) {
                Some(ITEM_SCHEMA) => items.push(serde_json::from_value::<Item>(value).map_err(at)?),
                Some(MEASURE_ITEM_SCHEMA) => {
                    measures.push(serde_json::from_value::<MeasureItem>(value).map_err(at)?);
                }
                Some(_) => {}
                None => return Err(format!("{}:{}: no schema", path.display(), i + 1)),
            }
        }
        Ok(Some(Self {
            path: path.to_path_buf(),
            header,
            items,
            measures,
        }))
    }

    fn fingerprints(&self, check: &str) -> BTreeSet<&str> {
        self.items
            .iter()
            .filter(|i| i.check == check)
            .map(|i| i.fingerprint.as_str())
            .collect()
    }
}

/// Stored runs of the work tree, oldest first, without replayed runs, which
/// only `lab evaluate` reads. A missing directory means no history.
pub fn history(project: &Project) -> Result<Vec<Run>, String> {
    load_runs(project, false)
}

/// Every stored run, replayed ones included, in the order they were stored.
pub fn history_with_replays(project: &Project) -> Result<Vec<Run>, String> {
    load_runs(project, true)
}

fn load_runs(project: &Project, replays: bool) -> Result<Vec<Run>, String> {
    let mut runs = Vec::new();
    for path in &run_paths(project)? {
        // A replayed run is left out by its header, before its lines are parsed.
        if let Some(run) = Run::read(path, replays)? {
            runs.push(run);
        }
    }
    Ok(runs)
}

/// The stored run files, in the order they were stored.
fn run_paths(project: &Project) -> Result<Vec<PathBuf>, String> {
    let dir = project.checks_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.extension().is_some_and(|x| x == "jsonl") {
            paths.push(path);
        }
    }
    paths.sort_by_cached_key(|p| run_order(p));
    Ok(paths)
}

/// `<stamp>.jsonl` sorts before `<stamp>-1.jsonl`, which sorts before `<stamp>-2.jsonl`.
fn run_order(path: &Path) -> (String, u32) {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match stem.rsplit_once('-') {
        Some((stamp, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
            (stamp.to_string(), n.parse().unwrap_or(u32::MAX))
        }
        _ => (stem, 0),
    }
}

/// One stdout line of a checker.
enum Line {
    Finding(Finding),
    Measure(Measure),
}

impl Line {
    fn parse(line: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let measure = value
            .get("schema")
            .and_then(|s| s.as_str())
            .is_some_and(|s| s.starts_with("rf.measure/"));
        if measure {
            Measure::parse(value).map(Self::Measure)
        } else {
            Finding::from_value(value).map(Self::Finding)
        }
    }
}

/// A check's arguments, with each that names a gitignored file a linked git
/// worktree lacks replaced by the main checkout's path, such as a downloaded
/// manifest; and the replacements, as (main checkout path, argument). Only an
/// argument with a directory part counts as a path, so `jsonl` stays a word.
fn main_checkout_args(
    spec: &CheckSpec,
    cwd: &Path,
    fallback: &crate::worktree::Fallback,
) -> (Vec<String>, Vec<(String, String)>) {
    let mut args = spec.run[1..].to_vec();
    let mut swapped = Vec::new();
    for arg in &mut args {
        if !arg.contains('/') || cwd.join(&*arg).exists() {
            continue;
        }
        let rel = match &spec.cwd {
            Some(c) => format!("{}/{arg}", c.trim_end_matches('/')),
            None => arg.clone(),
        };
        if !crate::config::below_root(Path::new(&rel)) {
            continue;
        }
        match fallback.file(&rel) {
            Ok(Some(main)) => {
                let main = main.to_string_lossy().into_owned();
                eprintln!(
                    "lab: check `{}`: this linked git worktree has no {rel}, which git ignores; \
                     using the main checkout's {main}",
                    spec.name
                );
                swapped.push((main.clone(), std::mem::replace(arg, main)));
            }
            Ok(None) => {}
            Err(e) => eprintln!("lab: check `{}`: {rel}: {e}", spec.name),
        }
    }
    (args, swapped)
}

/// A finding of lab itself, checked against the rule table in debug builds.
fn lab_finding(rule: &'static str, severity: Severity, subject: &str, message: String) -> Finding {
    crate::rules::debug_check(rule, severity);
    Finding::lab(rule, severity, subject, message)
}

/// Runs one checker in `root` (the project root, or a replayed commit's copy
/// of it) and parses its stdout. Its program resolves against the project
/// root, so a replay runs today's checker. With `fallback`, the root is a
/// linked git worktree whose main checkout supplies missing ignored files.
fn run_one(
    project: &Project,
    root: &Path,
    spec: &CheckSpec,
    version: Option<String>,
    fallback: Option<&crate::worktree::Fallback>,
) -> (CheckOutcome, Vec<Finding>, Vec<Measure>) {
    let cwd = spec
        .cwd
        .as_deref()
        .map_or_else(|| root.to_path_buf(), |c| root.join(c));
    let program = project.program(spec);
    let (args, swapped) = match fallback {
        Some(f) => main_checkout_args(spec, &cwd, f),
        None => (spec.run[1..].to_vec(), Vec::new()),
    };
    let started = Instant::now();
    let output = Command::new(&program)
        .args(&args)
        .current_dir(&cwd)
        .output();
    let duration_ms = started.elapsed().as_millis();
    let mut outcome = CheckOutcome {
        name: spec.name.clone(),
        command: spec.run.clone(),
        exit: None,
        status: "failed".into(),
        findings: 0,
        unparsed: 0,
        duration_ms,
        version,
        uncommitted_inputs: Vec::new(),
    };
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            let f = lab_finding(
                "check-failed",
                Severity::Error,
                &spec.name,
                format!("could not start `{}`: {e}", program.display()),
            );
            outcome.findings = 1;
            return (outcome, vec![f], Vec::new());
        }
    };
    outcome.exit = output.status.code();
    let mut findings = Vec::new();
    let mut measures = Vec::new();
    let mut first_unparsed = None;
    for (i, line) in String::from_utf8_lossy(&output.stdout).lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match Line::parse(line) {
            Ok(Line::Finding(mut f)) => {
                // Written as the check names it, so the fingerprint matches a main checkout's.
                for (main, arg) in &swapped {
                    f.subject = f.subject.replace(main.as_str(), arg);
                    f.message = f.message.replace(main.as_str(), arg);
                    for e in &mut f.evidence {
                        e.path = e.path.replace(main.as_str(), arg);
                    }
                }
                findings.push(f)
            }
            Ok(Line::Measure(m)) => measures.push(m),
            Err(e) => {
                outcome.unparsed += 1;
                first_unparsed.get_or_insert((i + 1, e));
            }
        }
    }
    if let Some((line, e)) = first_unparsed {
        // Output that is not rf.finding/1 may hide findings, so the run is incomplete.
        findings.push(lab_finding(
            "check-output-invalid",
            Severity::Error,
            &spec.name,
            format!(
                "{} stdout line(s) are not rf.finding/1 or rf.measure/1 records \
                 (first: line {line}: {e}); is the checker run with `--format jsonl`?",
                outcome.unparsed
            ),
        ));
    } else if matches!(outcome.exit, Some(0 | 1)) {
        // interfaces.md: 0 = no errors, 1 = errors found, anything else = the check itself failed.
        outcome.status = "ok".into();
        let errors = findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count();
        if outcome.exit == Some(1) && errors == 0 {
            findings.push(lab_finding(
                "check-exit-mismatch",
                Severity::Warn,
                &spec.name,
                "exited 1 but reported no error finding".into(),
            ));
        } else if outcome.exit == Some(0) && errors > 0 {
            findings.push(lab_finding(
                "check-exit-mismatch",
                Severity::Warn,
                &spec.name,
                format!("exited 0 but reported {errors} error finding(s)"),
            ));
        }
    }
    if !matches!(outcome.exit, Some(0 | 1)) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = lines[lines.len().saturating_sub(3)..].join(" | ");
        findings.push(lab_finding(
            "check-failed",
            Severity::Error,
            &spec.name,
            format!("exit {:?}: {tail}", outcome.exit),
        ));
    }
    outcome.findings = findings.len();
    // A check that did not run to completion measured an unknown part.
    if !outcome.ran() {
        measures.clear();
    }
    (outcome, findings, measures)
}

/// Fingerprints that appeared or disappeared relative to the baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct Delta {
    pub new: BTreeSet<String>,
    pub resolved: BTreeSet<String>,
}

pub struct Report {
    /// The run; when it was not stored, its `path` is the stored run it repeats.
    pub run: Run,
    /// `None` when the run was stored. When it repeats the newest stored run
    /// (see `repeats`) and was not stored, the number of findings that are
    /// in one of the two runs and not in the other.
    pub not_stored: Option<usize>,
    /// Per check that ran: the change since that check's baseline, or `None`
    /// when it has no baseline.
    pub delta: BTreeMap<String, Option<Delta>>,
}

/// Runs git in the project root with a stable message language. `None` when
/// git is not installed or the root is not in a git repository.
fn git(project: &Project, args: &[&str]) -> Result<Option<std::process::Output>, String> {
    let out = match Command::new("git")
        .args(args)
        .current_dir(&project.root)
        .env("LC_ALL", "C")
        .output()
    {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("git: {e}")),
    };
    if !out.status.success()
        && String::from_utf8_lossy(&out.stderr).contains("not a git repository")
    {
        return Ok(None);
    }
    Ok(Some(out))
}

/// A pathspec that leaves out the state directory: tool-owned state, such as
/// a run's own file, is not checked text. A state directory outside the root
/// is outside the `.` pathspec anyway.
fn state_exclude(project: &Project) -> Option<String> {
    project
        .state_dir
        .strip_prefix(&project.root)
        .ok()
        .filter(|p| crate::config::below_root(p))
        .map(|p| format!(":(exclude,literal){}", p.display()))
}

/// The `HEAD` commit of the git repository holding the project root, and
/// whether the files under the root differ from it, leaving out the state
/// directory; `(None, None)` when git is missing, the root is not in a
/// repository, or the repository has no commit yet. Any other git failure is
/// an error, for the caller to report.
fn git_state(project: &Project) -> Result<(Option<String>, Option<bool>), String> {
    let failed = |args: &[&str], out: &std::process::Output| {
        format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )
    };
    let exclude = state_exclude(project);
    // No optional locks, so a background run never blocks the user's own git
    // commands; untracked files are listed whatever the user's config says.
    let mut args = vec![
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "--branch",
        "--untracked-files=normal",
        "--",
        ".",
    ];
    args.extend(exclude.as_deref());
    let Some(out) = git(project, &args)? else {
        return Ok((None, None));
    };
    if !out.status.success() {
        return Err(failed(&args, &out));
    }
    let status = String::from_utf8_lossy(&out.stdout);
    let commit = status
        .lines()
        .find_map(|l| l.strip_prefix("# branch.oid "))
        .filter(|oid| *oid != "(initial)");
    let Some(commit) = commit else {
        return Ok((None, None));
    };
    let mut dirty = status.lines().any(|l| !l.starts_with('#'));
    if !dirty {
        // An ignored root lists no changes, yet no commit holds its files.
        let args = ["check-ignore", "--quiet", "."];
        let out = git(project, &args)?.ok_or("git: the repository disappeared")?;
        dirty = match out.status.code() {
            Some(0) => true,
            Some(1) => false,
            _ => return Err(failed(&args, &out)),
        };
    }
    Ok((Some(commit.to_string()), Some(dirty)))
}

/// The configured checks named by `--only` (all when empty); every name must exist.
pub fn select<'a>(project: &'a Project, only: &[String]) -> Result<Vec<&'a CheckSpec>, String> {
    if project.lab.checks.is_empty() {
        return Err("no checks configured: add [[lab.checks]] to research.toml".into());
    }
    let unknown: Vec<&str> = only
        .iter()
        .filter(|n| !project.lab.checks.iter().any(|c| &c.name == *n))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        return Err(format!("no configured check is named {unknown:?}"));
    }
    Ok(project
        .lab
        .checks
        .iter()
        .filter(|s| only.is_empty() || only.contains(&s.name))
        .collect())
}

/// A run's checks, before it is stored.
pub(crate) struct Ran {
    pub outcomes: Vec<CheckOutcome>,
    pub items: Vec<Item>,
    pub measures: Vec<MeasureItem>,
}

/// Runs `specs` in `root`, each with its version from `versions` and, for a
/// replay, the uncommitted inputs linked for it.
pub(crate) fn run_checks(
    project: &Project,
    root: &Path,
    specs: &[&CheckSpec],
    versions: &BTreeMap<String, Option<String>>,
    linked: &BTreeMap<String, Vec<String>>,
    fallback: Option<&crate::worktree::Fallback>,
) -> Ran {
    let mut ran = Ran {
        outcomes: Vec::new(),
        items: Vec::new(),
        measures: Vec::new(),
    };
    for spec in specs {
        let version = versions.get(&spec.name).cloned().flatten();
        let (mut outcome, findings, measured) = run_one(project, root, spec, version, fallback);
        outcome.uncommitted_inputs = linked.get(&spec.name).cloned().unwrap_or_default();
        ran.measures
            .extend(measured.into_iter().map(|measure| MeasureItem {
                schema: MEASURE_ITEM_SCHEMA.into(),
                check: spec.name.clone(),
                measure,
            }));
        let fps = finding::fingerprints(&findings);
        ran.items
            .extend(findings.into_iter().zip(fps).map(|(f, fingerprint)| Item {
                schema: ITEM_SCHEMA.into(),
                check: spec.name.clone(),
                fingerprint,
                finding: f,
            }));
        ran.outcomes.push(outcome);
    }
    ran
}

/// Stores a run under a new file name and returns it. Only a clean run can
/// end an interval, so only it keeps the measure keys, which can be most of
/// a run file.
pub(crate) fn store_run(project: &Project, mut header: RunHeader, ran: Ran) -> Result<Run, String> {
    let Ran {
        outcomes,
        items,
        mut measures,
    } = ran;
    if header.dirty != Some(false) {
        for m in &mut measures {
            m.measure.keys = None;
        }
    }
    header.checks = outcomes;
    let path = store(project, &header, &items, &measures)?;
    Ok(Run {
        path,
        header,
        items,
        measures,
    })
}

/// Longest version line kept.
const MAX_VERSION_CHARS: usize = 200;
/// How long a version command may run.
const VERSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Programs whose `--version` names an interpreter, not the checker it runs.
/// Names starting with `python` count too, such as `python3.12`.
const INTERPRETERS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "fish", "env", "uv", "uvx", "pipx", "poetry", "pdm",
    "hatch", "conda", "node", "npm", "npx", "pnpm", "yarn", "bun", "bunx", "deno", "perl", "ruby",
    "bundle", "cargo", "go", "java", "make", "just", "timeout", "nice", "nohup", "time", "xargs",
    "docker", "podman",
];

/// The command that prints a check's version: its `version`, or, when that is
/// not set, `<program> --version` for a program named without a directory
/// part that is not an interpreter. `None` when nothing tells the version.
fn version_command(spec: &CheckSpec) -> Option<Vec<String>> {
    if let Some(argv) = &spec.version {
        return (!argv.is_empty()).then(|| argv.clone());
    }
    let program = &spec.run[0];
    let bare = Path::new(program).components().count() == 1 && !program.contains('/');
    let interpreter = INTERPRETERS.contains(&program.as_str()) || program.starts_with("python");
    (bare && !interpreter).then(|| vec![program.clone(), "--version".into()])
}

/// Each check's version: the first line its version command prints, run in
/// `root` (the project root, or a replayed commit's copy of it, where the
/// check runs) with its program resolved as the check's is. A command that
/// fails, times out or prints no short, non-JSON line gives `None`, with a
/// warning on stderr.
pub(crate) fn versions(
    project: &Project,
    root: &Path,
    specs: &[&CheckSpec],
) -> BTreeMap<String, Option<String>> {
    specs
        .iter()
        .map(|spec| {
            let version = version_command(spec).and_then(|argv| {
                version_of(project, root, &argv)
                    .map_err(|e| {
                        eprintln!(
                            "lab: check `{}`: no version from `{}`: {e}; set `version` in its \
                             [[lab.checks]] entry, or `version = []` for none",
                            spec.name,
                            argv.join(" ")
                        );
                    })
                    .ok()
            });
            (spec.name.clone(), version)
        })
        .collect()
}

fn version_of(project: &Project, root: &Path, argv: &[String]) -> Result<String, String> {
    use std::io::Read as _;
    use std::os::unix::process::CommandExt as _;
    use std::process::Stdio;
    let mut child = Command::new(project.resolve_program(&argv[0]))
        .args(&argv[1..])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("could not start: {e}"))?;
    // A thread drains stdout, so a program that prints much never blocks on
    // a full pipe; one whose output a background process keeps open after it
    // exits is given up on.
    let mut out = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = tx.send(out.read_to_end(&mut bytes).map(|_| bytes));
    });
    let status = match crate::lesson_loop::wait_for(&mut child, VERSION_TIMEOUT)
        .map_err(|e| e.to_string())?
    {
        crate::lesson_loop::Ended::Exit(status) => status,
        crate::lesson_loop::Ended::TimedOut => {
            return Err(format!("timed out after {} s", VERSION_TIMEOUT.as_secs()));
        }
    };
    let stdout = match rx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(Ok(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
        Ok(Err(e)) => return Err(format!("its output: {e}")),
        Err(_) => return Err("its output stayed open after it exited".into()),
    };
    if !status.success() {
        return Err(format!("it exited with {status}"));
    }
    let line = stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or("it printed nothing")?;
    if line.chars().count() > MAX_VERSION_CHARS || line.starts_with('{') {
        return Err("its first line is not a version".into());
    }
    Ok(line.to_string())
}

pub fn run(project: &Project, only: &[String]) -> Result<Report, String> {
    let specs = select(project, only)?;
    let baseline = Baseline::load(project)?;
    let at = finding::now();
    let (commit, dirty) = git_state(project).unwrap_or_else(|e| {
        eprintln!("lab: the run records no commit: {e}");
        (None, None)
    });
    let versions = versions(project, &project.root, &specs);
    // A linked git worktree lacks the main checkout's ignored files, such as downloads.
    let fallback = crate::worktree::Fallback::find(&project.root).unwrap_or_else(|e| {
        eprintln!("lab: ignored files are not looked up in a main checkout: {e}");
        None
    });
    let ran = run_checks(
        project,
        &project.root,
        &specs,
        &versions,
        &BTreeMap::new(),
        fallback.as_ref(),
    );
    if let Some(fallback) = &fallback {
        let lacking = lacking_inputs(project, &specs, &ran.items, fallback)?;
        if !lacking.is_empty() {
            let shown: Vec<&str> = lacking.iter().take(3).map(String::as_str).collect();
            return Err(format!(
                "this linked git worktree lacks {} git-ignored file(s) that the findings name and \
                 the main checkout {} has, such as {}. The findings would differ from the main \
                 checkout's, so no run is stored. Copy the main checkout's git-ignored files into \
                 this worktree (a project may have a step that seeds them), then run the check again",
                lacking.len(),
                fallback.main().display(),
                shown.join(", ")
            ));
        }
    }
    let header = RunHeader {
        schema: RUN_SCHEMA.into(),
        at,
        commit,
        dirty,
        replayed: false,
        checks: Vec::new(),
    };
    let same = repeats(project, &header, &ran).unwrap_or_else(|e| {
        eprintln!("lab: the run is stored, since the newest stored run could not be read: {e}");
        None
    });
    let (run, not_stored) = match same {
        Some((path, differing)) => {
            let Ran {
                outcomes,
                items,
                measures,
            } = ran;
            let header = RunHeader {
                checks: outcomes,
                ..header
            };
            let run = Run {
                path,
                header,
                items,
                measures,
            };
            (run, Some(differing))
        }
        None => (store_run(project, header, ran)?, None),
    };
    let mut delta = BTreeMap::new();
    for outcome in run.header.checks.iter().filter(|c| c.ran()) {
        let entry = baseline.checks.get(&outcome.name).map(|b| {
            let before: BTreeSet<&str> = b.fingerprints.iter().map(String::as_str).collect();
            let after = run.fingerprints(&outcome.name);
            Delta {
                new: after.difference(&before).map(|s| s.to_string()).collect(),
                resolved: before.difference(&after).map(|s| s.to_string()).collect(),
            }
        });
        delta.insert(outcome.name.clone(), entry);
    }
    Ok(Report {
        run,
        delta,
        not_stored,
    })
}

/// The newest stored live run and the number of info findings that differ
/// from it, when this clean run repeats it: that run is clean too, dated the
/// same day in the project's zone, git finds no difference between the two
/// commits outside the state directory, the same checks (names, commands,
/// exits, statuses and versions, in order) measured the same units, and
/// their error and warning findings are equal. Such a run checks no new
/// text, and storing it would leave a file that needs one more commit, and
/// so one more check: a check of a merged head whose own run is committed
/// is one.
///
/// Only info findings may differ, such as a checkout path that a checker
/// prints; the stored run stays the record of that text. `lab baseline set`
/// and `--new-only` read errors and warnings, which are equal. `lab evaluate`
/// counts the same new units: the interval between the two runs has none, so
/// it is left out, and the next interval starts from the same measures. The
/// same day keeps every run on the same side of an adoption date. Any doubt,
/// such as a commit git cannot compare, stores the run.
/// The files that findings name, relative to the project root, that this
/// linked worktree lacks and its main checkout has, where git ignores them: a
/// source text that only the main checkout downloaded, for example. A checker
/// that reports such a file as missing found something the main checkout
/// would not, so the run is not the project's record of its text. A file a
/// check's argument names does not count: the check read the main
/// checkout's copy (`main_checkout_args`).
fn lacking_inputs(
    project: &Project,
    specs: &[&CheckSpec],
    items: &[Item],
    fallback: &crate::worktree::Fallback,
) -> Result<BTreeSet<String>, String> {
    let root = &project.root;
    let by_name: BTreeMap<&str, (Option<&str>, BTreeSet<String>)> = specs
        .iter()
        .map(|s| {
            let cwd = s.cwd.as_deref();
            let args = s.run[1..]
                .iter()
                .filter(|a| a.contains('/'))
                .filter_map(|a| crate::change::relative(root, cwd, a))
                .collect();
            (s.name.as_str(), (cwd, args))
        })
        .collect();
    let mut absent = BTreeSet::new();
    for item in items {
        let (cwd, args) = by_name
            .get(item.check.as_str())
            .map_or((None, None), |(c, a)| (*c, Some(a)));
        absent.extend(
            crate::change::named(root, cwd, &item.finding)
                .into_iter()
                .filter(|p| !args.is_some_and(|a| a.contains(p)) && !root.join(p).exists()),
        );
    }
    fallback
        .resolve(absent.iter().map(String::as_str))
        .map_err(|e| {
            format!("cannot tell whether this worktree lacks the main checkout's files: {e}")
        })
}

/// The header of a stored run, without reading the rest of the file.
fn read_header(path: &Path) -> Result<RunHeader, String> {
    use std::io::BufRead as _;
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
        if !line.trim().is_empty() {
            return serde_json::from_str(&line).map_err(|e| format!("{}:1: {e}", path.display()));
        }
    }
    Err(format!("{}: empty check run", path.display()))
}

fn repeats(
    project: &Project,
    header: &RunHeader,
    ran: &Ran,
) -> Result<Option<(PathBuf, usize)>, String> {
    let (Some(commit), Some(false)) = (header.commit.as_deref(), header.dirty) else {
        return Ok(None);
    };
    // Headers only, so replays stored after the newest live run are not read
    // in full.
    let mut newest = None;
    for path in run_paths(project)?.iter().rev() {
        if !read_header(path)?.replayed {
            newest = Run::read(path, false)?;
            break;
        }
    }
    let Some(prev) = newest else {
        return Ok(None);
    };
    let (Some(prev_commit), Some(false)) = (prev.header.commit.as_deref(), prev.header.dirty)
    else {
        return Ok(None);
    };
    if project.date_of(&prev.header.at)? != project.date_of(&header.at)? {
        return Ok(None);
    }
    let same_check = |a: &CheckOutcome, b: &CheckOutcome| {
        a.name == b.name
            && a.command == b.command
            && a.exit == b.exit
            && a.status == b.status
            && a.version == b.version
            && a.uncommitted_inputs == b.uncommitted_inputs
    };
    let same_checks = prev.header.checks.len() == ran.outcomes.len()
        && prev
            .header
            .checks
            .iter()
            .zip(&ran.outcomes)
            .all(|(a, b)| same_check(a, b));
    if !same_checks || prev.measures != ran.measures {
        return Ok(None);
    }
    if prev_commit != commit {
        let mut args = vec![
            "--no-optional-locks",
            "diff",
            "--quiet",
            "--no-ext-diff",
            "--no-textconv",
            prev_commit,
            commit,
            "--",
            ".",
        ];
        let exclude = state_exclude(project);
        args.extend(exclude.as_deref());
        // Exit 0: no difference. 1: a difference; anything else, such as a
        // commit this repository lacks, cannot show the text is the same.
        let same_text = git(project, &args)?.is_some_and(|out| out.status.code() == Some(0));
        if !same_text {
            return Ok(None);
        }
    }
    // Findings counted by fingerprint, one side positive and the other
    // negative; any error or warning left over stores the run.
    let mut balance: BTreeMap<(&str, &str), (i64, Severity)> = BTreeMap::new();
    for (items, sign) in [(&ran.items, 1), (&prev.items, -1)] {
        for i in items {
            let entry = balance
                .entry((&i.check, &i.fingerprint))
                .or_insert((0, i.finding.severity));
            entry.0 += sign;
        }
    }
    let mut differing = 0;
    for (n, severity) in balance.values() {
        if *n != 0 {
            if *severity != Severity::Info {
                return Ok(None);
            }
            differing += n.unsigned_abs() as usize;
        }
    }
    Ok(Some((prev.path, differing)))
}

pub const BASELINE_SCHEMA: &str = "rf.labbaseline/1";

/// The accepted findings of each check: `lab check --new-only` fails only on
/// errors outside them. Set explicitly with `lab baseline set`, never by `lab check`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Baseline {
    pub schema: String,
    pub checks: BTreeMap<String, BaselineEntry>,
    /// Every `lab baseline set`, oldest first.
    #[serde(default)]
    pub history: Vec<BaselineChange>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BaselineEntry {
    /// File name of the stored run the fingerprints come from.
    pub run: String,
    pub at: String,
    pub fingerprints: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BaselineChange {
    pub at: String,
    pub checks: Vec<String>,
    pub note: String,
}

impl Baseline {
    /// The project's baseline; empty when none was set.
    pub fn load(project: &Project) -> Result<Self, String> {
        let path = project.baseline_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    schema: BASELINE_SCHEMA.into(),
                    ..Self::default()
                });
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let b: Self =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if b.schema != BASELINE_SCHEMA {
            return Err(format!(
                "{}: not a {BASELINE_SCHEMA} record",
                path.display()
            ));
        }
        Ok(b)
    }

    /// Accepts, for each selected check, the findings of the latest stored run
    /// in which it ran to completion.
    pub fn set(
        &mut self,
        runs: &[Run],
        specs: &[&CheckSpec],
        note: &str,
    ) -> Result<Vec<String>, String> {
        if note.trim().is_empty() {
            return Err("setting a baseline needs a note saying why".into());
        }
        let mut names = Vec::new();
        for spec in specs {
            let run = runs
                .iter()
                .rev()
                .find(|r| {
                    r.header
                        .checks
                        .iter()
                        .any(|c| c.name == spec.name && c.ran())
                })
                .ok_or_else(|| {
                    format!(
                        "check `{}` has no completed run; run `lab check` first",
                        spec.name
                    )
                })?;
            self.checks.insert(
                spec.name.clone(),
                BaselineEntry {
                    run: run.name(),
                    at: run.header.at.clone(),
                    fingerprints: run
                        .fingerprints(&spec.name)
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                },
            );
            names.push(spec.name.clone());
        }
        self.history.push(BaselineChange {
            at: finding::now(),
            checks: names.clone(),
            note: note.trim().into(),
        });
        Ok(names)
    }

    pub fn save(&self, project: &Project) -> Result<(), String> {
        let path = project.baseline_path();
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let tmp = dir.join(format!(".baseline-{}.tmp", std::process::id()));
        let body = serde_json::to_string_pretty(self).map_err(|e| e.to_string())? + "\n";
        std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Writes the run atomically under a new, sortable file name.
fn store(
    project: &Project,
    header: &RunHeader,
    items: &[Item],
    measures: &[MeasureItem],
) -> Result<PathBuf, String> {
    let dir = project.checks_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stamp = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%S%.3fZ")
        .to_string();
    let mut body = serde_json::to_string(header).map_err(|e| e.to_string())? + "\n";
    for item in items {
        body += &(serde_json::to_string(item).map_err(|e| e.to_string())? + "\n");
    }
    for m in measures {
        body += &(serde_json::to_string(m).map_err(|e| e.to_string())? + "\n");
    }
    let tmp = dir.join(format!(".{stamp}-{}.tmp", std::process::id()));
    let mut file = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    file.write_all(body.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    drop(file);
    let mut result = Err(format!(
        "{}: could not allocate a run file name",
        dir.display()
    ));
    for n in 0..100 {
        let path = dir.join(if n == 0 {
            format!("{stamp}.jsonl")
        } else {
            format!("{stamp}-{n}.jsonl")
        });
        // A hard link fails if the target exists, so concurrent runs never overwrite each other.
        match std::fs::hard_link(&tmp, &path) {
            Ok(()) => {
                result = Ok(path);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                result = Err(format!("{}: {e}", path.display()));
                break;
            }
        }
    }
    std::fs::remove_file(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &Path, checks: &str) -> Project {
        std::fs::write(dir.join("research.toml"), checks).unwrap();
        Project::discover(dir, None).unwrap()
    }

    #[test]
    fn runs_stored_before_the_commit_fields_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.jsonl");
        std::fs::write(
            &path,
            r#"{"schema":"rf.labcheck/1","at":"2026-11-01T00:00:00Z","checks":[]}"#,
        )
        .unwrap();
        let run = Run::load(&path).unwrap();
        assert!(run.measures.is_empty());
        assert_eq!((run.header.commit, run.header.dirty), (None, None));
        assert!(!run.header.replayed);
        // A check outcome stored before lab 0.9.0 has no version.
        std::fs::write(
            &path,
            r#"{"schema":"rf.labcheck/1","at":"2026-11-01T00:00:00Z","checks":[{"name":"c","command":["c"],"exit":0,"status":"ok","findings":0,"unparsed":0,"duration_ms":1}]}"#,
        )
        .unwrap();
        let c = &Run::load(&path).unwrap().header.checks[0];
        assert_eq!(
            (c.version.as_deref(), c.uncommitted_inputs.len()),
            (None, 0)
        );
    }

    fn spec(run: &[&str], version: Option<&[&str]>) -> CheckSpec {
        CheckSpec {
            name: "c".into(),
            run: run.iter().map(|s| (*s).to_string()).collect(),
            cwd: None,
            version: version.map(|v| v.iter().map(|s| (*s).to_string()).collect()),
            uncommitted_inputs: Vec::new(),
        }
    }

    #[test]
    fn the_version_command_defaults_to_a_named_programs_own_flag() {
        let cmd = |run: &[&str], version| version_command(&spec(run, version));
        assert_eq!(
            cmd(&["claimcheck", "check"], None),
            Some(vec!["claimcheck".to_string(), "--version".to_string()])
        );
        // A script or an interpreter does not tell the checker's version.
        assert_eq!(cmd(&["scripts/check.sh"], None), None);
        assert_eq!(cmd(&["/usr/local/bin/claimcheck"], None), None);
        assert_eq!(cmd(&["sh", "-c", "claimcheck check"], None), None);
        assert_eq!(cmd(&["npm", "run", "check"], None), None);
        assert_eq!(cmd(&["python3.12", "check.py"], None), None);
        assert_eq!(cmd(&["claimcheck"], Some(&[])), None);
        assert_eq!(
            cmd(&["sh", "-c", "x"], Some(&["x", "-V"])),
            Some(vec!["x".to_string(), "-V".to_string()])
        );
    }

    #[test]
    fn each_check_records_its_version_or_null() {
        let dir = tempfile::tempdir().unwrap();
        let checks = emit_script(dir.path(), &[FINDING_B], 0, false);
        let p = project(
            dir.path(),
            &format!(
                "{checks}version = [\"sh\", \"-c\", \"echo; echo '  tool 1.2  '; echo more\"]\n\
                 [[lab.checks]]\nname = \"none\"\nrun = [\"sh\", \"-c\", \"exit 0\"]\n\
                 [[lab.checks]]\nname = \"bad\"\nrun = [\"sh\", \"-c\", \"exit 0\"]\nversion = [\"sh\", \"-c\", \"echo 2.0; exit 3\"]\n\
                 [[lab.checks]]\nname = \"json\"\nrun = [\"sh\", \"-c\", \"exit 0\"]\nversion = [\"sh\", \"-c\", \"echo '{{}}'\"]\n\
                 [[lab.checks]]\nname = \"big\"\nrun = [\"sh\", \"-c\", \"exit 0\"]\nversion = [\"sh\", \"-c\", \"echo 4.0; yes x | head -c 300000\"]\n\
                 [[lab.checks]]\nname = \"held\"\nrun = [\"sh\", \"-c\", \"exit 0\"]\nversion = [\"sh\", \"-c\", \"echo 5.0; (sleep 30; echo late) &\"]\n"
            ),
        );
        let started = Instant::now();
        let report = run(&p, &[]).unwrap();
        // Neither much output nor a background process holding it open
        // blocks the run.
        assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
        let versions: Vec<Option<&str>> = report
            .run
            .header
            .checks
            .iter()
            .map(|c| c.version.as_deref())
            .collect();
        assert_eq!(
            versions,
            [Some("tool 1.2"), None, None, None, Some("4.0"), None]
        );
        let stored = std::fs::read_to_string(&report.run.path).unwrap();
        let header: serde_json::Value =
            serde_json::from_str(stored.lines().next().unwrap()).unwrap();
        assert_eq!(header["checks"][0]["version"], "tool 1.2");
        assert!(header["checks"][1]["version"].is_null(), "{header}");
        assert!(header.get("replayed").is_none(), "{header}");
        assert!(header["checks"][0].get("uncommitted_inputs").is_none());
    }

    #[test]
    fn run_lines_of_another_schema_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.jsonl");
        let item = format!(
            r#"{{"schema":"{ITEM_SCHEMA}","check":"c","fingerprint":"f","finding":{FINDING_A}}}"#
        );
        let later = r#"{"schema":"rf.labcheck.later/1","check":"c","count":3}"#;
        let header = r#"{"schema":"rf.labcheck/1","at":"2026-11-01T00:00:00Z","checks":[]}"#;
        std::fs::write(&path, [header, &item, later].join("\n")).unwrap();
        assert_eq!(Run::load(&path).unwrap().items.len(), 1);
        std::fs::write(&path, [header, r#"{"check":"c"}"#].join("\n")).unwrap();
        assert!(Run::load(&path).unwrap_err().contains("no schema"));
    }

    const FINDING_A: &str = r#"{"schema":"rf.finding/1","tool":"t","rule":"r","severity":"error","subject":"a","message":"m"}"#;
    const FINDING_B: &str = r#"{"schema":"rf.finding/1","tool":"t","rule":"r","severity":"warn","subject":"b","message":"m"}"#;

    fn emit_script(dir: &Path, lines: &[&str], exit: i32, garbage: bool) -> String {
        let out = dir.join("out.txt");
        let tail = if garbage { "\nnot json\n" } else { "\n" };
        std::fs::write(&out, lines.join("\n") + tail).unwrap();
        format!(
            "[[lab.checks]]\nname = \"x\"\nrun = [\"sh\", \"-c\", \"cat '{}'; exit {exit}\"]\n",
            out.display()
        )
    }

    #[test]
    fn deltas_are_relative_to_the_baseline_not_the_last_run() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), &emit_script(dir.path(), &[FINDING_A], 1, false));
        let first = run(&p, &[]).unwrap();
        assert_eq!(first.run.header.checks[0].findings, 1);
        assert_eq!(first.delta["x"], None, "no baseline yet");

        let mut b = Baseline::load(&p).unwrap();
        assert!(
            b.set(&history(&p).unwrap(), &select(&p, &[]).unwrap(), " ")
                .is_err()
        );
        b.set(
            &history(&p).unwrap(),
            &select(&p, &[]).unwrap(),
            "accept backlog",
        )
        .unwrap();
        b.save(&p).unwrap();

        let p = project(dir.path(), &emit_script(dir.path(), &[FINDING_B], 0, false));
        for _ in 0..2 {
            // Re-running does not move the baseline, so B stays new and A resolved.
            let report = run(&p, &[]).unwrap();
            let delta = report.delta["x"].clone().unwrap();
            assert_eq!((delta.new.len(), delta.resolved.len()), (1, 1));
        }
        assert_eq!(history(&p).unwrap().len(), 3);
        let saved = Baseline::load(&p).unwrap();
        assert_eq!(saved.history.len(), 1);
        assert_eq!(saved.history[0].note, "accept backlog");
    }

    #[test]
    fn output_that_is_not_findings_fails_the_check() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), &emit_script(dir.path(), &[FINDING_B], 0, true));
        let report = run(&p, &[]).unwrap();
        let c = &report.run.header.checks[0];
        assert_eq!((c.ran(), c.unparsed), (false, 1));
        assert!(
            report
                .run
                .items
                .iter()
                .any(|i| i.finding.rule == "check-output-invalid"
                    && i.finding.severity == Severity::Error)
        );
    }

    #[test]
    fn measures_are_stored_and_loaded_not_counted_as_unparsed() {
        let dir = tempfile::tempdir().unwrap();
        let measure = r#"{"schema":"rf.measure/1","tool":"t","unit":"sentence","count":3,"keys":["00000000000000aa","00000000000000bb"]}"#;
        let p = project(
            dir.path(),
            &emit_script(dir.path(), &[FINDING_B, measure], 0, false),
        );
        let report = run(&p, &[]).unwrap();
        let c = &report.run.header.checks[0];
        assert_eq!((c.ran(), c.unparsed, c.findings), (true, 0, 1));
        let stored = Run::load(&report.run.path).unwrap();
        assert_eq!(stored.measures.len(), 1);
        let m = &stored.measures[0];
        assert_eq!(
            (m.schema.as_str(), m.check.as_str()),
            (MEASURE_ITEM_SCHEMA, "x")
        );
        assert_eq!((m.measure.unit.as_str(), m.measure.count), ("sentence", 3));
        assert_eq!(m.measure.keys, None);
        // The temporary directory is not a git repository, so the run is
        // not clean and keeps no keys.
        assert_eq!(report.run.header.dirty, None);
    }

    #[test]
    fn a_check_stores_several_units_for_one_tool() {
        let dir = tempfile::tempdir().unwrap();
        let citing = r#"{"schema":"rf.measure/1","tool":"t","unit":"citing-sentence","count":3}"#;
        let uncited = r#"{"schema":"rf.measure/1","tool":"t","unit":"uncited-sentence","count":5}"#;
        let p = project(
            dir.path(),
            &emit_script(dir.path(), &[FINDING_B, citing, uncited], 0, false),
        );
        let report = run(&p, &[]).unwrap();
        assert_eq!(report.run.header.checks[0].unparsed, 0);
        let stored = Run::load(&report.run.path).unwrap();
        let units: Vec<(&str, u64)> = stored
            .measures
            .iter()
            .map(|m| (m.measure.unit.as_str(), m.measure.count))
            .collect();
        assert_eq!(units, [("citing-sentence", 3), ("uncited-sentence", 5)]);
    }

    #[test]
    fn a_failed_check_stores_no_measure() {
        let dir = tempfile::tempdir().unwrap();
        let measure = r#"{"schema":"rf.measure/1","tool":"t","unit":"sentence","count":3}"#;
        let p = project(
            dir.path(),
            &emit_script(dir.path(), &[FINDING_B, measure], 2, false),
        );
        let report = run(&p, &[]).unwrap();
        assert!(!report.run.header.checks[0].ran());
        assert!(report.run.measures.is_empty());
    }

    #[test]
    fn invalid_measures_fail_the_check() {
        for bad in [
            r#"{"schema":"rf.measure/1","tool":"t","unit":"","count":1}"#,
            r#"{"schema":"rf.measure/2","tool":"t","unit":"u","count":1}"#,
            r#"{"schema":"rf.measure/1","tool":"t","unit":"u","count":1,"keys":["XYZ"]}"#,
            r#"{"schema":"rf.measure/1","tool":"t","unit":"u","count":2,"keys":["00000000000000bb","00000000000000aa"]}"#,
            r#"{"schema":"rf.measure/1","tool":"t","unit":"u","count":1,"keys":["00000000000000aa","00000000000000bb"]}"#,
            r#"{"schema":"rf.measure/1","tool":"t","unit":"u","count":-1}"#,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let p = project(dir.path(), &emit_script(dir.path(), &[bad], 0, false));
            let report = run(&p, &[]).unwrap();
            let c = &report.run.header.checks[0];
            assert_eq!((c.ran(), c.unparsed), (false, 1), "{bad}");
            assert!(report.run.measures.is_empty(), "{bad}");
        }
    }

    #[test]
    fn unknown_only_names_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), &emit_script(dir.path(), &[FINDING_B], 0, false));
        let err = run(&p, &["x".into(), "typo".into()]).err().unwrap();
        assert!(err.contains("typo") && !err.contains("\"x\""), "{err}");
    }

    #[test]
    fn collision_files_sort_after_the_original() {
        let mut v = [
            PathBuf::from("20261107T000000.000Z-1.jsonl"),
            PathBuf::from("20261107T000000.000Z-10.jsonl"),
            PathBuf::from("20261107T000000.000Z.jsonl"),
            PathBuf::from("20261107T000000.000Z-2.jsonl"),
        ];
        v.sort_by_cached_key(|p| run_order(p));
        let names: Vec<_> = v.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        assert_eq!(
            names,
            [
                "20261107T000000.000Z.jsonl",
                "20261107T000000.000Z-1.jsonl",
                "20261107T000000.000Z-2.jsonl",
                "20261107T000000.000Z-10.jsonl"
            ]
        );
    }

    #[test]
    fn a_crashing_or_missing_checker_is_an_error_finding() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(
            dir.path(),
            "[[lab.checks]]\nname = \"boom\"\nrun = [\"sh\", \"-c\", \"echo one >&2; echo two >&2; echo three >&2; echo four >&2; exit 2\"]\n\
             [[lab.checks]]\nname = \"gone\"\nrun = [\"/nonexistent/checker\"]\n",
        );
        let report = run(&p, &[]).unwrap();
        assert!(report.run.header.checks.iter().all(|c| !c.ran()));
        assert_eq!(report.run.items.len(), 2);
        assert!(
            report
                .run
                .items
                .iter()
                .all(|i| i.finding.rule == "check-failed")
        );
        assert!(report.delta.is_empty());
        assert!(report.run.header.checks.iter().all(|c| c.findings == 1));
        let boom = &report.run.items[0].finding.message;
        assert!(boom.ends_with("two | three | four"), "{boom}");
    }

    #[test]
    fn exit_one_without_errors_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), &emit_script(dir.path(), &[FINDING_B], 1, false));
        let report = run(&p, &[]).unwrap();
        assert!(
            report
                .run
                .items
                .iter()
                .any(|i| i.finding.rule == "check-exit-mismatch")
        );
    }

    #[test]
    fn exit_zero_with_errors_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), &emit_script(dir.path(), &[FINDING_A], 0, false));
        let report = run(&p, &[]).unwrap();
        let outcome = &report.run.header.checks[0];
        assert_eq!(outcome.status, "ok", "the check still ran to completion");
        let mismatch: Vec<_> = report
            .run
            .items
            .iter()
            .filter(|i| i.finding.rule == "check-exit-mismatch")
            .collect();
        assert_eq!(mismatch.len(), 1);
        assert_eq!(
            mismatch[0].finding.message,
            "exited 0 but reported 1 error finding(s)"
        );
    }
}
