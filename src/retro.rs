//! `lab retro`: turn recurring findings into lesson candidates; and the
//! agent-session records that `lab evaluate` measures lessons against.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use crate::check::Run;
use crate::finding::{self, Finding, Severity};
use crate::lessons::{Lesson, Status};

/// How often one `tool:rule` recurred.
#[derive(Debug, Default, Clone)]
pub struct Recurrence {
    pub severity: Option<Severity>,
    /// Check runs in which the rule appeared.
    pub runs: usize,
    /// Distinct subjects (files, sessions, records) it appeared on.
    pub subjects: BTreeSet<String>,
    /// Distinct findings by fingerprint, so a finding left unfixed across
    /// several runs counts once.
    pub fingerprints: BTreeSet<String>,
}

impl Recurrence {
    fn add(&mut self, f: &Finding, fingerprint: &str) {
        self.fingerprints.insert(fingerprint.to_string());
        self.subjects.insert(f.subject.clone());
        // Keep the most severe level seen (Error < Warn < Info in the enum order).
        self.severity = Some(self.severity.map_or(f.severity, |s| s.min(f.severity)));
    }

    /// Distinct findings of the rule: what counts toward the recurrence threshold.
    pub fn findings(&self) -> usize {
        self.fingerprints.len()
    }
}

pub type Key = (String, String);

/// Groups findings from stored check runs and from an agent-findings file.
pub fn recurrences(runs: &[Run], agent: &[Finding]) -> BTreeMap<Key, Recurrence> {
    let mut map: BTreeMap<Key, Recurrence> = BTreeMap::new();
    for run in runs {
        let mut seen_in_run = BTreeSet::new();
        for item in &run.items {
            let f = &item.finding;
            let key = (f.tool.clone(), f.rule.clone());
            map.entry(key.clone())
                .or_default()
                .add(f, &item.fingerprint);
            seen_in_run.insert(key);
        }
        for key in seen_in_run {
            map.entry(key).or_default().runs += 1;
        }
    }
    for (f, fp) in agent.iter().zip(finding::fingerprints(agent)) {
        map.entry((f.tool.clone(), f.rule.clone()))
            .or_default()
            .add(f, &fp);
    }
    map
}

/// Rules at warn or error severity with at least `threshold` distinct
/// findings that no non-retired lesson measures.
pub fn candidates<'a>(
    rec: &'a BTreeMap<Key, Recurrence>,
    lessons: &[Lesson],
    threshold: usize,
) -> Vec<(&'a Key, &'a Recurrence)> {
    let covered: BTreeSet<Key> = lessons
        .iter()
        .filter(|l| l.meta.status != Status::Retired)
        .filter_map(|l| l.meta.metric.as_ref())
        .map(|m| (m.tool.clone(), m.rule.clone()))
        .collect();
    let mut out: Vec<_> = rec
        .iter()
        .filter(|(key, r)| {
            !covered.contains(*key)
                && r.severity.is_some_and(|s| s <= Severity::Warn)
                && r.findings() >= threshold
                && !(key.0 == "lab" && key.1.starts_with("check-"))
        })
        .collect();
    out.sort_by(|a, b| {
        a.1.severity
            .cmp(&b.1.severity)
            .then(b.1.findings().cmp(&a.1.findings()))
            .then(a.0.cmp(b.0))
    });
    out
}

pub fn read_findings(path: &Path) -> Result<Vec<Finding>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| Finding::parse(l).map_err(|e| format!("{}:{}: {e}", path.display(), i + 1)))
        .collect()
}

/// An `rf.session/1` record: the fields `lab evaluate` stratifies and dates
/// sessions by, and the whole record for `session.` field metrics.
#[derive(Debug, Clone)]
pub struct Session {
    pub source: String,
    pub id: String,
    /// `main` or `subagent`; `None` when the record does not say.
    pub role: Option<String>,
    /// Time of the session's first record; `None` when unknown.
    pub start: Option<String>,
    record: serde_json::Value,
}

#[derive(Deserialize)]
struct SessionHead {
    schema: String,
    source: String,
    id: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    start: Option<String>,
}

impl Session {
    /// Parses one JSON line, rejecting other schemas and other major versions.
    pub fn parse(line: &str) -> Result<Self, String> {
        let record: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let head = SessionHead::deserialize(&record).map_err(|e| e.to_string())?;
        let major = head
            .schema
            .strip_prefix("rf.session/")
            .map(|v| v.split('.').next());
        if major != Some(Some("1")) {
            return Err(format!("unsupported schema `{}`", head.schema));
        }
        Ok(Self {
            source: head.source,
            id: head.id,
            role: head.role,
            start: head.start,
            record,
        })
    }

    /// The `subject` agentprof gives this session's findings.
    pub fn subject(&self) -> String {
        format!("{}:{}", self.source, self.id)
    }

    pub fn role(&self) -> &str {
        self.role.as_deref().unwrap_or("unknown")
    }

    /// The number at a dotted path such as `total.tokens.output`; `None` when
    /// a part is missing, `null` or not a number.
    pub fn field(&self, path: &str) -> Option<f64> {
        path.split('.')
            .try_fold(&self.record, |v, key| v.get(key))?
            .as_f64()
    }
}

