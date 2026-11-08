//! `lab brief`: the short text an agent reads at the start of a session.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::check::{CheckOutcome, Run};
use crate::evaluate::{Evaluation, Method, Verdict, p_label};
use crate::finding::Severity;
use crate::lesson_loop::{Record, SendStatus};
use crate::lessons::{Kind, Lesson, Status};

/// Adopted lessons older than this without a `verified` decision are listed
/// for evaluation.
pub const EVALUATE_AFTER_DAYS: i32 = 14;

/// `runs` are the stored check runs, oldest first, and `checks` the configured
/// check names; `candidates` counts recurring rules without a lesson across
/// the whole project. `measurements` is the lesson-measurement section of a
/// project with `[lab.loop]`, which replaces the reminder to evaluate.
#[allow(clippy::too_many_arguments)]
pub fn render(
    project: &str,
    today: &str,
    lessons: &[Lesson],
    runs: &[Run],
    checks: &[String],
    tags: &[String],
    candidates: usize,
    measurements: Option<&str>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {project}: research brief ({today})\n");

    let active: Vec<&Lesson> = lessons
        .iter()
        .filter(|l| l.meta.status.active())
        .filter(|l| tags.is_empty() || l.meta.tags.iter().any(|t| tags.contains(t)))
        .collect();
    let _ = writeln!(out, "## Rules from lessons ({})\n", active.len());
    if active.is_empty() {
        if !tags.is_empty() && lessons.iter().any(|l| l.meta.status.active()) {
            let _ = writeln!(out, "None adopted with tag {}.", tags.join(", "));
        } else {
            out.push_str("None adopted yet.\n");
        }
    } else {
        out.push_str("Follow every rule below; its group says only where it came from.\n");
    }
    let (mut failures, mut worked) = (Vec::new(), Vec::new());
    for l in &active {
        match l.meta.kind {
            Kind::Bad => failures.push(*l),
            Kind::Good => worked.push(*l),
        }
    }
    for (heading, rules) in [
        ("Learned from failures", failures),
        ("Practices that worked", worked),
    ] {
        if rules.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n{heading}:\n");
        for l in rules {
            let _ = writeln!(out, "- {} (`{}`)", l.meta.rule.trim(), l.meta.id);
        }
    }
    out.push_str("\n## Last check\n\n");
    // Each check's latest run: `lab check --only` stores only the checks it ran.
    let latest: Vec<(&Run, &CheckOutcome)> = checks
        .iter()
        .filter_map(|name| {
            runs.iter().rev().find_map(|r| {
                r.header
                    .checks
                    .iter()
                    .find(|c| &c.name == name)
                    .map(|c| (r, c))
            })
        })
        .collect();
    if latest.is_empty() {
        out.push_str("No `lab check` run recorded.\n");
    } else {
        let mut by_sev: BTreeMap<Severity, usize> = BTreeMap::new();
        let mut by_rule: BTreeMap<(String, String), usize> = BTreeMap::new();
        for (run, check) in &latest {
            for item in run.items.iter().filter(|i| i.check == check.name) {
                *by_sev.entry(item.finding.severity).or_default() += 1;
                if item.finding.severity == Severity::Error {
                    *by_rule
                        .entry((item.finding.tool.clone(), item.finding.rule.clone()))
                        .or_default() += 1;
                }
            }
        }
        let at: BTreeSet<&str> = latest.iter().map(|(r, _)| r.header.at.as_str()).collect();
        let when = match (at.first(), at.last()) {
            (Some(a), Some(b)) if a != b => format!("{a} to {b} (each check's latest run)"),
            (Some(a), _) => (*a).to_string(),
            _ => unreachable!("latest is not empty"),
        };
        let _ = writeln!(
            out,
            "{when}: {} errors, {} warnings, {} info across {} checks.",
            by_sev.get(&Severity::Error).unwrap_or(&0),
            by_sev.get(&Severity::Warn).unwrap_or(&0),
            by_sev.get(&Severity::Info).unwrap_or(&0),
            latest.len()
        );
        let failed: Vec<&str> = latest
            .iter()
            .filter(|(_, c)| !c.ran())
            .map(|(_, c)| c.name.as_str())
            .collect();
        if !failed.is_empty() {
            let _ = writeln!(out, "Checks that failed to run: {}.", failed.join(", "));
        }
        let never: Vec<&str> = checks
            .iter()
            .filter(|n| !latest.iter().any(|(_, c)| &c.name == *n))
            .map(String::as_str)
            .collect();
        if !never.is_empty() {
            let _ = writeln!(out, "Never run: {}.", never.join(", "));
        }
        let mut top: Vec<_> = by_rule.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for ((tool, rule), n) in top.into_iter().take(5) {
            let _ = writeln!(out, "- {n} × {tool}:{rule}");
        }
    }

    if let Some(m) = measurements {
        out.push_str(m);
    }
    let due: Vec<&Lesson> = lessons
        .iter()
        .filter(|_| measurements.is_none())
        .filter(|l| l.meta.status == Status::Adopted)
        .filter(|l| tags.is_empty() || l.meta.tags.iter().any(|t| tags.contains(t)))
        .filter(|l| {
            l.meta
                .adopted
                .as_deref()
                .and_then(|a| a.parse::<jiff::civil::Date>().ok())
                .zip(today.parse::<jiff::civil::Date>().ok())
                .is_some_and(|(a, t)| (t - a).get_days() >= EVALUATE_AFTER_DAYS)
        })
        .collect();
    if !due.is_empty() || candidates > 0 {
        out.push_str("\n## Maintenance\n\n");
        if !due.is_empty() {
            let ids: Vec<&str> = due.iter().map(|l| l.meta.id.as_str()).collect();
            let _ = writeln!(
                out,
                "- Adopted over {EVALUATE_AFTER_DAYS} days, not yet verified: {} (run `lab evaluate`).",
                ids.join(", ")
            );
        }
        if candidates > 0 {
            let _ = writeln!(
                out,
                "- {candidates} recurring finding rule(s) have no lesson{} (run `lab retro`).",
                if tags.is_empty() {
                    ""
                } else {
                    " across the project, whatever their tags"
                }
            );
        }
    }
    out
}

