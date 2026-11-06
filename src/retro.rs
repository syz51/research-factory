//! `lab retro` and `lab evaluate`: turn recurring findings into lesson
//! candidates, and measure adopted lessons against their metric.

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

/// The fields of an `rf.session/1` record that `lab evaluate` needs.
#[derive(Debug, Clone, Deserialize)]
pub struct Session {
    schema: String,
    pub source: String,
    pub id: String,
    /// Time of the session's first record; `null` when unknown.
    #[serde(default)]
    pub start: Option<String>,
}

impl Session {
    /// The `subject` agentprof gives this session's findings.
    pub fn subject(&self) -> String {
        format!("{}:{}", self.source, self.id)
    }
}

pub fn read_sessions(path: &Path) -> Result<Vec<Session>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            let at = || format!("{}:{}", path.display(), i + 1);
            let s: Session = serde_json::from_str(l).map_err(|e| format!("{}: {e}", at()))?;
            let major = s
                .schema
                .strip_prefix("rf.session/")
                .map(|v| v.split('.').next());
            if major != Some(Some("1")) {
                return Err(format!("{}: unsupported schema `{}`", at(), s.schema));
            }
            Ok(s)
        })
        .collect()
}

/// Agent-session data for measuring lessons whose metric `agentprof` reports.
pub struct Agent<'a> {
    pub findings: &'a [Finding],
    /// `Err` holds why the sessions file could not be read.
    pub sessions: Result<&'a [Session], &'a str>,
}

/// The outcome of measuring one lesson against its metric.
#[derive(Debug, PartialEq)]
pub struct Evaluation {
    /// What one observation is: a check `run` or an agent `session`.
    pub unit: &'static str,
    pub before_runs: usize,
    pub after_runs: usize,
    pub before_mean: Option<f64>,
    pub after_mean: Option<f64>,
    pub verdict: String,
}

/// The verdict that allows `lab lesson set <id> verified`.
pub const IMPROVED: &str = "fewer findings since adoption";

/// Minimum observations on each side of the adoption date for a verdict.
pub const MIN_RUNS: usize = 2;

