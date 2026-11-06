//! `lab check`: run the configured checkers, store their findings, and compare
//! with the previous run.

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
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    paths.sort();
    paths.iter().map(|p| Run::load(p)).collect()
}

/// Runs one checker and parses its stdout.
fn run_one(project: &Project, spec: &CheckSpec) -> (CheckOutcome, Vec<Finding>) {
    let cwd = spec
        .cwd
        .as_deref()
        .map_or_else(|| project.root.clone(), |c| project.path(c));
    let started = Instant::now();
    let output = Command::new(&spec.run[0])
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
                format!("could not start `{}`: {e}", spec.run[0]),
            );
            return (outcome, vec![f]);
        }
    };
    outcome.exit = output.status.code();
    let mut findings = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match Finding::parse(line) {
            Ok(f) => findings.push(f),
            Err(_) => outcome.unparsed += 1,
        }
    }
    // interfaces.md: 0 = no errors, 1 = errors found, anything else = the check itself failed.
    if matches!(outcome.exit, Some(0 | 1)) {
        outcome.status = "ok".into();
        if outcome.exit == Some(1) && !findings.iter().any(|f| f.severity == Severity::Error) {
            findings.push(Finding::lab(
                "check-exit-mismatch",
                Severity::Warn,
                &spec.name,
                "exited 1 but reported no error finding".into(),
            ));
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
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

/// Fingerprints that appeared or disappeared between two runs of one check.
#[derive(Debug, Clone, PartialEq)]
pub struct Delta {
    pub new: Vec<String>,
    pub resolved: Vec<String>,
}

pub struct Report {
    pub run: Run,
    /// Per check that ran: the change since the previous run in which that
    /// check also ran, or `None` when there is no such run.
    pub delta: BTreeMap<String, Option<Delta>>,
}

pub fn run(project: &Project, only: &[String]) -> Result<Report, String> {
    let specs: Vec<&CheckSpec> = project
        .lab
        .checks
        .iter()
        .filter(|s| only.is_empty() || only.contains(&s.name))
        .collect();
    if specs.is_empty() {
        return Err(if project.lab.checks.is_empty() {
            "no checks configured: add [[lab.checks]] to research.toml".into()
        } else {
            format!("no configured check is named {only:?}")
        });
    }
    let previous = history(project)?;
    let at = finding::now();
    let mut outcomes = Vec::new();
    let mut items = Vec::new();
    for spec in specs {
        let (outcome, findings) = run_one(project, spec);
        items.extend(findings.into_iter().map(|f| Item {
            schema: ITEM_SCHEMA.into(),
            check: spec.name.clone(),
            fingerprint: f.fingerprint(),
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
        let prior = previous.iter().rev().find(|r| {
            r.header
                .checks
                .iter()
                .any(|c| c.name == outcome.name && c.ran())
        });
        let entry = prior.map(|prior| {
            let before = prior.fingerprints(&outcome.name);
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
    for n in 0..100 {
        let path = dir.join(if n == 0 {
            format!("{stamp}.jsonl")
        } else {
            format!("{stamp}-{n}.jsonl")
        });
        let tmp = dir.join(format!(".{stamp}-{n}.tmp"));
        let mut file =
            std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        file.write_all(body.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        drop(file);
        // A hard link fails if the target exists, so concurrent runs never overwrite each other.
        let linked = std::fs::hard_link(&tmp, &path);
        let _ = std::fs::remove_file(&tmp);
        match linked {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    Err(format!(
        "{}: could not allocate a run file name",
        dir.display()
    ))
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

    fn emit_script(dir: &Path, lines: &[&str], exit: i32) -> String {
        let out = dir.join("out.txt");
        std::fs::write(&out, lines.join("\n") + "\nnot json\n").unwrap();
        format!(
            "[[lab.checks]]\nname = \"x\"\nrun = [\"sh\", \"-c\", \"cat '{}'; exit {exit}\"]\n",
            out.display()
        )
    }

    #[test]
    fn stores_runs_and_reports_new_and_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), &emit_script(dir.path(), &[FINDING_A], 1));
        let first = run(&p, &[]).unwrap();
        assert_eq!(first.run.header.checks[0].findings, 1);
        assert_eq!(first.run.header.checks[0].unparsed, 1);
        assert_eq!(first.delta["x"], None);

        let p = project(dir.path(), &emit_script(dir.path(), &[FINDING_B], 0));
        let second = run(&p, &[]).unwrap();
        let delta = second.delta["x"].clone().unwrap();
        assert_eq!(delta.new.len(), 1);
        assert_eq!(delta.resolved.len(), 1);
        assert_eq!(history(&p).unwrap().len(), 2);
    }

    #[test]
    fn a_crashing_or_missing_checker_is_an_error_finding() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(
            dir.path(),
            "[[lab.checks]]\nname = \"boom\"\nrun = [\"sh\", \"-c\", \"echo bad >&2; exit 2\"]\n\
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
    }

    #[test]
    fn exit_one_without_errors_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), &emit_script(dir.path(), &[FINDING_B], 1));
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
