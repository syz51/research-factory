//! `lab evaluate`: whether each active lesson's metric changed after adoption.
//!
//! Agent-session metrics are tested within strata of comparable sessions
//! (same source and role): a finding rule by the exact stratified
//! hypergeometric test on per-session indicators, a numeric session field by
//! the van Elteren test. Check runs repeat the same backlog and are not
//! independent observations, so they are only described.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::check::Run;
use crate::config::EvaluateConfig;
use crate::finding::Finding;
use crate::lessons::{Lesson, Metric, Status};
use crate::retro::Session;
use crate::stats;

pub const SCHEMA: &str = "rf.evaluation/1";

/// A metric rule with this prefix is a numeric field of `rf.session/1`.
pub const SESSION_FIELD: &str = "session.";

/// `sessions_needed` searches no further than this.
const NEEDED_LIMIT: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Fewer than `min_sessions` observations on a side.
    Collecting,
    /// Lower after adoption: `p_lower < alpha/2`, or a lower mean for check runs.
    Improved,
    /// Higher after adoption.
    Worse,
    /// Enough data, and neither direction is shown.
    Inconclusive,
    /// This metric cannot show an effect; `reason` says why.
    Unmeasurable,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Collecting => "collecting",
            Self::Improved => "improved",
            Self::Worse => "worse",
            Self::Inconclusive => "inconclusive",
            Self::Unmeasurable => "unmeasurable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Session,
    Run,
}

impl Unit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Run => "run",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    StratifiedHypergeometric,
    VanElteren,
    /// Means compared without a test (check runs).
    Descriptive,
}

/// One side of the adoption date.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Side {
    pub n: usize,
    /// Sessions with at least one finding of the rule.
    pub positives: Option<u64>,
    /// `positives / n`.
    pub rate: Option<f64>,
    pub mean: Option<f64>,
    pub median: Option<f64>,
}

/// A stratum that contributes to the test: sessions before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stratum {
    pub source: String,
    pub role: String,
    pub before: usize,
    pub after: usize,
}

/// An `rf.evaluation/1` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evaluation {
    pub schema: String,
    pub lesson: String,
    pub lesson_status: Status,
    /// `tool:rule`.
    pub metric: Option<String>,
    pub unit: Option<Unit>,
    pub method: Option<Method>,
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub adopted: Option<String>,
    pub before: Side,
    pub after: Side,
    pub p_lower: Option<f64>,
    pub p_upper: Option<f64>,
    pub strata: Vec<Stratum>,
    /// Sessions left out because their stratum has none on the other side.
    pub excluded: usize,
    /// Estimated clean after-sessions still needed to show a decrease.
    pub sessions_needed: Option<u64>,
    pub min_sessions: usize,
    pub alpha: f64,
    pub baseline_days: u32,
}

impl Evaluation {
    fn new(lesson: &Lesson, settings: &EvaluateConfig) -> Self {
        Self {
            schema: SCHEMA.into(),
            lesson: lesson.meta.id.clone(),
            lesson_status: lesson.meta.status,
            metric: lesson
                .meta
                .metric
                .as_ref()
                .map(|m| format!("{}:{}", m.tool, m.rule)),
            unit: None,
            method: None,
            verdict: Verdict::Unmeasurable,
            reason: None,
            adopted: lesson.meta.adopted.clone(),
            before: Side::default(),
            after: Side::default(),
            p_lower: None,
            p_upper: None,
            strata: Vec::new(),
            excluded: 0,
            sessions_needed: None,
            min_sessions: settings.min_sessions,
            alpha: settings.alpha,
            baseline_days: settings.baseline_days,
        }
    }

    fn unmeasurable(mut self, why: impl Into<String>) -> Self {
        self.verdict = Verdict::Unmeasurable;
        self.reason = Some(why.into());
        self
    }