/// Mean findings of the lesson's metric per observation before and after its
/// adoption date. Observational: other changes in the same period can explain
/// a difference.
///
/// When a configured check has reported the metric's tool, an observation is
/// a stored check run in which every such configured check ran to completion,
/// so a failed or partial (`--only`) run is not mistaken for a clean one.
/// Otherwise, when the agent findings contain the tool, an observation is an
/// agent session, dated by its start; sessions without findings count as zero.
pub fn evaluate(
    lesson: &Lesson,
    runs: &[Run],
    configured: &[&str],
    agent: &Agent,
    date_of: impl Fn(&str) -> Option<jiff::civil::Date>,
) -> Option<Evaluation> {
    let metric = lesson.meta.metric.as_ref()?;
    let adopted: jiff::civil::Date = lesson.meta.adopted.as_deref()?.parse().ok()?;
    let matches = |f: &Finding| f.tool == metric.tool && f.rule == metric.rule;
    let required: BTreeSet<&str> = runs
        .iter()
        .flat_map(|r| r.items.iter())
        .filter(|i| i.finding.tool == metric.tool && configured.contains(&i.check.as_str()))
        .map(|i| i.check.as_str())
        .collect();
    let (mut before, mut after) = (Vec::new(), Vec::new());
    // The adoption date is a calendar date in the project's zone; an
    // observation on that day counts as after adoption. One without a readable
    // time is skipped.
    let mut place = |at: &str, count: usize| match date_of(at) {
        Some(d) if d < adopted => before.push(count as f64),
        Some(_) => after.push(count as f64),
        None => {}
    };
    let unit = if !required.is_empty() {
        for run in runs {
            let complete = required
                .iter()
                .all(|name| run.header.checks.iter().any(|c| c.name == *name && c.ran()));
            if complete {
                let count = run
                    .items
                    .iter()
                    .filter(|i| required.contains(i.check.as_str()) && matches(&i.finding))
                    .count();
                place(&run.header.at, count);
            }
        }
        "run"
    } else if agent.findings.iter().any(|f| f.tool == metric.tool) {
        let sessions = match agent.sessions {
            Ok(s) => s,
            Err(why) => {
                return Some(Evaluation {
                    unit: "session",
                    before_runs: 0,
                    after_runs: 0,
                    before_mean: None,
                    after_mean: None,
                    verdict: format!("not measurable: {why}"),
                });
            }
        };
        let mut per_session: BTreeMap<&str, usize> = BTreeMap::new();
        for f in agent.findings.iter().filter(|f| matches(f)) {
            *per_session.entry(f.subject.as_str()).or_default() += 1;
        }
        for s in sessions {
            if let Some(start) = &s.start {
                place(
                    start,
                    per_session.get(s.subject().as_str()).copied().unwrap_or(0),
                );
            }
        }
        "session"
    } else {
        "run"
    };
    let mean = |v: &[f64]| (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64);
    let (bm, am) = (mean(&before), mean(&after));
    let verdict = match (bm, am) {
        _ if before.len() < MIN_RUNS || after.len() < MIN_RUNS => "insufficient data",
        (Some(b), Some(a)) if a < b => IMPROVED,
        (Some(b), Some(a)) if a > b => "more findings since adoption",
        _ => "no change",
    };
    Some(Evaluation {
        unit,
        before_runs: before.len(),
        after_runs: after.len(),
        before_mean: bm,
        after_mean: am,
        verdict: verdict.into(),
    })
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

    const NO_AGENT: Agent = Agent {
        findings: &[],
        sessions: Err("none"),
    };

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

    fn utc_date(s: &str) -> Option<jiff::civil::Date> {
        s.parse::<jiff::Timestamp>()
            .ok()
            .map(|t| t.to_zoned(jiff::tz::TimeZone::UTC).date())
    }

    #[test]
    fn evaluation_needs_runs_on_both_sides_and_skips_failed_runs() {
        let mut runs = vec![
            run(
                "2026-11-01T00:00:00Z",
                &["quote-not-found", "quote-not-found"],
                true,
            ),
            run(
                "2026-11-02T00:00:00Z",
                &["quote-not-found", "quote-not-found"],
                true,
            ),
            run("2026-11-03T00:00:00Z", &["quote-not-found"], true),
        ];
        let e = evaluate(
            &lesson("2026-11-03"),
            &runs,
            &["claims"],
            &NO_AGENT,
            utc_date,
        )
        .unwrap();
        assert_eq!(
            (e.before_runs, e.after_runs, e.verdict.as_str()),
            (2, 1, "insufficient data")
        );
        runs.push(run("2026-11-04T00:00:00Z", &[], false)); // checker failed: not a clean run
        runs.push(run("2026-11-05T00:00:00Z", &[], true));
        let e = evaluate(
            &lesson("2026-11-03"),
            &runs,
            &["claims"],
            &NO_AGENT,
            utc_date,
        )
        .unwrap();
        assert_eq!((e.before_runs, e.after_runs), (2, 2));
        assert_eq!(e.before_mean, Some(2.0));
        assert_eq!(e.after_mean, Some(0.5));
        assert_eq!(e.verdict, IMPROVED);
    }

    #[test]
    fn partial_runs_are_not_counted() {
        let both = |at: &str| {
            run_checks(
                at,
                &[
                    ("docs", &["quote-not-found", "quote-not-found"], true),
                    ("thesis", &["quote-not-found", "quote-not-found"], true),
                ],
            )
        };
        let docs_only = |at: &str| {
            run_checks(
                at,
                &[("docs", &["quote-not-found", "quote-not-found"], true)],
            )
        };
        let runs = vec![
            both("2026-11-01T00:00:00Z"),
            both("2026-11-02T00:00:00Z"),
            docs_only("2026-11-04T00:00:00Z"),
            docs_only("2026-11-05T00:00:00Z"),
        ];
        let e = evaluate(
            &lesson("2026-11-03"),
            &runs,
            &["docs", "thesis"],
            &NO_AGENT,
            utc_date,
        )
        .unwrap();
        assert_eq!((e.before_runs, e.after_runs), (2, 0));
        assert_eq!(e.verdict, "insufficient data");
        // A check that is no longer configured is not required.
        let e = evaluate(&lesson("2026-11-03"), &runs, &["docs"], &NO_AGENT, utc_date).unwrap();
        assert_eq!((e.before_runs, e.after_runs), (2, 2));
        assert_eq!(e.verdict, "no change");
    }

    #[test]
    fn agent_rules_are_measured_per_session() {
        let mut l = lesson("2026-11-03");
        l.meta.metric = Some("agentprof:reread-file".parse().unwrap());
        let f = |subject: &str| {
            let mut f = Finding::lab("reread-file", Severity::Warn, subject, "m".into());
            f.tool = "agentprof".into();
            f
        };
        let findings = vec![f("claude:a"), f("claude:b"), f("codex:c")];
        let session = |source: &str, id: &str, start: &str| Session {
            schema: "rf.session/1".into(),
            source: source.into(),
            id: id.into(),
            start: Some(start.into()),
        };
        let sessions = vec![
            session("claude", "a", "2026-11-01T00:00:00Z"),
            session("claude", "b", "2026-11-02T00:00:00Z"),
            session("codex", "c", "2026-11-04T00:00:00Z"),
            session("codex", "d", "2026-11-05T00:00:00Z"),
            session("codex", "e", "2026-11-06T00:00:00Z"),
        ];
        let agent = Agent {
            findings: &findings,
            sessions: Ok(&sessions),
        };
        let e = evaluate(&l, &[], &[], &agent, utc_date).unwrap();
        assert_eq!(e.unit, "session");
        assert_eq!((e.before_runs, e.after_runs), (2, 3));
        assert_eq!((e.before_mean, e.after_mean), (Some(1.0), Some(1.0 / 3.0)));
        assert_eq!(e.verdict, IMPROVED);
        let missing = Agent {
            findings: &findings,
            sessions: Err("sessions.jsonl: not found"),
        };
        let e = evaluate(&l, &[], &[], &missing, utc_date).unwrap();
        assert_eq!(e.verdict, "not measurable: sessions.jsonl: not found");
    }
}
