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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    pub fn load(path: &Path) -> Result<Self, String> {
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
        Ok(Self {
            path: path.to_path_buf(),
            header,
            items,
            measures,
        })
    }

    fn fingerprints(&self, check: &str) -> BTreeSet<&str> {
        self.items
            .iter()
            .filter(|i| i.check == check)
            .map(|i| i.fingerprint.as_str())
            .collect()
    }
}

/// Stored runs, oldest first. A missing directory means no history.
pub fn history(project: &Project) -> Result<Vec<Run>, String> {
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
    paths.iter().map(|p| Run::load(p)).collect()
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

/// Runs one checker and parses its stdout.
fn run_one(project: &Project, spec: &CheckSpec) -> (CheckOutcome, Vec<Finding>, Vec<Measure>) {
    let cwd = spec
        .cwd
        .as_deref()
        .map_or_else(|| project.root.clone(), |c| project.path(c));
    let program = project.program(spec);
    let started = Instant::now();
    let output = Command::new(&program)
        .args(&spec.run[1..])
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
    };
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            let f = Finding::lab(
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
            Ok(Line::Finding(f)) => findings.push(f),
            Ok(Line::Measure(m)) => measures.push(m),
            Err(e) => {
                outcome.unparsed += 1;
                first_unparsed.get_or_insert((i + 1, e));
            }
        }
    }
    if let Some((line, e)) = first_unparsed {
        // Output that is not rf.finding/1 may hide findings, so the run is incomplete.
        findings.push(Finding::lab(
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
        if outcome.exit == Some(1) && !findings.iter().any(|f| f.severity == Severity::Error) {
            findings.push(Finding::lab(
                "check-exit-mismatch",
                Severity::Warn,
                &spec.name,
                "exited 1 but reported no error finding".into(),
            ));
        }
    }
    if !matches!(outcome.exit, Some(0 | 1)) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = lines[lines.len().saturating_sub(3)..].join(" | ");
        findings.push(Finding::lab(
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
    pub run: Run,
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
    // Tool-owned state, such as this run's own file, is not checked text. A
    // state directory outside the root is outside the pathspec anyway.
    let exclude = project
        .state_dir
        .strip_prefix(&project.root)
        .ok()
        .filter(|p| {
            p.components().next().is_some()
                && p.components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
        })
        .map(|p| format!(":(exclude,literal){}", p.display()));
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

pub fn run(project: &Project, only: &[String]) -> Result<Report, String> {
    let specs = select(project, only)?;
    let baseline = Baseline::load(project)?;
    let at = finding::now();
    let (commit, dirty) = git_state(project).unwrap_or_else(|e| {
        eprintln!("lab: the run records no commit: {e}");
        (None, None)
    });
    let mut outcomes = Vec::new();
    let mut items = Vec::new();
    let mut measures = Vec::new();
    for spec in specs {
        let (outcome, findings, measured) = run_one(project, spec);
        measures.extend(measured.into_iter().map(|measure| MeasureItem {
            schema: MEASURE_ITEM_SCHEMA.into(),
            check: spec.name.clone(),
            measure,
        }));
        let fps = finding::fingerprints(&findings);
        items.extend(findings.into_iter().zip(fps).map(|(f, fingerprint)| Item {
            schema: ITEM_SCHEMA.into(),
            check: spec.name.clone(),
            fingerprint,
            finding: f,
        }));
        outcomes.push(outcome);
    }
    // Only a clean run can end an interval, so only it keeps the keys,
    // which can be most of a run file.
    if dirty != Some(false) {
        for m in &mut measures {
            m.measure.keys = None;
        }
    }
    let header = RunHeader {
        schema: RUN_SCHEMA.into(),
        at,
        commit,
        dirty,
        checks: outcomes,
    };
    let path = store(project, &header, &items, &measures)?;
    let run = Run {
        path,
        header,
        items,
        measures,
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
    Ok(Report { run, delta })
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
}
