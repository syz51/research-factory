//! `lab retro` and `lab evaluate`: turn recurring findings into lesson
//! candidates, and measure adopted lessons against their metric.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::check::Run;
use crate::finding::{Finding, Severity};
use crate::lessons::{Lesson, Status};

/// How often one `tool:rule` recurred.
#[derive(Debug, Default, Clone)]
pub struct Recurrence {
    pub severity: Option<Severity>,
    /// Check runs in which the rule appeared.
    pub runs: usize,
    /// Distinct subjects (files, sessions, records) it appeared on.
    pub subjects: BTreeSet<String>,
    pub findings: usize,
}

impl Recurrence {
    fn add(&mut self, f: &Finding) {
        self.findings += 1;
        self.subjects.insert(f.subject.clone());
        // Keep the most severe level seen (Error < Warn < Info in the enum order).
        self.severity = Some(self.severity.map_or(f.severity, |s| s.min(f.severity)));
    }

    /// Occurrences that count toward the recurrence threshold: the number of
    /// check runs it appeared in or of distinct subjects, whichever is larger,
    /// so one problem repeated across many files or sessions also recurs.
    pub fn occurrences(&self) -> usize {
        self.runs.max(self.subjects.len())
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
            map.entry(key.clone()).or_default().add(f);
            seen_in_run.insert(key);
        }
        for key in seen_in_run {
            map.entry(key).or_default().runs += 1;
        }
    }
    for f in agent {
        map.entry((f.tool.clone(), f.rule.clone()))
            .or_default()
            .add(f);
    }
    map
}

/// Rules at warn or error severity that recur at least `threshold` times and
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
                && r.occurrences() >= threshold
                && !(key.0 == "lab" && key.1.starts_with("check-"))
        })
        .collect();
    out.sort_by(|a, b| {
        a.1.severity
            .cmp(&b.1.severity)
            .then(b.1.occurrences().cmp(&a.1.occurrences()))
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

/// The outcome of measuring one lesson against its metric.
#[derive(Debug, PartialEq)]
pub struct Evaluation {
    pub before_runs: usize,
    pub after_runs: usize,
    pub before_mean: Option<f64>,
    pub after_mean: Option<f64>,
    pub verdict: &'static str,
}

/// The verdict that allows `lab lesson set <id> verified`.
pub const IMPROVED: &str = "fewer findings since adoption";

/// Minimum check runs on each side of the adoption date for a verdict.
pub const MIN_RUNS: usize = 2;

/// Mean findings per check run for the lesson's metric before and after its
/// adoption date. Only runs in which a check that has ever reported the
/// metric's tool ran successfully count, so a run without that checker is not
/// mistaken for a clean one. Observational: other changes in the same period
/// can explain a difference.
pub fn evaluate(
    lesson: &Lesson,
    runs: &[Run],
    date_of: impl Fn(&str) -> Option<jiff::civil::Date>,
) -> Option<Evaluation> {
    let metric = lesson.meta.metric.as_ref()?;
    let adopted: jiff::civil::Date = lesson.meta.adopted.as_deref()?.parse().ok()?;
    let checks_for_tool: BTreeSet<&str> = runs
        .iter()
        .flat_map(|r| r.items.iter())
        .filter(|i| i.finding.tool == metric.tool)
        .map(|i| i.check.as_str())
        .collect();
    let (mut before, mut after) = (Vec::new(), Vec::new());
    for run in runs {
        let ran = run
            .header
            .checks
            .iter()
            .any(|c| c.ran() && checks_for_tool.contains(c.name.as_str()));
        if !ran {
            continue;
        }
        let count = run
            .items
            .iter()
            .filter(|i| i.finding.tool == metric.tool && i.finding.rule == metric.rule)
            .count() as f64;
        // The adoption date is a calendar date in the project's zone; a run on
        // that day counts as after adoption. A run with an unreadable time is skipped.
        match date_of(&run.header.at) {
            Some(d) if d < adopted => before.push(count),
            Some(_) => after.push(count),
            None => {}
        }
    }
    let mean = |v: &[f64]| (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64);
    let (bm, am) = (mean(&before), mean(&after));
    let verdict = match (bm, am) {
        _ if before.len() < MIN_RUNS || after.len() < MIN_RUNS => "insufficient data",
        (Some(b), Some(a)) if a < b => IMPROVED,
        (Some(b), Some(a)) if a > b => "more findings since adoption",
        _ => "no change",
    };
    Some(Evaluation {
        before_runs: before.len(),
        after_runs: after.len(),
        before_mean: bm,
        after_mean: am,
        verdict,
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
        let items = rules
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let f = finding(r, &format!("doc.md:{i}"), Severity::Error);
                Item {
                    schema: ITEM_SCHEMA.into(),
                    check: "claims".into(),
                    fingerprint: f.fingerprint(),
                    finding: f,
                }
            })
            .collect();
        Run {
            path: "x".into(),
            header: RunHeader {
                schema: RUN_SCHEMA.into(),
                at: at.into(),
                checks: vec![CheckOutcome {
                    name: "claims".into(),
                    command: vec![],
                    exit: Some(if ran { 1 } else { 2 }),
                    status: if ran { "ok" } else { "failed" }.into(),
                    findings: rules.len(),
                    unparsed: 0,
                    duration_ms: 0,
                }],
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
        // quote-not-found is covered by the lesson; big-output is info only.
        assert_eq!(names, ["number-not-in-source", "reread-file"]);
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
        let e = evaluate(&lesson("2026-11-03"), &runs, utc_date).unwrap();
        assert_eq!(
            (e.before_runs, e.after_runs, e.verdict),
            (2, 1, "insufficient data")
        );
        runs.push(run("2026-11-04T00:00:00Z", &[], false)); // checker failed: not a clean run
        runs.push(run("2026-11-05T00:00:00Z", &[], true));
        let e = evaluate(&lesson("2026-11-03"), &runs, utc_date).unwrap();
        assert_eq!((e.before_runs, e.after_runs), (2, 2));
        assert_eq!(e.before_mean, Some(2.0));
        assert_eq!(e.after_mean, Some(0.5));
        assert_eq!(e.verdict, "fewer findings since adoption");
    }
}