pub fn read_sessions(path: &Path) -> Result<Vec<Session>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| Session::parse(l).map_err(|e| format!("{}:{}: {e}", path.display(), i + 1)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{CheckOutcome, ITEM_SCHEMA, Item, RUN_SCHEMA, RunHeader};
    use crate::lessons::{Kind, Meta};

    fn finding(rule: &str, subject: &str, severity: Severity) -> Finding {
        let mut f = Finding::lab(rule, severity, subject, "m".into());
        f.tool = "claimcheck".into();
        f
    }

    fn run(at: &str, rules: &[&str], ran: bool) -> Run {
        run_checks(at, &[("claims", rules, ran)])
    }

    /// A run of several checks, each reporting claimcheck findings on doc.md.
    fn run_checks(at: &str, checks: &[(&str, &[&str], bool)]) -> Run {
        let mut items = Vec::new();
        let mut outcomes = Vec::new();
        for (name, rules, ran) in checks {
            let findings: Vec<Finding> = rules
                .iter()
                .enumerate()
                .map(|(i, r)| finding(r, &format!("{name}/doc.md:{i}"), Severity::Error))
                .collect();
            let fps = finding::fingerprints(&findings);
            items.extend(findings.into_iter().zip(fps).map(|(f, fingerprint)| Item {
                schema: ITEM_SCHEMA.into(),
                check: name.to_string(),
                fingerprint,
                finding: f,
            }));
            outcomes.push(CheckOutcome {
                name: name.to_string(),
                command: vec![],
                exit: Some(if *ran { 1 } else { 2 }),
                status: if *ran { "ok" } else { "failed" }.into(),
                findings: rules.len(),
                unparsed: 0,
                duration_ms: 0,
            });
        }
        Run {
            path: "x".into(),
            header: RunHeader {
                schema: RUN_SCHEMA.into(),
                at: at.into(),
                checks: outcomes,
            },
            items,
        }
    }

    fn lesson(adopted: &str) -> Lesson {
        Lesson {
            path: "quote-first.md".into(),
            meta: Meta {
                id: "quote-first".into(),
                title: "t".into(),
                kind: Kind::Bad,
                status: Status::Adopted,
                scope: "writing".into(),
                tags: vec![],
                rule: "r".into(),
                metric: Some("claimcheck:quote-not-found".parse().unwrap()),
                created: "2026-11-01".into(),
                updated: adopted.into(),
                adopted: Some(adopted.into()),
                evidence: vec![],
            },
            body: String::new(),
        }
    }

    #[test]
    fn recurring_uncovered_rules_become_candidates() {
        let runs = vec![
            run(
                "2026-11-01T00:00:00Z",
                &["quote-not-found", "number-not-in-source"],
                true,
            ),
            run(
                "2026-11-02T00:00:00Z",
                &["quote-not-found", "number-not-in-source"],
                true,
            ),
            run("2026-11-03T00:00:00Z", &["number-not-in-source"], true),
        ];
        let agent = vec![
            finding("reread-file", "s1", Severity::Warn),
            finding("reread-file", "s2", Severity::Warn),
            finding("reread-file", "s3", Severity::Warn),
            finding("big-output", "s1", Severity::Info),
        ];
        let rec = recurrences(&runs, &agent);
        let lessons = vec![lesson("2026-11-02")];
        let names: Vec<&str> = candidates(&rec, &lessons, 3)
            .iter()
            .map(|(k, _)| k.1.as_str())
            .collect();
        // number-not-in-source is one finding (same file and message; only its line
        // moved) seen in 3 runs; quote-not-found is covered by the lesson; big-output
        // is info only.
        assert_eq!(names, ["reread-file"]);
        assert_eq!(
            rec[&("claimcheck".into(), "number-not-in-source".into())].findings(),
            1
        );
    }

    #[test]
    fn an_unfixed_finding_does_not_recur_by_persisting() {
        let runs: Vec<Run> = (1..=5)
            .map(|d| {
                run(
                    &format!("2026-11-0{d}T00:00:00Z"),
                    &["quote-not-found"],
                    true,
                )
            })
            .collect();
        let rec = recurrences(&runs, &[]);
        let r = &rec[&("claimcheck".into(), "quote-not-found".into())];
        assert_eq!((r.findings(), r.runs), (1, 5));
        assert!(candidates(&rec, &[], 3).is_empty());
    }

    #[test]
    fn sessions_expose_strata_and_numeric_fields() {
        let s = Session::parse(
            r#"{"schema":"rf.session/1.2","source":"codex","id":"x","role":"subagent","start":null,"total":{"tokens":{"output":12,"note":"n"}},"compactions":{"count":null}}"#,
        )
        .unwrap();
        assert_eq!((s.subject().as_str(), s.role()), ("codex:x", "subagent"));
        assert_eq!(s.field("total.tokens.output"), Some(12.0));
        assert_eq!(s.field("total.tokens.note"), None);
        assert_eq!(s.field("compactions.count"), None);
        assert_eq!(s.field("missing.path"), None);
        assert!(Session::parse(r#"{"schema":"rf.session/2","source":"a","id":"b"}"#).is_err());
        let bare = Session::parse(r#"{"schema":"rf.session/1","source":"a","id":"b"}"#).unwrap();
        assert_eq!((bare.role(), bare.start.as_deref()), ("unknown", None));
    }
}