/// Hours after which the brief says the loop may not be running.
const LOOP_STALE_HOURS: i64 = 24;

fn age(d: jiff::SignedDuration) -> String {
    let mins = d.as_mins().max(0);
    if mins < 60 {
        format!("{mins} min ago")
    } else if mins < 48 * 60 {
        format!("{} h ago", mins / 60)
    } else {
        format!("{} days ago", mins / (24 * 60))
    }
}

/// One lesson's verdict, as short as the brief needs.
/// `status` and `reviewed` are the lesson's current ones, which can be newer
/// than the loop's record.
fn verdict_line(e: &Evaluation, status: Status, reviewed: Option<&str>) -> String {
    let unit = e.unit.map_or("session", |u| u.as_str());
    // A check-run lesson: its verdict, if any, and its introduced findings,
    // which are the news while there is no verdict.
    if e.is_check_run() && e.verdict != Verdict::Unmeasurable {
        let min = e.min_sessions;
        let verdict = match e.verdict {
            Verdict::Collecting if e.method == Some(Method::ConditionalBinomial) => {
                let mut s = format!("collecting {}/{min} {unit}s after", e.after.n);
                if e.before.n < min {
                    let _ = write!(s, ", {}/{min} before", e.before.n);
                }
                s
            }
            Verdict::Collecting => String::new(),
            _ => verdict_text(e, status),
        };
        let introduced = match &e.introduced {
            Some(i) if reviewed.is_some() && i.reviewed.as_deref() != reviewed => {
                "reviewed after the last `lab loop`; `lab evaluate` gives the current count".into()
            }
            Some(i) if i.count > 0 => format!(
                "{} introduced since {} → review (`lab evaluate`)",
                i.count,
                i.since_text(e.adopted.as_deref())
            ),
            Some(i) => format!(
                "none introduced since {}",
                i.since_text(e.adopted.as_deref())
            ),
            None => "no complete check run since adoption".into(),
        };
        return if verdict.is_empty() {
            introduced
        } else {
            format!("{verdict}; {introduced}")
        };
    }
    verdict_text(e, status)
}