    /// One line of `lab evaluate` text output.
    pub fn line(&self) -> String {
        let mut out = self.lesson.clone();
        match (&self.metric, self.unit) {
            (Some(m), Some(u)) => {
                let _ = write!(out, " ({m}, {}s): ", u.as_str());
            }
            (Some(m), None) => {
                let _ = write!(out, " ({m}): ");
            }
            (None, _) => out.push_str(": "),
        }
        let (b, a) = (&self.before, &self.after);
        if self.unit.is_some() && (b.n > 0 || a.n > 0) {
            let unit = self.unit.map_or("", Unit::as_str);
            if b.rate.is_some() || a.rate.is_some() {
                let pct = |s: &Side| {
                    s.rate
                        .map_or_else(|| "-".into(), |r| format!("{:.1}%", r * 100.0))
                };
                let _ = write!(out, "{} → {} of {unit}s", pct(b), pct(a));
            } else if self.method == Some(Method::VanElteren) {
                let _ = write!(
                    out,
                    "median {} → {}, mean {} → {}",
                    num(b.median),
                    num(a.median),
                    num(b.mean),
                    num(a.mean)
                );
            } else {
                let _ = write!(out, "{} → {} per {unit}", num(b.mean), num(a.mean));
            }
            let _ = write!(out, " (n {} → {}", b.n, a.n);
            if self.excluded > 0 {
                let _ = write!(out, "; {} in unmatched strata left out", self.excluded);
            }
            out.push(')');
            if let (Some(lo), Some(up)) = (self.p_lower, self.p_upper) {
                let _ = write!(out, ", p↓ {} p↑ {}", p_value(lo), p_value(up));
            }
            out.push_str(": ");
        }
        out.push_str(self.verdict.as_str());
        if let Some(r) = &self.reason {
            let _ = write!(out, " ({r})");
        }
        if let Some(m) = self.sessions_needed
            && matches!(self.verdict, Verdict::Collecting | Verdict::Inconclusive)
        {
            let _ = write!(
                out,
                "; about {m} more clean session(s) in the baseline's mix would show a decrease"
            );
        }
        if self.method == Some(Method::Descriptive)
            && matches!(self.verdict, Verdict::Improved | Verdict::Worse)
        {
            out.push_str(" (descriptive: no significance test)");
        }
        out
    }
}

fn num(v: Option<f64>) -> String {
    match v {
        None => "-".into(),
        Some(v) if v.abs() >= 1000.0 || v.fract() == 0.0 => format!("{v:.0}"),
        Some(v) => format!("{v:.2}"),
    }
}

/// A p-value at a precision the tests support.
pub fn p_value(p: f64) -> String {
    if p < 0.001 {
        "<0.001".into()
    } else if p < 0.01 {
        format!("{p:.3}")
    } else {
        format!("{p:.2}")
    }
}

/// `p=0.004`, or `p<0.001`.
pub fn p_label(p: f64) -> String {
    let v = p_value(p);
    if v.starts_with('<') {
        format!("p{v}")
    } else {
        format!("p={v}")
    }
}

/// Everything a lesson is measured against.
pub struct Inputs<'a> {
    /// Stored check runs, oldest first.
    pub runs: &'a [Run],
    /// Configured check names.
    pub configured: &'a [&'a str],
    /// Agent-session findings (`rf.finding/1` from agentprof).
    pub findings: &'a [Finding],
    /// Agent sessions, or why they could not be read.
    pub sessions: Result<&'a [Session], &'a str>,
    pub settings: &'a EvaluateConfig,
}

/// Where an observation dated `d` falls: `Some(false)` before adoption and
/// inside the before-window, `Some(true)` on or after the adoption date.
fn side_of(
    d: jiff::civil::Date,
    adopted: jiff::civil::Date,
    window_start: Option<jiff::civil::Date>,
) -> Option<bool> {
    if d >= adopted {
        Some(true)
    } else if window_start.is_none_or(|w| d >= w) {
        Some(false)
    } else {
        None
    }
}

