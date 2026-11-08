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
    /// What counts toward the recurrence threshold.
    units: BTreeSet<Unit>,
}

/// One recurrence: a distinct finding, or the rule's agent-session findings
/// in one session group.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Unit {
    Finding(String),
    Group(String),
}

impl Recurrence {
    fn add(&mut self, f: &Finding, fingerprint: &str, unit: Unit) {
        self.fingerprints.insert(fingerprint.to_string());
        self.subjects.insert(f.subject.clone());
        self.units.insert(unit);
        // Keep the most severe level seen (Error < Warn < Info in the enum order).
        self.severity = Some(self.severity.map_or(f.severity, |s| s.min(f.severity)));
    }

    /// Distinct findings of the rule.
    pub fn findings(&self) -> usize {
        self.fingerprints.len()
    }

    /// Session groups the rule's agent-session findings fell in.
    pub fn groups(&self) -> usize {
        self.units
            .iter()
            .filter(|u| matches!(u, Unit::Group(_)))
            .count()
    }

    /// What counts toward the recurrence threshold: distinct findings, with
    /// the agent-session findings in one session group counted once.
    pub fn recurrences(&self) -> usize {
        self.units.len()
    }

    /// The count, such as `2 recurrence(s) from 7 distinct finding(s), 2 of them session group(s)`.
    pub fn describe(&self) -> String {
        match self.groups() {
            0 => format!("{} distinct finding(s)", self.findings()),
            g => format!(
                "{} recurrence(s) from {} distinct finding(s), {g} of them session group(s)",
                self.recurrences(),
                self.findings()
            ),
        }
    }
}

pub type Key = (String, String);

/// Groups findings from stored check runs and from an agent-findings file.
/// With `groups`, agent findings on a known session count once per session
/// group; without it, and for check runs, each distinct finding counts.
pub fn recurrences(
    runs: &[Run],
    agent: &[Finding],
    groups: Option<&SessionGroups>,
) -> BTreeMap<Key, Recurrence> {
    let mut map: BTreeMap<Key, Recurrence> = BTreeMap::new();
    for run in runs {
        let mut seen_in_run = BTreeSet::new();
        for item in &run.items {
            let f = &item.finding;
            let key = (f.tool.clone(), f.rule.clone());
            map.entry(key.clone()).or_default().add(
                f,
                &item.fingerprint,
                Unit::Finding(item.fingerprint.clone()),
            );
            seen_in_run.insert(key);
        }
        for key in seen_in_run {
            map.entry(key).or_default().runs += 1;
        }
    }
    for (f, fp) in agent.iter().zip(finding::fingerprints(agent)) {
        let unit = match groups.and_then(|g| g.of(&f.subject)) {
            Some(group) => Unit::Group(group.to_string()),
            None => Unit::Finding(fp.clone()),
        };
        map.entry((f.tool.clone(), f.rule.clone()))
            .or_default()
            .add(f, &fp, unit);
    }
    map
}

