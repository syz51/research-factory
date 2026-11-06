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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckOutcome {
    pub name: String,
    pub command: Vec<String>,
    /// Exit code, or `None` when the program could not start or was killed.
    pub exit: Option<i32>,
    /// `ok` when the checker ran to completion (exit 0 or 1), else `failed`.
    pub status: String,
    pub findings: usize,
    /// Non-empty stdout lines that were not `rf.finding/1` records.
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
}

impl Run {
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
        let items = lines
            .map(|(i, line)| {
                serde_json::from_str::<Item>(line)
                    .map_err(|e| format!("{}:{}: {e}", path.display(), i + 1))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            path: path.to_path_buf(),
            header,
            items,
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

/// Runs one checker and parses its stdout.
fn run_one(project: &Project, spec: &CheckSpec) -> (CheckOutcome, Vec<Finding>) {
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
            return (outcome, vec![f]);
        }
    };
    outcome.exit = output.status.code();
    let mut findings = Vec::new();
    let mut first_unparsed = None;
    for (i, line) in String::from_utf8_lossy(&output.stdout).lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match Finding::parse(line) {
            Ok(f) => findings.push(f),
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
                "{} stdout line(s) are not rf.finding/1 records (first: line {line}: {e}); \
                 is the checker run with `--format jsonl`?",
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
    (outcome, findings)
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
    let mut outcomes = Vec::new();
    let mut items = Vec::new();
    for spec in specs {
        let (outcome, findings) = run_one(project, spec);
        let fps = finding::fingerprints(&findings);
        items.extend(findings.into_iter().zip(fps).map(|(f, fingerprint)| Item {
            schema: ITEM_SCHEMA.into(),
            check: spec.name.clone(),
            fingerprint,
            finding: f,
        }));
        outcomes.push(outcome);
    }
    let header = RunHeader {
        schema: RUN_SCHEMA.into(),
        at,
        checks: outcomes,
    };
    let path = store(project, &header, &items)?;
    let run = Run {
        path,
        header,
        items,
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
                    run: run
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
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
fn store(project: &Project, header: &RunHeader, items: &[Item]) -> Result<PathBuf, String> {
    let dir = project.checks_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stamp = jiff::Timestamp::now()
        .strftime("%Y%m%dT%H%M%S%.3fZ")
        .to_string();
    let mut body = serde_json::to_string(header).map_err(|e| e.to_string())? + "\n";
    for item in items {
        body += &(serde_json::to_string(item).map_err(|e| e.to_string())? + "\n");
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