/// A verdict as the brief shows it for agent sessions, and for a check-run
/// lesson with a verdict.
fn verdict_text(e: &Evaluation, status: Status) -> String {
    let p = |p: Option<f64>| p.map(|p| format!(" {}", p_label(p))).unwrap_or_default();
    let unit = e.unit.map_or("session", |u| u.as_str());
    match e.verdict {
        Verdict::Collecting => {
            let mut s = format!("collecting {}/{}", e.after.n, e.min_sessions);
            if e.before.n < e.min_sessions {
                let _ = write!(s, " after, {}/{} before", e.before.n, e.min_sessions);
            }
            if let Some(m) = e.sessions_needed {
                let _ = write!(s, ", about {m} more {unit}s");
            }
            s
        }
        Verdict::Inconclusive => match e.sessions_needed {
            Some(m) => format!("inconclusive, about {m} more {unit}s"),
            None => format!("inconclusive{}", p(e.p_lower)),
        },
        Verdict::Improved if status == Status::Adopted => {
            format!("improved{} → verify", p(e.p_lower))
        }
        Verdict::Improved => format!("improved{}", p(e.p_lower)),
        Verdict::Worse => format!("worse{} → revise or retire", p(e.p_upper)),
        Verdict::Unmeasurable => format!(
            "unmeasurable: {}",
            e.reason.as_deref().unwrap_or("no reason recorded")
        ),
    }
}