/// Measures one lesson. Observational: other changes in the same period can
/// explain a difference.
pub fn evaluate(
    lesson: &Lesson,
    inputs: &Inputs,
    date_of: impl Fn(&str) -> Option<jiff::civil::Date>,
) -> Evaluation {
    let e = Evaluation::new(lesson, inputs.settings);
    let Some(metric) = lesson.meta.metric.as_ref() else {
        return e.unmeasurable("no metric");
    };
    let Some(adopted) = lesson
        .meta
        .adopted
        .as_deref()
        .and_then(|a| a.parse::<jiff::civil::Date>().ok())
    else {
        return e.unmeasurable("no adoption date");
    };
    let days = inputs.settings.baseline_days;
    let window_start = (days > 0)
        .then(|| {
            adopted
                .checked_sub(jiff::Span::new().days(i64::from(days)))
                .ok()
        })
        .flatten();
    let place = |at: &str| date_of(at).and_then(|d| side_of(d, adopted, window_start));

    if let Some(path) = metric.rule.strip_prefix(SESSION_FIELD) {
        return sessions(e, metric, Some(path), inputs, place);
    }
    let required: BTreeSet<&str> = inputs
        .runs
        .iter()
        .flat_map(|r| r.items.iter())
        .filter(|i| i.finding.tool == metric.tool && inputs.configured.contains(&i.check.as_str()))
        .map(|i| i.check.as_str())
        .collect();
    if !required.is_empty() {
        return check_runs(e, metric, &required, inputs, place);
    }
    if inputs.findings.iter().any(|f| f.tool == metric.tool) {
        return sessions(e, metric, None, inputs, place);
    }
    e.unmeasurable(format!(
        "no configured check or agent session has reported a `{}` finding",
        metric.tool
    ))
}

/// Means per complete check run, without a test.
///
/// A run counts when every configured check that has reported the metric's
/// tool ran to completion in it, so a failed or partial (`--only`) run is
/// not mistaken for a clean one.
fn check_runs(
    mut e: Evaluation,
    metric: &Metric,
    required: &BTreeSet<&str>,
    inputs: &Inputs,
    place: impl Fn(&str) -> Option<bool>,
) -> Evaluation {
    e.unit = Some(Unit::Run);
    e.method = Some(Method::Descriptive);
    let (mut before, mut after) = (Vec::new(), Vec::new());
    for run in inputs.runs {
        let complete = required
            .iter()
            .all(|name| run.header.checks.iter().any(|c| c.name == *name && c.ran()));
        if !complete {
            continue;
        }
        let count = run
            .items
            .iter()
            .filter(|i| {
                required.contains(i.check.as_str())
                    && i.finding.tool == metric.tool
                    && i.finding.rule == metric.rule
            })
            .count() as f64;
        match place(&run.header.at) {
            Some(true) => after.push(count),
            Some(false) => before.push(count),
            None => {}
        }
    }
    e.before = describe(&before);
    e.after = describe(&after);
    let min = inputs.settings.min_sessions;
    if before.is_empty() {
        return e.unmeasurable("no complete check run before adoption");
    }
    if before.len() < min {
        return e.unmeasurable(format!(
            "only {} complete check run(s) before adoption, fewer than {min}",
            before.len()
        ));
    }
    if after.len() < min {
        e.verdict = Verdict::Collecting;
        e.reason = Some(format!("{} of {min} runs after adoption", after.len()));
        return e;
    }
    let (b, a) = (e.before.mean.unwrap_or(0.0), e.after.mean.unwrap_or(0.0));
    e.verdict = if a < b {
        Verdict::Improved
    } else if a > b {
        Verdict::Worse
    } else {
        Verdict::Inconclusive
    };
    e
}