/// Rules at warn or error severity with at least `threshold` recurrences
/// that no non-retired lesson measures.
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
                && r.recurrences() >= threshold
                && !(key.0 == "lab" && key.1.starts_with("check-"))
        })
        .collect();
    out.sort_by(|a, b| {
        a.1.severity
            .cmp(&b.1.severity)
            .then(b.1.recurrences().cmp(&a.1.recurrences()))
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
/// sessions by, and the record's text, parsed again only for `session.`
/// field metrics.
#[derive(Debug, Clone)]
pub struct Session {
    pub source: String,
    pub id: String,
    /// `main` or `subagent`; `None` when the record does not say.
    pub role: Option<String>,
    /// Time of the session's first record; `None` when unknown.
    pub start: Option<String>,
    /// The session that started this one, for a subagent; `None` for a main
    /// session or when the record does not say.
    pub parent_id: Option<String>,
    record: Box<str>,
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
    #[serde(default)]
    parent_id: Option<String>,
}

impl Session {
    /// Parses one JSON line, rejecting other schemas and other major versions.
    pub fn parse(line: &str) -> Result<Self, String> {
        let head: SessionHead = serde_json::from_str(line).map_err(|e| e.to_string())?;
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
            parent_id: head.parent_id,
            record: line.into(),
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
        let record: serde_json::Value = serde_json::from_str(&self.record).ok()?;
        path.split('.')
            .try_fold(&record, |v, key| v.get(key))?
            .as_f64()
    }
}

/// The root of a subagent's tree: its parent's parent and so on, up to a
/// session without a parent or a parent that `by_id` does not have; `None`
/// for a session without a parent. The bound stops a cycle.
pub fn root_of<'a>(by_id: &BTreeMap<(&str, &str), &'a Session>, s: &'a Session) -> Option<&'a str> {
    let mut id = s.parent_id.as_deref()?;
    for _ in 0..by_id.len() {
        match by_id
            .get(&(s.source.as_str(), id))
            .and_then(|p| p.parent_id.as_deref())
        {
            Some(parent) => id = parent,
            None => break,
        }
    }
    Some(id)
}

/// Sessions keyed by `(source, id)`, for [`root_of`].
pub fn by_id(sessions: &[Session]) -> BTreeMap<(&str, &str), &Session> {
    sessions
        .iter()
        .map(|s| ((s.source.as_str(), s.id.as_str()), s))
        .collect()
}

/// The session group of each session's finding subject (`source:id`): the
/// subagents below one root session and the root itself form one group; any
/// other session is a group of its own.
#[derive(Debug, Default)]
pub struct SessionGroups(BTreeMap<String, String>);

impl SessionGroups {
    pub fn new(sessions: &[Session]) -> Self {
        let by_id = by_id(sessions);
        Self(
            sessions
                .iter()
                .map(|s| {
                    let root = root_of(&by_id, s).unwrap_or(&s.id);
                    (s.subject(), format!("{}:{root}", s.source))
                })
                .collect(),
        )
    }

    /// The group of a finding subject; `None` when it is not a known session.
    pub fn of(&self, subject: &str) -> Option<&str> {
        self.0.get(subject).map(String::as_str)
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
                reviewed: None,
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
        let rec = recurrences(&runs, &agent, None);
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
        let rec = recurrences(&runs, &[], None);
        let r = &rec[&("claimcheck".into(), "quote-not-found".into())];
        assert_eq!((r.findings(), r.runs), (1, 5));
        assert!(candidates(&rec, &[], 3).is_empty());
    }

    fn session(id: &str, parent: Option<&str>) -> Session {
        let parent = parent.map_or("null".into(), |p| format!("\"{p}\""));
        Session::parse(&format!(
            r#"{{"schema":"rf.session/1","source":"claude","id":"{id}","parent_id":{parent}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn agent_findings_recur_once_per_session_group() {
        // root → a → a1, and root → b: one tree; s1, s2, s3 are unrelated.
        let sessions = vec![
            session("root", None),
            session("a", Some("root")),
            session("a1", Some("a")),
            session("b", Some("root")),
            session("s1", None),
            session("s2", None),
            session("s3", None),
        ];
        let groups = SessionGroups::new(&sessions);
        let agent = |rule: &str, ids: &[&str]| -> Vec<Finding> {
            ids.iter()
                .map(|id| finding(rule, &format!("claude:{id}"), Severity::Warn))
                .collect()
        };
        let mut findings = agent("clustered", &["a", "a1", "b"]);
        findings.extend(agent("spread", &["s1", "s2", "s3"]));
        // Subjects that are not sessions in the file count per finding.
        findings.extend(agent("unknown", &["x1", "x2", "x3"]));
        let runs = vec![run(
            "2026-11-01T00:00:00Z",
            &["quote-not-found", "quote-not-found", "quote-not-found"],
            true,
        )];
        let rec = recurrences(&runs, &findings, Some(&groups));
        let count = |rule: &str| {
            let r = &rec[&("claimcheck".into(), rule.into())];
            (r.recurrences(), r.findings(), r.groups())
        };
        assert_eq!(count("clustered"), (1, 3, 1));
        assert_eq!(count("spread"), (3, 3, 3));
        assert_eq!(count("unknown"), (3, 3, 0));
        assert_eq!(count("quote-not-found"), (3, 3, 0), "check runs as before");
        let names: Vec<&str> = candidates(&rec, &[], 3)
            .iter()
            .map(|(k, _)| k.1.as_str())
            .collect();
        assert_eq!(names, ["quote-not-found", "spread", "unknown"]);
        assert_eq!(
            rec[&("claimcheck".into(), "clustered".into())].describe(),
            "1 recurrence(s) from 3 distinct finding(s), 1 of them session group(s)"
        );

        // Without the sessions file every distinct finding counts.
        let rec = recurrences(&runs, &findings, None);
        assert_eq!(
            rec[&("claimcheck".into(), "clustered".into())].recurrences(),
            3
        );
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