/// The `## Lesson measurements` section of a project with `[lab.loop]`:
/// when the loop last ran, each active lesson's verdict from that run, and
/// what failed or waits to be sent.
pub fn measurements(
    record: Result<Option<Record>, String>,
    now: jiff::Timestamp,
    tz: &jiff::tz::TimeZone,
    lessons: &[Lesson],
    tags: &[String],
) -> String {
    let mut out = String::from("\n## Lesson measurements\n\n");
    let r = match record {
        Err(e) => {
            let _ = writeln!(
                out,
                "The loop's last record is unreadable: {e}. Run `lab loop`."
            );
            return out;
        }
        Ok(None) => {
            out.push_str("The loop has never run: run `lab loop`.\n");
            return out;
        }
        Ok(Some(r)) => r,
    };
    match r.finished.parse::<jiff::Timestamp>() {
        Ok(t) => {
            let since = now.duration_since(t);
            let _ = write!(
                out,
                "Last measured {} ({}).",
                t.to_zoned(tz.clone()).strftime("%Y-%m-%d %H:%M"),
                age(since)
            );
            if since.as_hours() >= LOOP_STALE_HOURS {
                out.push_str(
                    " That is over a day: the hooks that run `lab loop --background` may not be running.",
                );
            }
            out.push('\n');
        }
        Err(_) => {
            let _ = writeln!(out, "Last measured at an unreadable time `{}`.", r.finished);
        }
    }
    let shown: Vec<&Lesson> = lessons
        .iter()
        .filter(|l| l.meta.status.active())
        .filter(|l| tags.is_empty() || l.meta.tags.iter().any(|t| tags.contains(t)))
        .collect();
    let mut lines = Vec::new();
    for l in &shown {
        match r.evaluations.iter().find(|e| e.lesson == l.meta.id) {
            Some(e) => lines.push(format!(
                "- `{}`: {}",
                l.meta.id,
                verdict_line(e, l.meta.status, l.meta.reviewed.as_deref())
            )),
            None => lines.push(format!("- `{}`: not measured yet", l.meta.id)),
        }
    }
    if !lines.is_empty() {
        out.push('\n');
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
    }
    let failed: Vec<String> = r
        .measure
        .iter()
        .filter(|m| !m.ok)
        .map(|m| format!("`{}` ({})", m.argv.join(" "), m.status))
        .collect();
    let count = |s: SendStatus| r.signals.iter().filter(|x| x.status == s).count();
    let mut notes = Vec::new();
    if !failed.is_empty() {
        notes.push(format!("Measure commands failed: {}.", failed.join(", ")));
    }
    if count(SendStatus::Failed) > 0 {
        notes.push(format!(
            "{} signal(s) could not be sent and will be retried; the notify errors are in <state_dir>/lab/loop/loop.log.",
            count(SendStatus::Failed)
        ));
    }
    if count(SendStatus::Pending) > 0 {
        notes.push(format!(
            "{} signal(s) pending: no `[lab.loop] notify` command is set (`lab loop --format jsonl` prints them).",
            count(SendStatus::Pending)
        ));
    }
    if r.errors.iter().any(|e| !e.starts_with("notify ")) {
        notes.push(format!(
            "The loop failed: {}.",
            r.errors
                .iter()
                .filter(|e| !e.starts_with("notify "))
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !notes.is_empty() {
        out.push('\n');
        for n in notes {
            let _ = writeln!(out, "{n}");
        }
    }
    out
}

/// What the brief says about a `[lab.archive]`.
pub struct Archive {
    /// The start page and its size, `None` when it does not exist.
    pub start: Option<(String, Option<u64>)>,
    /// The generated index files.
    pub indexes: Vec<String>,
}

/// The brief's closing section for a project with an archive.
pub fn archive(a: &Archive) -> String {
    let mut out = String::from("\n## Archive\n\n");
    match &a.start {
        Some((path, Some(bytes))) => {
            let _ = writeln!(
                out,
                "- Read {path} first ({:.1} KB).",
                *bytes as f64 / 1024.0
            );
        }
        Some((path, None)) => {
            let _ = writeln!(
                out,
                "- The start page {path} does not exist yet; create it."
            );
        }
        None => {}
    }
    let _ = write!(out, "- Look things up: `lab archive find <words>`");
    if a.indexes.is_empty() {
        out.push_str(".\n");
    } else {
        let _ = writeln!(out, ", or the indexes {}.", a.indexes.join(", "));
    }
    out.push_str("- Before finishing: `lab archive update`, then `lab check --new-only`.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lessons::Meta;

    fn lesson(
        id: &str,
        kind: Kind,
        status: Status,
        adopted: Option<&str>,
        tags: &[&str],
    ) -> Lesson {
        Lesson {
            path: format!("{id}.md").into(),
            meta: Meta {
                id: id.into(),
                title: "t".into(),
                kind,
                status,
                scope: "workflow".into(),
                tags: tags.iter().map(|t| t.to_string()).collect(),
                rule: format!("rule {id}"),
                metric: None,
                created: "2026-10-01".into(),
                updated: "2026-10-01".into(),
                adopted: adopted.map(Into::into),
                reviewed: None,
                evidence: vec![],
            },
            body: String::new(),
        }
    }

    #[test]
    fn lists_active_rules_and_due_evaluations() {
        let lessons = vec![
            lesson(
                "a",
                Kind::Good,
                Status::Adopted,
                Some("2026-11-06"),
                &["lit"],
            ),
            lesson(
                "b",
                Kind::Bad,
                Status::Adopted,
                Some("2026-10-01"),
                &["exp"],
            ),
            lesson("c", Kind::Bad, Status::Observed, None, &["lit"]),
        ];
        let text = render("demo", "2026-11-07", &lessons, &[], &[], &[], 2, None);
        let failures = text
            .find("Learned from failures:\n\n- rule b (`b`)")
            .unwrap();
        let worked = text
            .find("Practices that worked:\n\n- rule a (`a`)")
            .unwrap();
        assert!(
            failures < worked,
            "lessons from failures come first:\n{text}"
        );
        assert!(text.contains("Follow every rule below"));
        assert!(
            !text.contains("[avoid]") && !text.contains("[keep]"),
            "a rule is never shown inverted:\n{text}"
        );
        assert!(!text.contains("rule c"));
        assert!(text.contains("not yet verified: b"));
        assert!(text.contains("2 recurring finding rule(s)"));
        let lit = render(
            "demo",
            "2026-11-07",
            &lessons,
            &[],
            &[],
            &["lit".into()],
            0,
            None,
        );
        assert!(lit.contains("rule a") && !lit.contains("(`b`)") && !lit.contains("Maintenance"));
        assert!(
            !lit.contains("Learned from failures"),
            "an empty group has no heading:\n{lit}"
        );
    }

    fn run(at: &str, checks: &[(&str, &str)], errors: &[(&str, &str)]) -> Run {
        use crate::check::{Item, RunHeader};
        use crate::finding::Finding;
        Run {
            path: format!("{at}.jsonl").into(),
            header: RunHeader {
                schema: "rf.labcheck/1".into(),
                at: at.into(),
                commit: None,
                dirty: None,
                replayed: false,
                checks: checks
                    .iter()
                    .map(|(name, status)| CheckOutcome {
                        name: (*name).into(),
                        command: vec![],
                        exit: Some(0),
                        status: (*status).into(),
                        findings: 0,
                        unparsed: 0,
                        duration_ms: 0,
                        version: None,
                        uncommitted_inputs: Vec::new(),
                    })
                    .collect(),
            },
            items: errors
                .iter()
                .enumerate()
                .map(|(i, (check, rule))| Item {
                    schema: "rf.labcheck.item/1".into(),
                    check: (*check).into(),
                    fingerprint: format!("{i}"),
                    finding: Finding::lab(rule, Severity::Error, "s", "m".into()),
                })
                .collect(),
            measures: Vec::new(),
        }
    }

    #[test]
    fn last_check_uses_each_checks_latest_run() {
        let checks: Vec<String> = ["references", "claims", "sources"].map(Into::into).to_vec();
        let runs = [
            run(
                "2026-11-06T00:00:00Z",
                &[("references", "ok"), ("claims", "ok")],
                &[("references", "r1"), ("claims", "c1"), ("claims", "c1")],
            ),
            // `lab check --only claims` found nothing.
            run("2026-11-07T00:00:00Z", &[("claims", "ok")], &[]),
        ];
        let text = render("demo", "2026-11-07", &[], &runs, &checks, &[], 0, None);
        assert!(
            text.contains("2026-11-06T00:00:00Z to 2026-11-07T00:00:00Z (each check's latest run): 1 errors, 0 warnings, 0 info across 2 checks."),
            "{text}"
        );
        assert!(text.contains("- 1 × lab:r1"), "{text}");
        assert!(!text.contains("lab:c1"), "{text}");
        assert!(text.contains("Never run: sources."), "{text}");
    }

    #[test]
    fn tag_scoped_brief_says_what_the_tags_left_out() {
        let lessons = vec![lesson(
            "a",
            Kind::Good,
            Status::Adopted,
            Some("2026-11-06"),
            &["lit"],
        )];
        let exp = render(
            "demo",
            "2026-11-07",
            &lessons,
            &[],
            &[],
            &["exp".into()],
            3,
            None,
        );
        assert!(exp.contains("None adopted with tag exp."), "{exp}");
        assert!(!exp.contains("None adopted yet."), "{exp}");
        assert!(
            exp.contains("have no lesson across the project, whatever their tags"),
            "{exp}"
        );
        let all = render("demo", "2026-11-07", &lessons, &[], &[], &[], 3, None);
        assert!(all.contains("- 3 recurring finding rule(s) have no lesson (run `lab retro`)."));
    }

    fn evaluation(id: &str, verdict: Verdict) -> Evaluation {
        let mut l = lesson(id, Kind::Bad, Status::Adopted, Some("2026-11-01"), &[]);
        l.meta.metric = Some("agentprof:file-reread".parse().unwrap());
        Evaluation {
            schema: crate::evaluate::SCHEMA.into(),
            lesson: id.into(),
            lesson_status: Status::Adopted,
            metric: Some("agentprof:file-reread".into()),
            unit: None,
            method: None,
            verdict,
            reason: None,
            adopted: l.meta.adopted.clone(),
            before: crate::evaluate::Side {
                n: 1022,
                ..Default::default()
            },
            after: crate::evaluate::Side {
                n: 3,
                ..Default::default()
            },
            p_lower: Some(0.004),
            p_upper: Some(0.9),
            strata: vec![],
            excluded: 0,
            sessions_needed: None,
            min_sessions: 10,
            alpha: 0.05,
            baseline_days: 90,
            introduced: None,
            measure: None,
        }
    }

    #[test]
    fn a_check_run_lesson_shows_its_intervals_with_the_introduced_findings() {
        use crate::evaluate::{Introduced, Method, Unit};
        let mut e = evaluation("c", Verdict::Collecting);
        e.unit = Some(Unit::Interval);
        e.method = Some(Method::ConditionalBinomial);
        e.before.n = 0;
        e.introduced = Some(Introduced {
            since: "2026-11-01T00:00:00Z".into(),
            since_date: Some("2026-11-01".into()),
            latest: "2026-11-05T00:00:00Z".into(),
            latest_run: "r5.jsonl".into(),
            reviewed: None,
            count: 2,
            findings: vec![],
            version_changed: Default::default(),
        });
        assert_eq!(
            verdict_line(&e, Status::Adopted, None),
            "collecting 3/10 intervals after, 0/10 before; 2 introduced since the first \
             complete run after adoption, on 2026-11-01 → review (`lab evaluate`)"
        );
        e.verdict = Verdict::Inconclusive;
        assert!(
            verdict_line(&e, Status::Adopted, None)
                .starts_with("inconclusive p=0.004; 2 introduced")
        );
        e.verdict = Verdict::Improved;
        assert_eq!(
            verdict_line(&e, Status::Adopted, None),
            "improved p=0.004 → verify; 2 introduced since the first complete run after \
             adoption, on 2026-11-01 → review (`lab evaluate`)"
        );
        // Without a run since adoption the brief still names intervals.
        e.verdict = Verdict::Collecting;
        e.introduced = None;
        assert_eq!(
            verdict_line(&e, Status::Adopted, None),
            "collecting 3/10 intervals after, 0/10 before; no complete check run since adoption"
        );
    }

    #[test]
    fn measurements_show_each_verdict_and_the_loops_health() {
        use crate::lesson_loop::{MeasureResult, SignalStatus};
        let lessons: Vec<Lesson> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| lesson(id, Kind::Bad, Status::Adopted, Some("2026-11-01"), &[]))
            .collect();
        let mut inconclusive = evaluation("b", Verdict::Inconclusive);
        inconclusive.sessions_needed = Some(340);
        let mut unmeasurable = evaluation("d", Verdict::Unmeasurable);
        unmeasurable.reason = Some("no metric".into());
        let record = Record {
            schema: crate::lesson_loop::SCHEMA.into(),
            started: "2026-11-07T00:00:00Z".into(),
            finished: "2026-11-07T00:00:10Z".into(),
            measure: vec![MeasureResult {
                argv: vec!["agentprof".into(), "scan".into()],
                ok: false,
                status: "exit 2".into(),
            }],
            evaluations: vec![
                evaluation("a", Verdict::Collecting),
                inconclusive,
                evaluation("c", Verdict::Improved),
                unmeasurable,
            ],
            signals: vec![SignalStatus {
                id: "x".into(),
                kind: "k".into(),
                status: SendStatus::Pending,
            }],
            errors: vec![],
        };
        let tz = jiff::tz::TimeZone::get("Asia/Tokyo").unwrap();
        let now: jiff::Timestamp = "2026-11-07T02:00:10Z".parse().unwrap();
        let text = measurements(Ok(Some(record.clone())), now, &tz, &lessons, &[]);
        assert!(
            text.starts_with(
                "\n## Lesson measurements\n\nLast measured 2026-11-07 09:00 (2 h ago).\n\n"
            ),
            "{text}"
        );
        for line in [
            "- `a`: collecting 3/10\n",
            "- `b`: inconclusive, about 340 more sessions\n",
            "- `c`: improved p=0.004 → verify\n",
            "- `d`: unmeasurable: no metric\n",
            "- `e`: not measured yet\n",
            "Measure commands failed: `agentprof scan` (exit 2).\n",
            "1 signal(s) pending",
        ] {
            assert!(text.contains(line), "{line:?} in\n{text}");
        }
        assert!(!text.contains("hooks"), "{text}");
        let later: jiff::Timestamp = "2026-11-09T00:00:10Z".parse().unwrap();
        let stale = measurements(Ok(Some(record)), later, &tz, &lessons, &[]);
        assert!(
            stale.contains("(2 days ago). That is over a day: the hooks"),
            "{stale}"
        );
        let never = measurements(Ok(None), now, &tz, &lessons, &[]);
        assert!(never.ends_with("The loop has never run: run `lab loop`.\n"));
        let with = render(
            "demo",
            "2026-11-30",
            &lessons,
            &[],
            &[],
            &[],
            0,
            Some(&never),
        );
        assert!(with.contains("## Last check") && with.contains("## Lesson measurements"));
        assert!(!with.contains("not yet verified"), "{with}");
        assert!(
            render("demo", "2026-11-30", &lessons, &[], &[], &[], 0, None)
                .contains("not yet verified")
        );
    }

    #[test]
    fn archive_section_names_the_start_page_and_indexes() {
        let a = Archive {
            start: Some(("docs/START.md".into(), Some(6246))),
            indexes: vec!["docs/history/INDEX.md".into()],
        };
        assert_eq!(
            archive(&a),
            "\n## Archive\n\n- Read docs/START.md first (6.1 KB).\n\
             - Look things up: `lab archive find <words>`, or the indexes docs/history/INDEX.md.\n\
             - Before finishing: `lab archive update`, then `lab check --new-only`.\n"
        );
        let missing = archive(&Archive {
            start: Some(("docs/START.md".into(), None)),
            indexes: vec![],
        });
        assert!(missing.contains("The start page docs/START.md does not exist yet"));
        assert!(missing.contains("`lab archive find <words>`.\n"));
    }
}