fn describe(values: &[f64]) -> Side {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    Side {
        n,
        positives: None,
        rate: None,
        mean: (n > 0).then(|| sorted.iter().sum::<f64>() / n as f64),
        median: (n > 0).then(|| {
            if n % 2 == 1 {
                sorted[n / 2]
            } else {
                (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
            }
        }),
    }
}

fn indicator_side(values: &[f64]) -> Side {
    let positives = values.iter().filter(|&&v| v > 0.0).count() as u64;
    Side {
        n: values.len(),
        positives: Some(positives),
        rate: (!values.is_empty()).then(|| positives as f64 / values.len() as f64),
        mean: None,
        median: None,
    }
}

/// One stratum's values before and after adoption.
type Sides = (Vec<f64>, Vec<f64>);

/// Per-session evaluation: a finding rule as a 0/1 indicator per session
/// (`field` is `None`), or a numeric field of the session record.
fn sessions(
    mut e: Evaluation,
    metric: &Metric,
    field: Option<&str>,
    inputs: &Inputs,
    place: impl Fn(&str) -> Option<bool>,
) -> Evaluation {
    e.unit = Some(Unit::Session);
    let sessions = match inputs.sessions {
        Ok(s) => s,
        Err(why) => return e.unmeasurable(format!("sessions file unreadable: {why}")),
    };
    let flagged: BTreeSet<&str> = inputs
        .findings
        .iter()
        .filter(|f| f.tool == metric.tool && f.rule == metric.rule)
        .map(|f| f.subject.as_str())
        .collect();
    // (source, role) → (before values, after values)
    let mut strata: BTreeMap<(&str, &str), Sides> = BTreeMap::new();
    let mut dated = 0;
    for s in sessions {
        let Some(after) = s.start.as_deref().and_then(&place) else {
            continue;
        };
        dated += 1;
        let value = match field {
            None => f64::from(u8::from(flagged.contains(s.subject().as_str()))),
            Some(path) => match s.field(path) {
                Some(v) => v,
                None => continue,
            },
        };
        let entry = strata.entry((s.source.as_str(), s.role())).or_default();
        if after { &mut entry.1 } else { &mut entry.0 }.push(value);
    }
    let all_before: Vec<f64> = strata.values().flat_map(|s| s.0.iter().copied()).collect();
    let all_after: Vec<f64> = strata.values().flat_map(|s| s.1.iter().copied()).collect();
    let contributing: Vec<(&(&str, &str), &Sides)> = strata
        .iter()
        .filter(|(_, (b, a))| !b.is_empty() && !a.is_empty())
        .collect();
    // Only strata with sessions on both sides are compared; when none has,
    // the lesson is still collecting.
    e.excluded = all_before.len() + all_after.len()
        - contributing
            .iter()
            .map(|(_, (b, a))| b.len() + a.len())
            .sum::<usize>();
    e.strata = contributing
        .iter()
        .map(|((source, role), (b, a))| Stratum {
            source: (*source).into(),
            role: (*role).into(),
            before: b.len(),
            after: a.len(),
        })
        .collect();
    let (before, after): (Vec<f64>, Vec<f64>) = (
        contributing
            .iter()
            .flat_map(|(_, s)| s.0.iter().copied())
            .collect(),
        contributing
            .iter()
            .flat_map(|(_, s)| s.1.iter().copied())
            .collect(),
    );
    let (e_before, e_after) = match field {
        None => (indicator_side(&before), indicator_side(&after)),
        Some(_) => (describe(&before), describe(&after)),
    };
    e.before = e_before;
    e.after = e_after;
    e.method = Some(match field {
        None => Method::StratifiedHypergeometric,
        Some(_) => Method::VanElteren,
    });
    let min = inputs.settings.min_sessions;
    let half = inputs.settings.alpha / 2.0;

    if let Some(path) = field
        && all_before.is_empty()
        && all_after.is_empty()
    {
        return e.unmeasurable(if dated == 0 {
            "no comparable baseline sessions".to_string()
        } else {
            format!(
                "metric never observed in comparable sessions: no session has a numeric `{path}`"
            )
        });
    }
    if all_before.is_empty() {
        return e.unmeasurable("no comparable baseline sessions");
    }
    if all_before.len() < min {
        return e.unmeasurable(format!(
            "no comparable baseline: only {} session(s) before adoption, fewer than {min}",
            all_before.len()
        ));
    }
    if field.is_none() && !all_before.iter().chain(&all_after).any(|&v| v > 0.0) {
        return e.unmeasurable("metric never observed in comparable sessions");
    }
    // Unstratified, over every dated session: the finding may so far occur
    // only in strata that have no session after adoption yet.
    let positives = |v: &[f64]| v.iter().filter(|&&x| x > 0.0).count() as u64;
    let pooled = stats::Counts {
        before: all_before.len() as u64,
        before_positive: positives(&all_before),
        after: all_after.len() as u64,
        after_positive: positives(&all_after),
    };
    // Rule metrics: how many more clean sessions would show a decrease, or
    // why none can.
    let needed = |mut e: Evaluation| -> Evaluation {
        if field.is_some() {
            return e;
        }
        e.sessions_needed = stats::sessions_needed(pooled, half, NEEDED_LIMIT);
        if e.sessions_needed == Some(0) {
            // The pooled estimate already shows a decrease that the
            // stratified test does not: it has no count to offer.
            e.sessions_needed = None;
            return e;
        }
        if e.sessions_needed.is_none() {
            let why = if pooled.before_positive == 0 {
                "no finding in the sessions before adoption, so a decrease cannot be shown"
            } else {
                "too rare to show a decrease"
            };
            e = e.unmeasurable(why);
        }
        e
    };
    if before.len() < min || after.len() < min {
        e.verdict = Verdict::Collecting;
        e.reason = Some(if contributing.is_empty() {
            "no (source, role) stratum has sessions both before and after adoption yet".into()
        } else {
            format!(
                "{} of {min} sessions after adoption, {} of {min} before, in comparable strata",
                after.len(),
                before.len()
            )
        });
        return needed(e);
    }
    let (lo, up) = match field {
        None => stats::stratified_hypergeometric(
            &contributing
                .iter()
                .map(|(_, (b, a))| stats::Counts {
                    before: b.len() as u64,
                    before_positive: positives(b),
                    after: a.len() as u64,
                    after_positive: positives(a),
                })
                .collect::<Vec<_>>(),
        ),
        Some(_) => {
            let samples: Vec<stats::Samples> = contributing
                .iter()
                .map(|(_, (b, a))| stats::Samples {
                    before: b.clone(),
                    after: a.clone(),
                })
                .collect();
            match stats::van_elteren(&samples) {
                Some((_, lo, up)) => (lo, up),
                None => {
                    return e
                        .unmeasurable("the field has the same value in every comparable session");
                }
            }
        }
    };
    e.p_lower = Some(lo);
    e.p_upper = Some(up);
    if lo < half {
        e.verdict = Verdict::Improved;
        e
    } else if up < half {
        e.verdict = Verdict::Worse;
        e
    } else {
        e.verdict = Verdict::Inconclusive;
        needed(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{CheckOutcome, ITEM_SCHEMA, Item, RUN_SCHEMA, RunHeader};
    use crate::finding::{self, Severity};
    use crate::lessons::{Kind, Meta};

    fn lesson(metric: &str, adopted: &str) -> Lesson {
        Lesson {
            path: "l.md".into(),
            meta: Meta {
                id: "l".into(),
                title: "t".into(),
                kind: Kind::Bad,
                status: Status::Adopted,
                scope: "workflow".into(),
                tags: vec![],
                rule: "r".into(),
                metric: Some(metric.parse().unwrap()),
                created: "2026-01-01".into(),
                updated: adopted.into(),
                adopted: Some(adopted.into()),
                evidence: vec![],
            },
            body: String::new(),
        }
    }

    fn utc_date(s: &str) -> Option<jiff::civil::Date> {
        s.parse::<jiff::Timestamp>()
            .ok()
            .map(|t| t.to_zoned(jiff::tz::TimeZone::UTC).date())
    }

    fn session(source: &str, role: &str, id: &str, start: &str, extra: &str) -> Session {
        let line = format!(
            r#"{{"schema":"rf.session/1","source":"{source}","id":"{id}","role":"{role}","start":"{start}"{extra}}}"#
        );
        Session::parse(&line).unwrap()
    }

    fn agent_finding(subject: &str) -> Finding {
        let mut f = Finding::lab("file-reread", Severity::Warn, subject, "m".into());
        f.tool = "agentprof".into();
        f
    }

    /// `n` sessions of one stratum on `date`, the first `positive` of them flagged.
    fn batch(
        sessions: &mut Vec<Session>,
        findings: &mut Vec<Finding>,
        (source, role): (&str, &str),
        date: &str,
        n: usize,
        positive: usize,
    ) {
        for i in 0..n {
            let id = format!("{source}-{role}-{date}-{i}");
            sessions.push(session(source, role, &id, &format!("{date}T12:00:00Z"), ""));
            if i < positive {
                findings.push(agent_finding(&format!("{source}:{id}")));
            }
        }
    }

    fn run_eval(
        l: &Lesson,
        sessions: &[Session],
        findings: &[Finding],
        cfg: &EvaluateConfig,
    ) -> Evaluation {
        let inputs = Inputs {
            runs: &[],
            configured: &[],
            findings,
            sessions: Ok(sessions),
            settings: cfg,
        };
        evaluate(l, &inputs, utc_date)
    }

    #[test]
    fn stratified_rule_metric_matches_the_exact_test() {
        let (mut s, mut f) = (Vec::new(), Vec::new());
        batch(&mut s, &mut f, ("claude", "main"), "2026-10-01", 100, 10);
        batch(&mut s, &mut f, ("claude", "main"), "2026-11-05", 20, 0);
        batch(&mut s, &mut f, ("codex", "subagent"), "2026-10-02", 40, 2);
        batch(&mut s, &mut f, ("codex", "subagent"), "2026-11-06", 30, 6);
        // A stratum seen only before adoption is excluded.
        batch(&mut s, &mut f, ("t3", "main"), "2026-10-03", 7, 7);
        let e = run_eval(
            &lesson("agentprof:file-reread", "2026-11-01"),
            &s,
            &f,
            &EvaluateConfig::default(),
        );
        assert_eq!(e.method, Some(Method::StratifiedHypergeometric));
        assert!((e.p_lower.unwrap() - 0.7935927405628815).abs() < 1e-9);
        assert!((e.p_upper.unwrap() - 0.39732582580895653).abs() < 1e-9);
        assert_eq!(e.verdict, Verdict::Inconclusive);
        assert_eq!(e.excluded, 7);
        assert_eq!(e.strata.len(), 2);
        assert_eq!((e.before.n, e.before.positives), (140, Some(12)));
        assert_eq!((e.after.n, e.after.positives), (50, Some(6)));
        assert!(e.sessions_needed.is_some());
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""schema":"rf.evaluation/1""#)
                && json.contains(r#""method":"stratified-hypergeometric""#),
            "{json}"
        );
    }

    #[test]
    fn few_sessions_are_collecting_not_improved() {
        // 0 of 12 after vs 10 of 1000 before: the old mean comparison called this an improvement.
        let (mut s, mut f) = (Vec::new(), Vec::new());
        batch(&mut s, &mut f, ("claude", "main"), "2026-10-01", 1000, 10);
        batch(&mut s, &mut f, ("claude", "main"), "2026-11-02", 12, 0);
        let l = lesson("agentprof:file-reread", "2026-11-01");
        let cfg = EvaluateConfig {
            min_sessions: 20,
            ..EvaluateConfig::default()
        };
        let e = run_eval(&l, &s, &f, &cfg);
        assert_eq!(e.verdict, Verdict::Collecting, "{e:?}");
        assert!(
            e.reason
                .as_deref()
                .unwrap()
                .starts_with("12 of 20 sessions after"),
            "{e:?}"
        );
        assert_eq!(e.sessions_needed, Some(433));
        assert!(
            e.line().contains("about 433 more clean session(s)"),
            "{}",
            e.line()
        );
        // With enough sessions the test still finds no evidence.
        let e = run_eval(&l, &s, &f, &EvaluateConfig::default());
        assert_eq!(e.verdict, Verdict::Inconclusive);
        assert!(e.p_lower.unwrap() > 0.5);
        // Many clean sessions do show it.
        batch(&mut s, &mut f, ("claude", "main"), "2026-11-03", 500, 0);
        let e = run_eval(&l, &s, &f, &EvaluateConfig::default());
        assert_eq!(e.verdict, Verdict::Improved, "{e:?}");
        assert_eq!(e.sessions_needed, None);
    }

    #[test]
    fn strata_without_both_sides_are_collecting() {
        let (mut s, mut f) = (Vec::new(), Vec::new());
        batch(&mut s, &mut f, ("claude", "unknown"), "2026-10-01", 100, 50);
        batch(&mut s, &mut f, ("claude", "main"), "2026-11-02", 100, 0);
        let l = lesson("agentprof:file-reread", "2026-11-01");
        let e = run_eval(&l, &s, &f, &EvaluateConfig::default());
        assert_eq!(e.verdict, Verdict::Collecting, "{e:?}");
        assert_eq!((e.before.n, e.after.n, e.excluded), (0, 0, 200));
        assert!(e.strata.is_empty());
        assert!(e.p_lower.is_none());
        assert!(
            e.reason
                .as_deref()
                .unwrap()
                .starts_with("no (source, role) stratum"),
            "{e:?}"
        );
        // The pooled estimate says none are needed; that is not shown.
        assert_eq!(e.sessions_needed, None);
    }

    #[test]
    fn baseline_window_and_unmeasurable_reasons() {
        let (mut s, mut f) = (Vec::new(), Vec::new());
        batch(&mut s, &mut f, ("claude", "main"), "2025-01-01", 100, 10);
        batch(&mut s, &mut f, ("claude", "main"), "2026-11-02", 30, 0);
        let l = lesson("agentprof:file-reread", "2026-11-01");
        let e = run_eval(&l, &s, &f, &EvaluateConfig::default());
        assert_eq!(e.verdict, Verdict::Unmeasurable);
        assert_eq!(e.reason.as_deref(), Some("no comparable baseline sessions"));
        let all = EvaluateConfig {
            baseline_days: 0,
            ..EvaluateConfig::default()
        };
        assert_ne!(run_eval(&l, &s, &f, &all).verdict, Verdict::Unmeasurable);

        // Never observed on either side.
        let (mut s, mut f) = (Vec::new(), Vec::new());
        batch(&mut s, &mut f, ("claude", "main"), "2026-10-01", 50, 0);
        batch(&mut s, &mut f, ("claude", "main"), "2026-11-02", 50, 0);
        f.push({
            let mut x = agent_finding("claude:elsewhere");
            x.rule = "other-rule".into();
            x
        });
        let e = run_eval(&l, &s, &f, &EvaluateConfig::default());
        assert_eq!(
            e.reason.as_deref(),
            Some("metric never observed in comparable sessions")
        );

        let e = evaluate(
            &l,
            &Inputs {
                runs: &[],
                configured: &[],
                findings: &f,
                sessions: Err("sessions.jsonl: not found"),
                settings: &EvaluateConfig::default(),
            },
            utc_date,
        );
        assert_eq!(
            e.reason.as_deref(),
            Some("sessions file unreadable: sessions.jsonl: not found")
        );
        let mut none = l.clone();
        none.meta.metric = None;
        assert_eq!(
            run_eval(&none, &s, &f, &EvaluateConfig::default())
                .reason
                .as_deref(),
            Some("no metric")
        );
    }

    #[test]
    fn session_fields_use_van_elteren() {
        let before = [5., 7., 7., 9., 12., 15., 15., 20., 22., 30.];
        let after = [3., 4., 7., 8., 9., 10., 11.];
        let mut s = Vec::new();
        for (i, v) in before.iter().enumerate() {
            s.push(session(
                "claude",
                "main",
                &format!("b{i}"),
                "2026-10-01T00:00:00Z",
                &format!(r#","total":{{"tokens":{{"output":{v}}}}}"#),
            ));
        }
        for (i, v) in after.iter().enumerate() {
            s.push(session(
                "claude",
                "main",
                &format!("a{i}"),
                "2026-11-02T00:00:00Z",
                &format!(r#","total":{{"tokens":{{"output":{v}}}}}"#),
            ));
        }
        // Missing and null fields skip the session.
        s.push(session(
            "claude",
            "main",
            "x",
            "2026-11-02T00:00:00Z",
            r#","total":{"tokens":{"output":null}}"#,
        ));
        s.push(session("claude", "main", "y", "2026-11-02T00:00:00Z", ""));
        let cfg = EvaluateConfig {
            min_sessions: 5,
            ..EvaluateConfig::default()
        };
        let l = lesson("agentprof:session.total.tokens.output", "2026-11-01");
        let e = run_eval(&l, &s, &[], &cfg);
        assert_eq!(e.method, Some(Method::VanElteren));
        assert!(
            (e.p_lower.unwrap() - 0.03498618267703823).abs() < 1e-6,
            "{e:?}"
        );
        assert_eq!(
            e.verdict,
            Verdict::Inconclusive,
            "0.035 is not below alpha/2"
        );
        assert_eq!((e.before.n, e.after.n), (10, 7));
        assert_eq!((e.before.median, e.after.median), (Some(13.5), Some(8.0)));
        let loose = EvaluateConfig {
            alpha: 0.1,
            ..cfg.clone()
        };
        assert_eq!(run_eval(&l, &s, &[], &loose).verdict, Verdict::Improved);
        let typo = lesson("agentprof:session.total.tokens.outptu", "2026-11-01");
        let e = run_eval(&typo, &s, &[], &cfg);
        assert_eq!(e.verdict, Verdict::Unmeasurable);
        assert!(e.reason.unwrap().contains("never observed"));
    }

    /// A run of the named checks; each has `rules` quote-not-found findings
    /// and either completed (`ran`) or failed.
    fn run_checks(at: &str, checks: &[(&str, usize, bool)]) -> Run {
        let mut items = Vec::new();
        let mut outcomes = Vec::new();
        for &(name, rules, ran) in checks {
            let findings: Vec<Finding> = (0..rules)
                .map(|i| {
                    let mut f = Finding::lab(
                        "quote-not-found",
                        Severity::Error,
                        &format!("{name}.md:{i}"),
                        format!("m{i}"),
                    );
                    f.tool = "claimcheck".into();
                    f
                })
                .collect();
            let fps = finding::fingerprints(&findings);
            items.extend(
                findings
                    .into_iter()
                    .zip(fps)
                    .map(|(finding, fingerprint)| Item {
                        schema: ITEM_SCHEMA.into(),
                        check: name.into(),
                        fingerprint,
                        finding,
                    }),
            );
            outcomes.push(CheckOutcome {
                name: name.into(),
                command: vec![],
                exit: Some(if ran { 1 } else { 2 }),
                status: if ran { "ok" } else { "failed" }.into(),
                findings: rules,
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

    fn run(at: &str, rules: usize, ran: bool) -> Run {
        run_checks(at, &[("claims", rules, ran)])
    }

    #[test]
    fn partial_runs_are_not_counted_and_unconfigured_checks_not_required() {
        let l = lesson("claimcheck:quote-not-found", "2026-11-03");
        let cfg = EvaluateConfig {
            min_sessions: 2,
            ..EvaluateConfig::default()
        };
        let both = |at: &str| run_checks(at, &[("docs", 2, true), ("thesis", 2, true)]);
        let docs_only = |at: &str| run_checks(at, &[("docs", 0, true)]);
        let runs = vec![
            both("2026-11-01T00:00:00Z"),
            both("2026-11-02T00:00:00Z"),
            docs_only("2026-11-04T00:00:00Z"),
            docs_only("2026-11-05T00:00:00Z"),
        ];
        let eval = |configured: &[&str]| {
            evaluate(
                &l,
                &Inputs {
                    runs: &runs,
                    configured,
                    findings: &[],
                    sessions: Err("none"),
                    settings: &cfg,
                },
                utc_date,
            )
        };
        // `thesis` reports the tool, so a run without it is partial, not clean.
        let e = eval(&["docs", "thesis"]);
        assert_eq!(
            (e.before.n, e.after.n, e.verdict),
            (2, 0, Verdict::Collecting)
        );
        // Once `thesis` is no longer configured, it is not required.
        let e = eval(&["docs"]);
        assert_eq!((e.before.n, e.after.n), (2, 2));
        assert_eq!((e.before.mean, e.after.mean), (Some(2.0), Some(0.0)));
        assert_eq!(e.verdict, Verdict::Improved);
    }

    #[test]
    fn check_runs_are_descriptive_and_gated_by_min_sessions() {
        let l = lesson("claimcheck:quote-not-found", "2026-11-03");
        let cfg = EvaluateConfig {
            min_sessions: 2,
            ..EvaluateConfig::default()
        };
        let eval = |runs: &[Run]| {
            evaluate(
                &l,
                &Inputs {
                    runs,
                    configured: &["claims"],
                    findings: &[],
                    sessions: Err("none"),
                    settings: &cfg,
                },
                utc_date,
            )
        };
        let mut runs = vec![
            run("2026-11-01T00:00:00Z", 2, true),
            run("2026-11-02T00:00:00Z", 2, true),
            run("2026-11-03T00:00:00Z", 1, true),
        ];
        let e = eval(&runs);
        assert_eq!(
            (e.before.n, e.after.n, e.verdict),
            (2, 1, Verdict::Collecting)
        );
        runs.push(run("2026-11-04T00:00:00Z", 0, false)); // a failed run is not a clean one
        runs.push(run("2026-11-05T00:00:00Z", 0, true));
        let e = eval(&runs);
        assert_eq!((e.before.n, e.after.n), (2, 2));
        assert_eq!((e.before.mean, e.after.mean), (Some(2.0), Some(0.5)));
        assert_eq!(
            (e.verdict, e.method, e.unit),
            (
                Verdict::Improved,
                Some(Method::Descriptive),
                Some(Unit::Run)
            )
        );
        assert!(e.line().contains("descriptive"), "{}", e.line());
        let e = eval(&runs[2..]);
        assert_eq!(
            e.reason.as_deref(),
            Some("no complete check run before adoption")
        );
    }
}
