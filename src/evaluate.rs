//! `lab evaluate`: whether each active lesson's metric changed after adoption.
//!
//! Agent-session metrics are measured per session and tested within strata
//! of comparable sessions (same source and role). The subagents below one
//! root session share its task, so they are not independent: they form one
//! session group, the unit counted and moved by the test. Without such
//! groups, a finding rule is tested by the exact stratified hypergeometric
//! test on per-session indicators and a numeric session field by the van
//! Elteren test; with them, both by a stratified cluster permutation test.
//! Check runs repeat the same backlog and are not independent observations,
//! so they are described by the findings introduced since adoption, and
//! judged only through intervals between clean, measured runs on consecutive
//! commits: findings introduced per new unit of checked text, compared by the
//! exact conditional binomial test.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::check::{Item, Run};
use crate::config::EvaluateConfig;
use crate::finding::Finding;
use crate::lessons::{Lesson, Metric, Status};
use crate::retro::{self, Session};
use crate::stats;

pub const SCHEMA: &str = "rf.evaluation/2";

/// A metric rule with this prefix is a numeric field of `rf.session/1`.
pub const SESSION_FIELD: &str = "session.";

/// `sessions_needed` searches no further than this.
const NEEDED_LIMIT: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Fewer than `min_sessions` observations on a side. Check runs without
    /// measured intervals (`Method::Descriptive`) stay here: they are
    /// described by `introduced`.
    Collecting,
    /// Lower after adoption: `p_lower < alpha/2`.
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
    /// One agent session: `rf.evaluation/1` records.
    Session,
    /// A root session, or the subagents below one root session.
    #[serde(rename = "session-group")]
    SessionGroup,
    Run,
    /// The text between two clean, measured check runs on consecutive commits.
    Interval,
}

impl Unit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::SessionGroup => "session group",
            Self::Run => "run",
            Self::Interval => "interval",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    StratifiedHypergeometric,
    VanElteren,
    /// Check runs: introduced findings, without a test or verdict.
    Descriptive,
    /// Check-run intervals: introduced findings per new unit, the after
    /// side's count binomial given the total and the share of new units.
    ConditionalBinomial,
    /// Whole session groups permuted between the sides (agent sessions
    /// with subagents).
    ClusterPermutation,
}

/// One side of the adoption date.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Side {
    /// Observations: session groups, sessions (version 1), check runs or
    /// check-run intervals.
    pub n: usize,
    /// The agent sessions in the `n` session groups; `None` for other units.
    #[serde(default)]
    pub sessions: Option<usize>,
    /// Sessions with at least one finding of the rule.
    pub positives: Option<u64>,
    /// `positives` per session (per check run for check-run metrics).
    pub rate: Option<f64>,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    /// Check-run intervals: the findings introduced in them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<u64>,
    /// Check-run intervals: the new units of checked text in them; `rate`
    /// is `findings` per new unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_units: Option<u64>,
}

/// A stratum that contributes to the test: observations before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stratum {
    pub source: String,
    pub role: String,
    pub before: usize,
    pub after: usize,
}

/// An `rf.evaluation/2` record.
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
    /// Observations left out because their stratum has none on the other side.
    pub excluded: usize,
    /// Estimated clean observations after adoption still needed to show a
    /// decrease, in the evaluation's unit.
    pub sessions_needed: Option<u64>,
    pub min_sessions: usize,
    pub alpha: f64,
    pub baseline_days: u32,
    /// Check-run metrics: findings introduced since adoption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<Introduced>,
    /// Check-run intervals: the `rf.measure/1` unit they count, such as
    /// `citing-sentence`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<String>,
}

/// Findings of the metric's rule in the latest complete check run beyond
/// their message and data's count in the first complete run on or after
/// adoption, or in the reviewed run when the lesson records a later review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Introduced {
    /// The first complete run on or after adoption, or the reviewed run
    /// (its `at`, UTC).
    pub since: String,
    /// That run's date in the project's time zone.
    #[serde(default)]
    pub since_date: Option<String>,
    /// The latest complete run.
    pub latest: String,
    /// Its file name, which `lab lesson review` records.
    #[serde(default)]
    pub latest_run: String,
    /// The file name of the lesson's last reviewed run, when `since` is it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed: Option<String>,
    /// The sum of `added`.
    pub count: usize,
    pub findings: Vec<IntroducedFinding>,
}

impl Introduced {
    /// "the first complete run after adoption, on 2026-11-10 (adopted
    /// 2026-11-03)": the run's local date, and the adoption date when the
    /// run came later, since violations written in between count as backlog.
    pub fn since_text(&self, adopted: Option<&str>) -> String {
        let date = self.since_date.as_deref().unwrap_or(&self.since);
        if self.reviewed.is_some() {
            return format!("the reviewed run of {date}");
        }
        match adopted {
            Some(a) if a != date => {
                format!("the first complete run after adoption, on {date} (adopted {a})")
            }
            _ => format!("the first complete run after adoption, on {date}"),
        }
    }
}

/// A message and data with more findings in the latest run than in the
/// `since` run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntroducedFinding {
    pub message: String,
    /// Findings beyond the `since` run's count of this message and data.
    pub added: usize,
    /// Where the new findings are: the latest run's subjects for this key
    /// whose fingerprint the `since` run lacks (or all, when none does),
    /// at most ten.
    pub subjects: Vec<String>,
    /// Subjects left out of `subjects`.
    #[serde(default)]
    pub more: usize,
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
            introduced: None,
            measure: None,
        }
    }

    fn unmeasurable(mut self, why: impl Into<String>) -> Self {
        self.verdict = Verdict::Unmeasurable;
        self.reason = Some(why.into());
        self
    }

    /// A check-run lesson: described by `introduced`, judged over intervals.
    pub fn is_check_run(&self) -> bool {
        matches!(self.unit, Some(Unit::Run | Unit::Interval))
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
            if self.unit == Some(Unit::Interval) {
                let per = |s: &Side| num(s.rate.map(|r| r * 100.0));
                let _ = write!(
                    out,
                    "{} → {} introduced per 100 new {}s",
                    per(b),
                    per(a),
                    self.measure.as_deref().unwrap_or("unit")
                );
            } else if b.rate.is_some() || a.rate.is_some() {
                let pct = |s: &Side| {
                    s.rate
                        .map_or_else(|| "-".into(), |r| format!("{:.1}%", r * 100.0))
                };
                let _ = write!(out, "{} → {} of sessions", pct(b), pct(a));
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
            if let (Some(fb), Some(ub), Some(fa), Some(ua)) =
                (b.findings, b.new_units, a.findings, a.new_units)
            {
                let _ = write!(out, " {unit}s: {fb} in {ub} → {fa} in {ua} new units");
            }
            if let (Some(sb), Some(sa)) = (b.sessions, a.sessions) {
                let _ = write!(out, " {unit}s from {sb} → {sa} sessions");
            }
            if self.excluded > 0 {
                let _ = write!(out, "; {} in unmatched strata left out", self.excluded);
            }
            if let Some(i) = &self.introduced {
                let _ = write!(
                    out,
                    "; {} introduced since {}",
                    i.count,
                    i.since_text(self.adopted.as_deref())
                );
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
            let unit = self.unit.map_or("session", Unit::as_str);
            let _ = write!(
                out,
                "; about {m} more clean {unit}(s) in the baseline's mix would show a decrease"
            );
        }
        out
    }

    /// The introduced findings, one indented line each, for manual review.
    pub fn introduced_lines(&self) -> Vec<String> {
        let Some(i) = &self.introduced else {
            return Vec::new();
        };
        i.findings
            .iter()
            .map(|f| {
                let mut at = f.subjects.join(", ");
                if f.more > 0 {
                    let _ = write!(at, ", {} more", f.more);
                }
                format!("  +{} {} [{at}]", f.added, f.message)
            })
            .collect()
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
        let reviewed = lesson.meta.reviewed.as_deref();
        return check_runs(e, metric, &required, inputs, reviewed, place, &date_of);
    }
    if inputs.findings.iter().any(|f| f.tool == metric.tool) {
        return sessions(e, metric, None, inputs, place);
    }
    e.unmeasurable(format!(
        "no configured check or agent session has reported a `{}` finding",
        metric.tool
    ))
}

/// Check-run findings introduced since adoption, and a verdict from the
/// intervals between clean, measured runs.
///
/// A run counts when every configured check that has reported the metric's
/// tool ran to completion in it, so a failed or partial (`--only`) run is
/// not mistaken for a clean one.
///
/// Findings are keyed by message and data (the rule is the metric's), not by
/// fingerprint, whose file path makes moved text look new; a checker's data
/// usually holds the sentence, which tells two violations with one message
/// apart. A finding is introduced when the latest complete run has more
/// findings with its key than the first complete run on or after adoption,
/// which holds the violations written before that run. A review (`lab lesson
/// review`) moves that run to the reviewed one, so findings that cannot be
/// fixed, such as those in append-only files, stop counting once reviewed.
/// The baseline plays no part, so a finding stays introduced after
/// `lab baseline set` accepts it.
///
/// The verdict compares findings introduced per new unit of checked text
/// (see `intervals`). Without measured intervals the runs are only
/// described: the means per run before and after adoption are kept as
/// context, since they move with checker scope and backlog work, and the
/// verdict stays `collecting` with `Method::Descriptive`.
fn check_runs(
    mut e: Evaluation,
    metric: &Metric,
    required: &BTreeSet<&str>,
    inputs: &Inputs,
    reviewed: Option<&str>,
    place: impl Fn(&str) -> Option<bool>,
    date_of: impl Fn(&str) -> Option<jiff::civil::Date>,
) -> Evaluation {
    e.unit = Some(Unit::Run);
    e.method = Some(Method::Descriptive);
    let (mut before, mut after) = (Vec::new(), Vec::new());
    // The first and the latest complete run since adoption, and the reviewed
    // one, found by its file name.
    let mut first: Option<&Run> = None;
    let mut reviewed_run: Option<&Run> = None;
    let mut last: Option<&Run> = None;
    let mut observed = false;
    // Clean, measured runs: the last one on each commit.
    let mut by_commit: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, run) in inputs.runs.iter().enumerate() {
        let complete = required
            .iter()
            .all(|name| run.header.checks.iter().any(|c| c.name == *name && c.ran()));
        if !complete {
            continue;
        }
        let count = metric_items(run, metric, required).len();
        observed |= count > 0;
        // A replayed run (`lab check --at`) checks past text again with
        // today's checkers: an interval end, but not a run of the history
        // that `introduced` and the means describe.
        let live = !run.header.replayed;
        match place(&run.header.at).filter(|_| live) {
            Some(true) => {
                after.push(count as f64);
                first.get_or_insert(run);
                if reviewed.is_some_and(|r| run.name() == r) {
                    reviewed_run = Some(run);
                }
                last = Some(run);
            }
            Some(false) => before.push(count as f64),
            None => {}
        }
        let measures_all = || {
            required.iter().all(|name| {
                run.measures
                    .iter()
                    .any(|m| m.check == *name && m.measure.tool == metric.tool)
            })
        };
        if let (Some(commit), Some(false)) = (run.header.commit.as_deref(), run.header.dirty)
            && measures_all()
        {
            by_commit.insert(commit, index);
        }
    }
    e.before = describe(&before);
    e.after = describe(&after);
    if !observed {
        return e.unmeasurable(format!(
            "no complete check run has reported `{}:{}`",
            metric.tool, metric.rule
        ));
    }
    e.verdict = Verdict::Collecting;
    // A missing reviewed run (deleted, or not a complete run since adoption)
    // falls back to the adoption baseline, which shows more, not less.
    let mut note = String::new();
    if let (Some(first), Some(last)) = (first, last) {
        if let (Some(r), None) = (reviewed, reviewed_run) {
            note = format!(
                "; the reviewed run `{r}` is not a stored complete run since adoption, \
                 so findings count from the first one"
            );
        }
        let first = reviewed_run.unwrap_or(first);
        let mut found = introduced(
            &metric_items(first, metric, required),
            &metric_items(last, metric, required),
        );
        found.since = first.header.at.clone();
        found.since_date = date_of(&first.header.at).map(|d| d.to_string());
        found.latest = last.header.at.clone();
        found.latest_run = last.name();
        found.reviewed = reviewed_run.map(Run::name);
        e.introduced = Some(found);
    }

    let mut ends: Vec<usize> = by_commit.into_values().collect();
    ends.sort_by(|&a, &b| {
        let at = |i: usize| inputs.runs[i].header.at.as_str();
        at(a).cmp(at(b)).then(a.cmp(&b))
    });
    let points: Vec<(&Run, Measured)> = ends
        .into_iter()
        .map(|i| {
            let run = &inputs.runs[i];
            (run, measured(run, &metric.tool, required))
        })
        .collect();
    let min = inputs.settings.min_sessions;
    // Each interval: its side, its unit (the required checks' units, joined
    // by `+`), findings introduced and new units. An interval across a
    // change of a check's unit is left out, since its counts do not compare;
    // so is one across a change of a required checker's version, counted.
    let mut intervals: Vec<(bool, String, u64, u64)> = Vec::new();
    let mut version_changes = 0;
    let versions = known_versions(&points, required);
    for (i, pair) in points.windows(2).enumerate() {
        let ((r0, m0), (r1, m1)) = (&pair[0], &pair[1]);
        let Some(side) = place(&r1.header.at) else {
            continue;
        };
        let same_unit = m1
            .iter()
            .all(|(check, u)| u.unit.is_some() && m0[check].unit == u.unit);
        if !same_unit {
            continue;
        }
        let new_units = new_units(m0, m1);
        if new_units == 0 {
            continue;
        }
        let (v0, v1) = (&versions[i], &versions[i + 1]);
        if v1
            .iter()
            .any(|(check, v)| v0.get(check).is_some_and(|w| w != v))
        {
            version_changes += 1;
            continue;
        }
        let unit: BTreeSet<&str> = m1.values().filter_map(|u| u.unit).collect();
        let found = introduced_count(
            &metric_items(r0, metric, required),
            &metric_items(r1, metric, required),
        );
        let unit = unit.into_iter().collect::<Vec<_>>().join("+");
        intervals.push((side, unit, found, new_units));
    }
    if version_changes > 0 {
        let _ = write!(
            note,
            "; {version_changes} interval(s) across a checker version change left out"
        );
    }
    // Only the latest unit counts: a checker that renamed or changed its
    // unit gets a baseline again from runs in the new unit.
    let Some(unit) = intervals.last().map(|i| i.1.clone()) else {
        let why = if e.introduced.is_some() {
            "descriptive: no measured interval yet"
        } else {
            "no complete check run since adoption"
        };
        e.reason = Some(format!(
            "{why}; a verdict needs {min} or more intervals with new units on each side, \
             between clean runs (no uncommitted changes) on consecutive commits whose checks \
             print `rf.measure/1` (`--measure`){note}"
        ));
        return e;
    };
    let mut sides = [IntervalSide::default(); 2];
    let mut other_unit = 0;
    for (side, u, found, new_units) in &intervals {
        if *u != unit {
            other_unit += 1;
            continue;
        }
        let s = &mut sides[usize::from(*side)];
        s.n += 1;
        s.findings += found;
        s.new_units += new_units;
    }
    if other_unit > 0 {
        let _ = write!(
            note,
            "; {other_unit} interval(s) measured in another unit than `{unit}` left out"
        );
    }
    let [b, a] = sides;
    e.unit = Some(Unit::Interval);
    e.method = Some(Method::ConditionalBinomial);
    e.measure = Some(unit);
    e.before = b.side();
    e.after = a.side();
    if b.n < min {
        e.reason = Some(format!(
            "no baseline: {} of {min} measured intervals before adoption, {} of {min} after; \
             the rate after adoption has nothing to compare with yet (`lab check --at` can \
             replay commits before adoption){note}",
            b.n, a.n
        ));
        return e;
    }
    if a.n < min {
        e.reason = Some(format!(
            "{} of {min} measured intervals after adoption, {} of {min} before{note}",
            a.n, b.n
        ));
        return e;
    }
    let share = a.new_units as f64 / (a.new_units + b.new_units) as f64;
    let (lo, up) = stats::conditional_binomial(a.findings, a.findings + b.findings, share);
    e.p_lower = Some(lo);
    e.p_upper = Some(up);
    let half = inputs.settings.alpha / 2.0;
    e.verdict = if lo < half {
        Verdict::Improved
    } else if up < half {
        Verdict::Worse
    } else {
        Verdict::Inconclusive
    };
    // A verdict has no reason, but a missing reviewed run is still said.
    e.reason = note.strip_prefix("; ").map(Into::into);
    e
}

/// At each interval end, the last known version of each required check up
/// to it. An interval crosses a version change when a check has a known
/// version at both ends and they differ, so an unknown version, as in runs
/// stored before lab 0.9.0, neither forms a change nor hides one between the
/// known versions around it.
fn known_versions<'r>(
    points: &[(&'r Run, Measured)],
    required: &BTreeSet<&str>,
) -> Vec<BTreeMap<&'r str, &'r str>> {
    let mut known: BTreeMap<&'r str, &'r str> = BTreeMap::new();
    points
        .iter()
        .map(|(run, _)| {
            for c in run
                .header
                .checks
                .iter()
                .filter(|c| required.contains(c.name.as_str()))
            {
                if let Some(v) = &c.version {
                    known.insert(c.name.as_str(), v.as_str());
                }
            }
            known.clone()
        })
        .collect()
}

/// The units one check measured for a tool in one run, merged over its
/// `rf.measure/1` records.
#[derive(Debug)]
struct Units<'r> {
    /// `None` when the records name more than one unit.
    unit: Option<&'r str>,
    count: u64,
    /// `None` when a record has no keys.
    keys: Option<BTreeSet<&'r str>>,
}

/// Per required check, its units.
type Measured<'r> = BTreeMap<&'r str, Units<'r>>;

/// What each required check measured for `tool` in `run`. A check that
/// measured nothing is missing.
fn measured<'r>(run: &'r Run, tool: &str, required: &BTreeSet<&str>) -> Measured<'r> {
    let mut out: Measured = BTreeMap::new();
    for m in run
        .measures
        .iter()
        .filter(|m| m.measure.tool == tool && required.contains(m.check.as_str()))
    {
        let unit = m.measure.unit.as_str();
        let u = out.entry(m.check.as_str()).or_insert(Units {
            unit: Some(unit),
            count: 0,
            keys: Some(BTreeSet::new()),
        });
        if u.unit != Some(unit) {
            u.unit = None;
        }
        u.count += m.measure.count;
        match (&mut u.keys, &m.measure.keys) {
            (Some(keys), Some(more)) => keys.extend(more.iter().map(String::as_str)),
            (keys, None) => *keys = None,
            (None, Some(_)) => {}
        }
    }
    out
}

/// New units from `earlier` to `later`: per check, the keys `later` has and
/// `earlier` lacks, or without keys on both sides the count's increase.
fn new_units(earlier: &Measured, later: &Measured) -> u64 {
    later
        .iter()
        .map(|(check, l)| {
            let e = &earlier[check];
            match (&e.keys, &l.keys) {
                (Some(ek), Some(lk)) => lk.difference(ek).count() as u64,
                _ => l.count.saturating_sub(e.count),
            }
        })
        .sum()
}

/// Intervals on one side of adoption.
#[derive(Debug, Clone, Copy, Default)]
struct IntervalSide {
    n: usize,
    findings: u64,
    new_units: u64,
}

impl IntervalSide {
    fn side(self) -> Side {
        Side {
            n: self.n,
            rate: (self.new_units > 0).then(|| self.findings as f64 / self.new_units as f64),
            findings: Some(self.findings),
            new_units: Some(self.new_units),
            ..Side::default()
        }
    }
}

/// A run's findings of the metric's rule from the required checks.
fn metric_items<'r>(run: &'r Run, metric: &Metric, required: &BTreeSet<&str>) -> Vec<&'r Item> {
    run.items
        .iter()
        .filter(|i| {
            required.contains(i.check.as_str())
                && i.finding.tool == metric.tool
                && i.finding.rule == metric.rule
        })
        .collect()
}

/// Subjects kept per introduced key.
const INTRODUCED_SUBJECTS: usize = 10;

/// What introduced findings are counted by: message and data.
fn finding_key(i: &Item) -> (String, String) {
    (
        i.finding.message.clone(),
        i.finding
            .data
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default(),
    )
}

/// `introduced(first, last).count`, without the subjects.
fn introduced_count(first: &[&Item], last: &[&Item]) -> u64 {
    let mut net: BTreeMap<(String, String), i64> = BTreeMap::new();
    for i in first {
        *net.entry(finding_key(i)).or_default() -= 1;
    }
    for i in last {
        *net.entry(finding_key(i)).or_default() += 1;
    }
    net.values().map(|&n| n.max(0) as u64).sum()
}

/// The findings of `last` beyond their key's count in `first`, keyed by
/// message and data.
fn introduced(first: &[&Item], last: &[&Item]) -> Introduced {
    let key = finding_key;
    let mut base: BTreeMap<(String, String), usize> = BTreeMap::new();
    for i in first {
        *base.entry(key(i)).or_default() += 1;
    }
    let old: BTreeSet<&str> = first.iter().map(|i| i.fingerprint.as_str()).collect();
    let mut now: BTreeMap<(String, String), Vec<&Item>> = BTreeMap::new();
    for i in last {
        now.entry(key(i)).or_default().push(i);
    }
    let mut findings: Vec<IntroducedFinding> = now
        .into_iter()
        .filter_map(|(k, items)| {
            let added = items
                .len()
                .saturating_sub(base.get(&k).copied().unwrap_or(0));
            if added == 0 {
                return None;
            }
            // Where the new ones are: subjects with a fingerprint the first
            // run lacks (a moved copy also has one), else every subject.
            let new: Vec<&Item> = items
                .iter()
                .copied()
                .filter(|i| !old.contains(i.fingerprint.as_str()))
                .collect();
            let shown = if new.is_empty() { &items } else { &new };
            Some(IntroducedFinding {
                message: k.0,
                added,
                subjects: shown
                    .iter()
                    .take(INTRODUCED_SUBJECTS)
                    .map(|i| i.finding.subject.clone())
                    .collect(),
                more: shown.len().saturating_sub(INTRODUCED_SUBJECTS),
            })
        })
        .collect();
    findings.sort_by(|a, b| b.added.cmp(&a.added).then(a.message.cmp(&b.message)));
    Introduced {
        since: String::new(),
        since_date: None,
        latest: String::new(),
        latest_run: String::new(),
        reviewed: None,
        count: findings.iter().map(|f| f.added).sum(),
        findings,
    }
}

fn describe(values: &[f64]) -> Side {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    Side {
        n,
        sessions: None,
        positives: None,
        rate: None,
        findings: None,
        new_units: None,
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
        sessions: None,
        positives: Some(positives),
        rate: (!values.is_empty()).then(|| positives as f64 / values.len() as f64),
        mean: None,
        median: None,
        findings: None,
        new_units: None,
    }
}

/// A session group within a stratum: the subagents below one root session
/// (`Tree`, the root's id), or a root session on its own (`Own`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Group<'a> {
    Tree(&'a str),
    Own(&'a str),
}

/// One session group: its side of adoption and its sessions' values.
type Members = (bool, Vec<f64>);

/// A stratum's session groups.
type Groups<'a> = BTreeMap<Group<'a>, Members>;

/// Rounds of the cluster permutation test; its smallest p-value is
/// `1 / (ROUNDS + 1)`, far below any `alpha / 2` in use.
const ROUNDS: u32 = 20_000;

/// Fixed so that `lab evaluate` gives the same p-values on the same data.
const SEED: u64 = 0x5EED_1E55_0000_0001;

/// Per-session evaluation in session groups: a finding rule as a 0/1
/// indicator per session (`field` is `None`), or a numeric field of the
/// session record.
///
/// The subagents below one root session share its task, so they form one
/// group, on the side of adoption where the root started; any other session
/// is a group of its own. Groups are the units counted against
/// `min_sessions`. When every comparable group is a single session the exact
/// tests apply; otherwise the cluster permutation test moves whole groups.
fn sessions(
    mut e: Evaluation,
    metric: &Metric,
    field: Option<&str>,
    inputs: &Inputs,
    place: impl Fn(&str) -> Option<bool>,
) -> Evaluation {
    e.unit = Some(Unit::SessionGroup);
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
    let by_id = retro::by_id(sessions);
    let root = |s| retro::root_of(&by_id, s);
    // A tree's start: its root's, or its earliest member's when the file
    // does not have the root.
    let mut tree_start: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    for s in sessions {
        if let (Some(r), Some(start)) = (root(s), s.start.as_deref()) {
            let key = (s.source.as_str(), r);
            let known = by_id.get(&key).and_then(|r| r.start.as_deref());
            let entry = tree_start.entry(key).or_insert(known.unwrap_or(start));
            if known.is_none() && start < *entry {
                *entry = start;
            }
        }
    }
    // (source, role) → group → (side, its sessions' values)
    let mut strata: BTreeMap<(&str, &str), Groups> = BTreeMap::new();
    let mut dated = 0;
    for s in sessions {
        let (group, start) = match root(s) {
            Some(r) => (
                Group::Tree(r),
                tree_start.get(&(s.source.as_str(), r)).copied(),
            ),
            None => (Group::Own(s.id.as_str()), s.start.as_deref()),
        };
        let Some(after) = start.and_then(&place) else {
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
        strata
            .entry((s.source.as_str(), s.role()))
            .or_default()
            .entry(group)
            .or_insert((after, Vec::new()))
            .1
            .push(value);
    }
    let count = |groups: &Groups, after: bool| groups.values().filter(|(a, _)| *a == after).count();
    let contributing: Vec<(&(&str, &str), &Groups)> = strata
        .iter()
        .filter(|(_, g)| count(g, false) > 0 && count(g, true) > 0)
        .collect();
    // Only strata with groups on both sides are compared; when none has,
    // the lesson is still collecting.
    let groups_in = |after: bool| -> usize { strata.values().map(|g| count(g, after)).sum() };
    let (groups_before, groups_after) = (groups_in(false), groups_in(true));
    e.excluded =
        groups_before + groups_after - contributing.iter().map(|(_, g)| g.len()).sum::<usize>();
    e.strata = contributing
        .iter()
        .map(|((source, role), g)| Stratum {
            source: (*source).into(),
            role: (*role).into(),
            before: count(g, false),
            after: count(g, true),
        })
        .collect();
    // Every session's value on one side of the contributing strata.
    let values = |after: bool| -> Vec<f64> {
        contributing
            .iter()
            .flat_map(|(_, g)| g.values())
            .filter(|(a, _)| *a == after)
            .flat_map(|(_, v)| v.iter().copied())
            .collect()
    };
    let (before, after) = (values(false), values(true));
    let side = |v: &[f64], after: bool| -> Side {
        let s = match field {
            None => indicator_side(v),
            Some(_) => describe(v),
        };
        Side {
            n: contributing.iter().map(|(_, g)| count(g, after)).sum(),
            sessions: Some(v.len()),
            ..s
        }
    };
    e.before = side(&before, false);
    e.after = side(&after, true);
    let clustered = contributing
        .iter()
        .any(|(_, g)| g.values().any(|(_, v)| v.len() > 1));
    e.method = Some(match (clustered, field) {
        (true, _) => Method::ClusterPermutation,
        (false, None) => Method::StratifiedHypergeometric,
        (false, Some(_)) => Method::VanElteren,
    });
    let min = inputs.settings.min_sessions;
    let half = inputs.settings.alpha / 2.0;
    let any_value = strata
        .values()
        .flat_map(|g| g.values())
        .any(|(_, v)| !v.is_empty());

    if let Some(path) = field
        && !any_value
    {
        return e.unmeasurable(if dated == 0 {
            "no comparable baseline sessions".to_string()
        } else {
            format!(
                "metric never observed in comparable sessions: no session has a numeric `{path}`"
            )
        });
    }
    if groups_before == 0 {
        return e.unmeasurable("no comparable baseline sessions");
    }
    if groups_before < min {
        return e.unmeasurable(format!(
            "no comparable baseline: only {groups_before} session group(s) before adoption, fewer than {min}"
        ));
    }
    let positive = |v: &[f64]| v.iter().any(|&x| x > 0.0);
    if field.is_none()
        && !strata
            .values()
            .flat_map(|g| g.values())
            .any(|(_, v)| positive(v))
    {
        return e.unmeasurable("metric never observed in comparable sessions");
    }
    // Unstratified, over every dated group: the finding may so far occur
    // only in strata that have no group after adoption yet. A group without
    // the finding in any session is a clean group.
    let positive_groups = |after: bool| -> u64 {
        strata
            .values()
            .flat_map(|g| g.values())
            .filter(|(a, v)| *a == after && positive(v))
            .count() as u64
    };
    let pooled = stats::Counts {
        before: groups_before as u64,
        before_positive: positive_groups(false),
        after: groups_after as u64,
        after_positive: positive_groups(true),
    };
    // Rule metrics: how many more clean groups would show a decrease, or
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
    if e.before.n < min || e.after.n < min {
        e.verdict = Verdict::Collecting;
        e.reason = Some(if contributing.is_empty() {
            "no (source, role) stratum has sessions both before and after adoption yet".into()
        } else {
            format!(
                "{} of {min} session groups after adoption, {} of {min} before, in comparable strata",
                e.after.n, e.before.n
            )
        });
        return needed(e);
    }
    let tied = "the field has the same value in every comparable session";
    let (lo, up) = if clustered {
        // Scores: the indicator, or a field's rank within its stratum
        // scaled by `1/(N+1)` as in the van Elteren test.
        let mut clusters = Vec::new();
        let mut varies = field.is_none();
        for (_, g) in &contributing {
            let all: Vec<f64> = g.values().flat_map(|(_, v)| v.iter().copied()).collect();
            let scores = match field {
                None => all.clone(),
                Some(_) => {
                    varies |= all.iter().any(|&v| v != all[0]);
                    let n = all.len() as f64;
                    stats::midranks(&all)
                        .into_iter()
                        .map(|r| r / (n + 1.0))
                        .collect()
                }
            };
            let mut next = scores.into_iter();
            clusters.push(
                g.values()
                    .map(|(after, v)| stats::Cluster {
                        after: *after,
                        sum: next.by_ref().take(v.len()).sum(),
                        count: v.len(),
                    })
                    .collect::<Vec<_>>(),
            );
        }
        if !varies {
            return e.unmeasurable(tied);
        }
        stats::cluster_permutation(&clusters, ROUNDS, SEED)
    } else {
        let sides = |g: &Groups, after: bool| -> Vec<f64> {
            g.values()
                .filter(|(a, _)| *a == after)
                .flat_map(|(_, v)| v.iter().copied())
                .collect()
        };
        match field {
            None => stats::stratified_hypergeometric(
                &contributing
                    .iter()
                    .map(|(_, g)| {
                        let (b, a) = (sides(g, false), sides(g, true));
                        let positives = |v: &[f64]| v.iter().filter(|&&x| x > 0.0).count() as u64;
                        stats::Counts {
                            before: b.len() as u64,
                            before_positive: positives(&b),
                            after: a.len() as u64,
                            after_positive: positives(&a),
                        }
                    })
                    .collect::<Vec<_>>(),
            ),
            Some(_) => {
                let samples: Vec<stats::Samples> = contributing
                    .iter()
                    .map(|(_, g)| stats::Samples {
                        before: sides(g, false),
                        after: sides(g, true),
                    })
                    .collect();
                match stats::van_elteren(&samples) {
                    Some((_, lo, up)) => (lo, up),
                    None => return e.unmeasurable(tied),
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
    use crate::check::{
        CheckOutcome, ITEM_SCHEMA, Item, MEASURE_ITEM_SCHEMA, Measure, MeasureItem, RUN_SCHEMA,
        RunHeader,
    };
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
                reviewed: None,
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
            json.contains(r#""schema":"rf.evaluation/2""#)
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
                .starts_with("12 of 20 session groups after"),
            "{e:?}"
        );
        assert_eq!(e.sessions_needed, Some(433));
        assert!(
            e.line().contains("about 433 more clean session group(s)"),
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

    /// `n` subagents of `parent` on `date`, the first `positive` of them flagged.
    fn children(
        sessions: &mut Vec<Session>,
        findings: &mut Vec<Finding>,
        parent: &str,
        date: &str,
        n: usize,
        positive: usize,
    ) {
        for i in 0..n {
            let id = format!("{parent}-{date}-{i}");
            let extra = format!(r#","parent_id":"{parent}""#);
            sessions.push(session(
                "claude",
                "subagent",
                &id,
                &format!("{date}T12:00:00Z"),
                &extra,
            ));
            if i < positive {
                findings.push(agent_finding(&format!("claude:{id}")));
            }
        }
    }

    /// The same sessions with no parent: each one its own group, as before
    /// session groups.
    fn ungrouped(s: &[Session]) -> Vec<Session> {
        s.iter()
            .map(|x| {
                let mut x = x.clone();
                x.parent_id = None;
                x
            })
            .collect()
    }

    #[test]
    fn one_parents_subagents_count_as_one_session_group() {
        // The masters case: every finding after adoption comes from one
        // parent's subagents. 500 clean subagents of other parents form the
        // baseline, and 6 of 12 children of one parent are positive after.
        let (mut s, mut f) = (Vec::new(), Vec::new());
        for i in 0..500 {
            children(&mut s, &mut f, &format!("p{i}"), "2026-10-01", 1, 0);
        }
        children(&mut s, &mut f, "cluster", "2026-11-03", 12, 6);
        let l = lesson("agentprof:file-reread", "2026-11-01");
        let e = run_eval(&l, &s, &f, &EvaluateConfig::default());
        assert_eq!(e.unit, Some(Unit::SessionGroup));
        assert_eq!((e.before.n, e.before.sessions), (500, Some(500)));
        assert_eq!((e.after.n, e.after.sessions), (1, Some(12)));
        assert_eq!((e.after.positives, e.after.rate), (Some(6), Some(0.5)));
        assert_eq!(e.strata[0].after, 1);
        // One group after adoption is too few to call anything; with a clean
        // baseline no decrease can be shown either.
        assert_eq!(e.verdict, Verdict::Unmeasurable, "{e:?}");
        let line = e.line();
        assert!(
            line.contains(
                "0.0% → 50.0% of sessions (n 500 → 1 session groups from 500 → 12 sessions"
            ),
            "{line}"
        );
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""schema":"rf.evaluation/2""#)
                && json.contains(r#""unit":"session-group""#)
                && json.contains(r#""sessions":12"#),
            "{json}"
        );
        // Counted per session, as before session groups, the burst is `worse`.
        let e = run_eval(&l, &ungrouped(&s), &f, &EvaluateConfig::default());
        assert_eq!((e.after.n, e.after.positives), (12, Some(6)));
        assert_eq!(e.verdict, Verdict::Worse, "{e:?}");
    }

    #[test]
    fn a_burst_in_one_group_is_tested_by_moving_whole_groups() {
        let (mut s, mut f) = (Vec::new(), Vec::new());
        for i in 0..100 {
            children(
                &mut s,
                &mut f,
                &format!("p{i}"),
                "2026-10-01",
                1,
                usize::from(i == 0),
            );
        }
        for i in 0..9 {
            children(&mut s, &mut f, &format!("q{i}"), "2026-11-02", 1, 0);
        }
        children(&mut s, &mut f, "burst", "2026-11-03", 12, 6);
        let l = lesson("agentprof:file-reread", "2026-11-01");
        let e = run_eval(&l, &s, &f, &EvaluateConfig::default());
        assert_eq!(e.method, Some(Method::ClusterPermutation));
        assert_eq!((e.after.n, e.after.sessions), (10, Some(21)));
        // The burst lands after adoption in about 10 of 110 random splits.
        let up = e.p_upper.unwrap();
        assert!((0.06..0.13).contains(&up), "{e:?}");
        assert_eq!(e.verdict, Verdict::Inconclusive, "{e:?}");
        // The same data give the same p-values.
        assert_eq!(
            run_eval(&l, &s, &f, &EvaluateConfig::default()).p_upper,
            Some(up)
        );
        let e = run_eval(&l, &ungrouped(&s), &f, &EvaluateConfig::default());
        assert_eq!(e.verdict, Verdict::Worse, "{e:?}");
    }

    /// Adds one side's sessions and findings on a date.
    type Fill = fn(&mut Vec<Session>, &mut Vec<Finding>, &str);

    #[test]
    fn a_different_mix_of_group_sizes_alone_is_not_a_change() {
        // 5% of sessions have the finding on both sides; only the group
        // sizes differ (review of research-factory-45).
        let l = lesson("agentprof:file-reread", "2026-11-01");
        let singles: Fill = |s, f, date| {
            for i in 0..300 {
                children(
                    s,
                    f,
                    &format!("s{date}{i}"),
                    date,
                    1,
                    usize::from(i % 20 == 0),
                );
            }
        };
        let bursts: Fill = |s, f, date| {
            for i in 0..20 {
                children(
                    s,
                    f,
                    &format!("b{date}{i}"),
                    date,
                    10,
                    usize::from(i % 2 == 0),
                );
            }
        };
        for (first, second) in [(singles, bursts), (bursts, singles)] {
            let (mut s, mut f) = (Vec::new(), Vec::new());
            first(&mut s, &mut f, "2026-10-01");
            second(&mut s, &mut f, "2026-11-02");
            let e = run_eval(&l, &s, &f, &EvaluateConfig::default());
            assert_eq!(e.method, Some(Method::ClusterPermutation));
            assert_eq!(e.before.rate, e.after.rate, "{e:?}");
            assert_eq!(e.verdict, Verdict::Inconclusive, "{e:?}");
        }

        // A skewed field with the same distribution per session on both
        // sides: nine 1s and one 100 in every ten sessions.
        let value = |i: usize| if i % 10 == 9 { 100.0 } else { 1.0 };
        let mut s = Vec::new();
        for i in 0..100 {
            s.push(session(
                "claude",
                "subagent",
                &format!("b{i}"),
                "2026-10-01T00:00:00Z",
                &format!(r#","total":{{"tokens":{{"output":{}}}}}"#, value(i)),
            ));
        }
        for g in 0..12 {
            for i in 0..10 {
                s.push(session(
                    "claude",
                    "subagent",
                    &format!("a{g}-{i}"),
                    "2026-11-02T00:00:00Z",
                    &format!(
                        r#","total":{{"tokens":{{"output":{}}}}},"parent_id":"g{g}""#,
                        value(i)
                    ),
                ));
            }
        }
        let l = lesson("agentprof:session.total.tokens.output", "2026-11-01");
        let e = run_eval(&l, &s, &[], &EvaluateConfig::default());
        assert_eq!(e.method, Some(Method::ClusterPermutation));
        assert_eq!((e.after.n, e.after.sessions), (12, Some(120)));
        assert_eq!(e.verdict, Verdict::Inconclusive, "{e:?}");
    }

    #[test]
    fn a_tree_is_one_group_on_its_roots_side() {
        let (mut s, mut f) = (Vec::new(), Vec::new());
        // Root main session M started before adoption; its subagent A, and
        // A's five subagents, started after it. All six are one group, before.
        s.push(session("claude", "main", "M", "2026-10-31T23:00:00Z", ""));
        children(&mut s, &mut f, "M", "2026-11-02", 1, 1);
        children(&mut s, &mut f, "M-2026-11-02-0", "2026-11-02", 5, 5);
        // Other subagents, whose parents the file does not have.
        for i in 0..3 {
            children(&mut s, &mut f, &format!("q{i}"), "2026-10-20", 1, 0);
            children(&mut s, &mut f, &format!("r{i}"), "2026-11-03", 2, 0);
        }
        let l = lesson("agentprof:file-reread", "2026-11-01");
        let cfg = EvaluateConfig {
            min_sessions: 1,
            ..EvaluateConfig::default()
        };
        let e = run_eval(&l, &s, &f, &cfg);
        let sub = e.strata.iter().find(|x| x.role == "subagent").unwrap();
        assert_eq!((sub.before, sub.after), (4, 3), "{e:?}");
        assert_eq!((e.before.sessions, e.before.positives), (Some(9), Some(6)));
        assert_eq!((e.after.sessions, e.after.positives), (Some(6), Some(0)));
        assert_eq!(
            e.excluded, 1,
            "M's main stratum has no group after adoption"
        );
    }

    #[test]
    fn a_root_is_a_group_apart_from_its_subagents_in_one_stratum() {
        // Without roles, a parent and its children share the `unknown` stratum.
        let line = |id: &str, parent: Option<&str>| {
            let p = parent
                .map(|p| format!(r#","parent_id":"{p}""#))
                .unwrap_or_default();
            Session::parse(&format!(
                r#"{{"schema":"rf.session/1","source":"codex","id":"{id}","start":"2026-11-02T00:00:00Z"{p}}}"#
            ))
            .unwrap()
        };
        let mut s = vec![
            line("P", None),
            line("c1", Some("P")),
            line("c2", Some("P")),
        ];
        s.push(line("x", None));
        s[0].start = Some("2026-10-02T00:00:00Z".into());
        s[3].start = Some("2026-10-03T00:00:00Z".into());
        let f = vec![agent_finding("codex:c1")];
        let l = lesson("agentprof:file-reread", "2026-11-01");
        let cfg = EvaluateConfig {
            min_sessions: 1,
            ..EvaluateConfig::default()
        };
        let e = run_eval(&l, &s, &f, &cfg);
        // P and x before; P's subagents c1 and c2 form one group, on P's
        // side: three groups, none after adoption.
        assert!(e.strata.is_empty(), "{e:?}");
        assert_eq!(e.excluded, 3, "{e:?}");
    }

    #[test]
    fn version_1_records_still_parse() {
        // A `last.json` written by lab 0.3.
        let v1 = r#"{"schema":"rf.evaluation/1","lesson":"read-by-section","lesson_status":"adopted","metric":"agentprof:file-reread","unit":"session","method":"stratified-hypergeometric","verdict":"collecting","reason":"3 of 10 sessions after adoption, 540 of 10 before, in comparable strata","adopted":"2026-11-07","before":{"n":540,"positives":5,"rate":0.009259259259259259,"mean":null,"median":null},"after":{"n":3,"positives":0,"rate":0.0,"mean":null,"median":null},"p_lower":null,"p_upper":null,"strata":[{"source":"claude","role":"main","before":540,"after":3}],"excluded":12,"sessions_needed":295,"min_sessions":10,"alpha":0.05,"baseline_days":90}"#;
        let e: Evaluation = serde_json::from_str(v1).unwrap();
        assert_eq!((e.unit, e.before.sessions), (Some(Unit::Session), None));
    }

    #[test]
    fn cluster_permutation_matches_the_exact_test_without_clusters() {
        // Singletons in one stratum: the permutation distribution is the
        // hypergeometric one, up to Monte Carlo error.
        let cluster = |after, positive| stats::Cluster {
            after,
            sum: f64::from(u8::from(positive)),
            count: 1,
        };
        let mut c: Vec<_> = (0..40).map(|i| cluster(false, i < 2)).collect();
        c.extend((0..30).map(|i| cluster(true, i < 6)));
        let (lo, up) = stats::cluster_permutation(&[c], ROUNDS, SEED);
        let (elo, eup) = stats::stratified_hypergeometric(&[stats::Counts {
            before: 40,
            before_positive: 2,
            after: 30,
            after_positive: 6,
        }]);
        assert!(
            (lo - elo).abs() < 0.01 && (up - eup).abs() < 0.01,
            "{lo} {up} vs {elo} {eup}"
        );
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
                version: None,
                uncommitted_inputs: Vec::new(),
            });
        }
        Run {
            path: "x".into(),
            header: RunHeader {
                schema: RUN_SCHEMA.into(),
                at: at.into(),
                commit: None,
                dirty: None,
                replayed: false,
                checks: outcomes,
            },
            items,
            measures: Vec::new(),
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
        let reason = e.reason.unwrap();
        assert!(
            reason.starts_with("no complete check run since adoption; a verdict needs 2"),
            "{reason}"
        );
        assert_eq!(e.introduced, None);
        // Once `thesis` is no longer configured, it is not required.
        let e = eval(&["docs"]);
        assert_eq!((e.before.n, e.after.n), (2, 2));
        assert_eq!((e.before.mean, e.after.mean), (Some(2.0), Some(0.0)));
        // Fewer findings per run is not a verdict: runs repeat the backlog.
        assert_eq!(e.verdict, Verdict::Collecting);
        assert_eq!(e.introduced.as_ref().map(|i| i.count), Some(0));
    }

    /// A complete `claims` run with source-text-missing findings at
    /// (subject, message, sentence); the sentence goes in the data, as
    /// claimcheck's does.
    fn run_with(at: &str, findings: &[(&str, &str, &str)]) -> Run {
        let findings: Vec<Finding> = findings
            .iter()
            .map(|&(subject, message, sentence)| {
                let mut f = Finding::lab(
                    "source-text-missing",
                    Severity::Warn,
                    subject,
                    message.into(),
                );
                f.tool = "claimcheck".into();
                f.data = Some(serde_json::json!({ "sentence": sentence }));
                f
            })
            .collect();
        let fps = finding::fingerprints(&findings);
        let mut run = run("x", 0, true);
        run.header.at = at.into();
        run.path = format!("{at}.jsonl").into();
        run.items = findings
            .into_iter()
            .zip(fps)
            .map(|(finding, fingerprint)| Item {
                schema: ITEM_SCHEMA.into(),
                check: "claims".into(),
                fingerprint,
                finding,
            })
            .collect();
        run
    }

    fn eval_runs(runs: &[Run]) -> Evaluation {
        eval_reviewed(runs, None)
    }

    fn eval_reviewed(runs: &[Run], reviewed: Option<&str>) -> Evaluation {
        let cfg = EvaluateConfig {
            min_sessions: 2,
            ..EvaluateConfig::default()
        };
        let mut l = lesson("claimcheck:source-text-missing", "2026-11-03");
        l.meta.reviewed = reviewed.map(Into::into);
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
    }

    const OLD: &str = "references 1, 484 have no source text";
    const NEW: &str = "reference 786 has no source text";

    #[test]
    fn a_review_moves_the_introduced_count_to_the_reviewed_run() {
        let runs = vec![
            run_with("2026-11-03T00:00:00Z", &[("a.md:3", OLD, "s3")]),
            run_with(
                "2026-11-04T00:00:00Z",
                &[("a.md:3", OLD, "s3"), ("log.md:9", NEW, "n9")],
            ),
            run_with(
                "2026-11-05T00:00:00Z",
                &[
                    ("a.md:3", OLD, "s3"),
                    ("log.md:9", NEW, "n9"),
                    ("b.md:1", OLD, "s1"),
                ],
            ),
        ];
        // Without a review both later findings count.
        assert_eq!(eval_runs(&runs).introduced.unwrap().count, 2);
        // Reviewed through the run of 4 November: only b.md:1 is new.
        let e = eval_reviewed(&runs, Some("2026-11-04T00:00:00Z.jsonl"));
        let i = e.introduced.clone().unwrap();
        assert_eq!(
            (i.since.as_str(), i.reviewed.as_deref(), i.count),
            (
                "2026-11-04T00:00:00Z",
                Some("2026-11-04T00:00:00Z.jsonl"),
                1
            )
        );
        assert_eq!(i.latest_run, "2026-11-05T00:00:00Z.jsonl");
        assert_eq!(e.introduced_lines(), vec![format!("  +1 {OLD} [b.md:1]")]);
        let line = e.line();
        assert!(
            line.contains("1 introduced since the reviewed run of 2026-11-04)"),
            "{line}"
        );
        let back: Evaluation = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
        // Reviewed through the latest run: nothing introduced.
        let i = eval_reviewed(&runs, Some("2026-11-05T00:00:00Z.jsonl"))
            .introduced
            .unwrap();
        assert_eq!((i.since.as_str(), i.count), ("2026-11-05T00:00:00Z", 0));
        // Runs that share a second are told apart by their file names: a run
        // stored after the reviewed one still counts.
        let at = "2026-11-05T00:00:00Z";
        let mut tied = [
            run_with(at, &[("a.md:3", OLD, "s3")]),
            run_with(at, &[("a.md:3", OLD, "s3"), ("log.md:9", NEW, "n9")]),
        ];
        tied[1].path = format!("{at}-1.jsonl").into();
        let i = eval_reviewed(&tied, Some("2026-11-05T00:00:00Z.jsonl"))
            .introduced
            .unwrap();
        assert_eq!((i.reviewed.is_some(), i.count), (true, 1));
        // A reviewed run that is no longer stored falls back to adoption.
        let e = eval_reviewed(&runs, Some("gone.jsonl"));
        let i = e.introduced.clone().unwrap();
        assert_eq!(
            (i.since.as_str(), i.reviewed, i.count),
            ("2026-11-03T00:00:00Z", None, 2)
        );
        assert!(
            e.reason.unwrap().contains(
                "the reviewed run `gone.jsonl` is not a stored complete run since adoption"
            )
        );
    }

    #[test]
    fn check_runs_count_findings_introduced_since_adoption_by_message_and_data() {
        let runs = vec![
            // Before adoption: no part in the count.
            run_with("2026-11-01T00:00:00Z", &[("a.md:1", NEW, "n")]),
            // The first run since adoption holds the earlier violations.
            run_with(
                "2026-11-03T00:00:00Z",
                &[("a.md:3", OLD, "s3"), ("a.md:9", OLD, "s9")],
            ),
            // A finding added and fixed between runs is not in the latest run.
            run_with(
                "2026-11-04T00:00:00Z",
                &[
                    ("a.md:3", OLD, "s3"),
                    ("a.md:9", OLD, "s9"),
                    ("c.md:1", "gone", "g"),
                ],
            ),
            // Both OLD sentences moved to a new file; NEW appears in two
            // sentences; OLD in a new sentence.
            run_with(
                "2026-11-05T00:00:00Z",
                &[
                    ("moved/a.md:3", OLD, "s3"),
                    ("moved/a.md:9", OLD, "s9"),
                    ("b.md:2", OLD, "s2"),
                    ("b.md:5", NEW, "n5"),
                    ("d.md:1", NEW, "n1"),
                ],
            ),
        ];
        let e = eval_runs(&runs);
        assert_eq!(
            (e.verdict, e.method, e.unit),
            (
                Verdict::Collecting,
                Some(Method::Descriptive),
                Some(Unit::Run)
            )
        );
        assert_eq!((e.before.n, e.after.n), (1, 3));
        let i = e.introduced.clone().unwrap();
        assert_eq!(
            (
                i.since.as_str(),
                i.since_date.as_deref(),
                i.latest.as_str(),
                i.count
            ),
            (
                "2026-11-03T00:00:00Z",
                Some("2026-11-03"),
                "2026-11-05T00:00:00Z",
                3
            )
        );
        let subjects: Vec<(&str, usize, Vec<&str>)> = i
            .findings
            .iter()
            .map(|f| {
                (
                    f.message.as_str(),
                    f.added,
                    f.subjects.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            subjects,
            [
                (NEW, 1, vec!["d.md:1"]),
                (NEW, 1, vec!["b.md:5"]),
                (OLD, 1, vec!["b.md:2"]),
            ]
        );
        let line = e.line();
        assert!(
            line.contains(
                "3 introduced since the first complete run after adoption, on 2026-11-03)"
            ),
            "{line}"
        );
        assert!(
            line.contains("collecting (descriptive: no measured interval yet; a verdict needs 2"),
            "{line}"
        );
        assert_eq!(
            e.introduced_lines(),
            vec![
                format!("  +1 {NEW} [d.md:1]"),
                format!("  +1 {NEW} [b.md:5]"),
                format!("  +1 {OLD} [b.md:2]"),
            ]
        );
        let json = serde_json::to_string(&e).unwrap();
        let back: Evaluation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn a_moved_finding_adds_nothing_but_a_fix_plus_a_new_one_counts() {
        let first = run_with("2026-11-03T00:00:00Z", &[("a.md:1", OLD, "s1")]);
        // The same sentence moved to a new file: a new fingerprint, the same key.
        let moved = run_with("2026-11-04T00:00:00Z", &[("new/a.md:1", OLD, "s1")]);
        assert_ne!(first.items[0].fingerprint, moved.items[0].fingerprint);
        let e = eval_runs(&[first, moved]);
        assert_eq!(e.introduced.as_ref().unwrap().count, 0);
        assert!(e.introduced_lines().is_empty());
        assert!(e.line().contains("0 introduced since"), "{}", e.line());
        // One violation fixed and another with the same message written:
        // the net count is unchanged, but the new sentence is introduced.
        let e = eval_runs(&[
            run_with("2026-11-03T00:00:00Z", &[("a.md:1", OLD, "s1")]),
            run_with("2026-11-04T00:00:00Z", &[("b.md:7", OLD, "s7")]),
        ]);
        assert_eq!(e.introduced_lines(), vec![format!("  +1 {OLD} [b.md:7]")]);
    }

    #[test]
    fn introduced_subjects_name_the_new_findings_and_are_capped() {
        let mut first = Vec::new();
        let mut last = Vec::new();
        let subjects: Vec<String> = (0..30).map(|i| format!("doc.md:{i}")).collect();
        for (i, s) in subjects.iter().enumerate() {
            // One sentence repeated: the same key 30 times in the backlog.
            if i < 7 {
                first.push((s.as_str(), OLD, "same"));
            }
            last.push((s.as_str(), OLD, "same"));
        }
        last.push(("new.md:99", OLD, "same"));
        let e = eval_runs(&[
            run_with("2026-11-03T00:00:00Z", &first),
            run_with("2026-11-04T00:00:00Z", &last),
        ]);
        let f = &e.introduced.as_ref().unwrap().findings[0];
        assert_eq!(f.added, 24);
        // The 23 duplicates in doc.md and new.md all have new fingerprints;
        // ten are kept and the rest counted.
        assert_eq!((f.subjects.len(), f.more), (10, 14));
        // The new file is named even behind a long backlog of one message.
        let e = eval_runs(&[
            run_with("2026-11-03T00:00:00Z", &last[..30]),
            run_with("2026-11-04T00:00:00Z", &last),
        ]);
        assert_eq!(
            e.introduced_lines(),
            vec![format!("  +1 {OLD} [new.md:99]")]
        );
    }

    #[test]
    fn a_rule_no_check_run_reported_is_unmeasurable_and_a_late_first_run_is_named() {
        let e = eval_runs(&[
            run("2026-11-01T00:00:00Z", 2, true), // quote-not-found, another rule
            run("2026-11-04T00:00:00Z", 1, true),
        ]);
        assert_eq!(e.verdict, Verdict::Unmeasurable);
        assert_eq!(
            e.reason.as_deref(),
            Some("no complete check run has reported `claimcheck:source-text-missing`")
        );
        // Adopted 2026-11-03; the first run after it is a week later.
        let e = eval_runs(&[
            run_with("2026-11-10T00:00:00Z", &[("a.md:1", OLD, "s1")]),
            run_with("2026-11-11T00:00:00Z", &[("a.md:1", OLD, "s1")]),
        ]);
        assert!(
            e.line().contains("on 2026-11-10 (adopted 2026-11-03)"),
            "{}",
            e.line()
        );
    }

    fn measure_item(unit: &str, count: u64, keys: Option<&[&str]>) -> MeasureItem {
        MeasureItem {
            schema: MEASURE_ITEM_SCHEMA.into(),
            check: "claims".into(),
            measure: Measure {
                schema: "rf.measure/1".into(),
                tool: "claimcheck".into(),
                unit: unit.into(),
                count,
                keys: keys.map(|k| k.iter().map(|k| (*k).to_string()).collect()),
            },
        }
    }

    /// Clean, measured `claims` runs, one per commit: (date, citing
    /// sentences so far, findings so far, each with its own sentence).
    fn measured_runs(steps: &[(&str, u64, usize)]) -> Vec<Run> {
        steps
            .iter()
            .enumerate()
            .map(|(i, &(day, units, found))| {
                let sentences: Vec<String> = (0..found).map(|k| format!("s{k}")).collect();
                let findings: Vec<(&str, &str, &str)> = sentences
                    .iter()
                    .map(|s| ("a.md:1", OLD, s.as_str()))
                    .collect();
                let mut run = run_with(&format!("{day}T00:00:00Z"), &findings);
                run.path = format!("{day}-{i}.jsonl").into();
                run.header.commit = Some(format!("c{i}"));
                run.header.dirty = Some(false);
                run.measures = vec![measure_item("citing-sentence", units, None)];
                run
            })
            .collect()
    }

    fn eval_min(runs: &[Run], min_sessions: usize) -> Evaluation {
        let cfg = EvaluateConfig {
            min_sessions,
            ..EvaluateConfig::default()
        };
        evaluate(
            &lesson("claimcheck:source-text-missing", "2026-11-03"),
            &Inputs {
                runs,
                configured: &["claims"],
                findings: &[],
                sessions: Err("none"),
                settings: &cfg,
            },
            utc_date,
        )
    }

    /// Three intervals of 100 new sentences on each side of adoption
    /// (2026-11-03), with `before` and `after` findings introduced in each.
    fn three_and_three(before: usize, after: usize) -> Vec<Run> {
        let mut steps = Vec::new();
        let mut found = 0;
        for (i, day) in ["2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04"]
            .into_iter()
            .enumerate()
        {
            found += if i == 0 { 0 } else { before };
            steps.push((day, 100 * i as u64, found));
        }
        for (i, day) in ["2026-11-03", "2026-11-04", "2026-11-05"]
            .into_iter()
            .enumerate()
        {
            found += after;
            steps.push((day, 400 + 100 * i as u64, found));
        }
        measured_runs(&steps)
    }

    #[test]
    fn fewer_findings_per_new_sentence_after_adoption_is_improved() {
        let e = eval_min(&three_and_three(10, 0), 3);
        assert_eq!(
            (e.unit, e.method, e.measure.as_deref()),
            (
                Some(Unit::Interval),
                Some(Method::ConditionalBinomial),
                Some("citing-sentence")
            )
        );
        assert_eq!(
            (
                e.before.n,
                e.before.findings,
                e.before.new_units,
                e.before.rate
            ),
            (3, Some(30), Some(300), Some(0.1))
        );
        assert_eq!(
            (e.after.n, e.after.findings, e.after.new_units),
            (3, Some(0), Some(300))
        );
        // Binomial(30, 1/2): P(X ≤ 0) = 2^-30.
        assert!((e.p_lower.unwrap() - 0.5f64.powi(30)).abs() < 1e-15);
        assert_eq!((e.verdict, e.reason.as_deref()), (Verdict::Improved, None));
        // The introduced findings are still described.
        assert_eq!(e.introduced.as_ref().map(|i| i.count), Some(0));
        let line = e.line();
        assert!(
            line.contains("10 → 0 introduced per 100 new citing-sentences (n 3 → 3 intervals: 30 in 300 → 0 in 300 new units"),
            "{line}"
        );
    }

    #[test]
    fn more_findings_per_new_sentence_after_adoption_is_worse() {
        let e = eval_min(&three_and_three(0, 10), 3);
        assert_eq!(e.verdict, Verdict::Worse);
        assert!((e.p_upper.unwrap() - 0.5f64.powi(30)).abs() < 1e-15);
        assert_eq!(e.introduced.as_ref().map(|i| i.count), Some(20));
    }

    #[test]
    fn the_same_rate_on_both_sides_is_inconclusive() {
        let e = eval_min(&three_and_three(1, 1), 3);
        assert_eq!(e.verdict, Verdict::Inconclusive);
        // Binomial(6, 1/2): P(X ≤ 3) = P(X ≥ 3) = 42/64.
        assert!((e.p_lower.unwrap() - 42.0 / 64.0).abs() < 1e-12);
        assert!((e.p_upper.unwrap() - 42.0 / 64.0).abs() < 1e-12);
    }

    #[test]
    fn intervals_after_adoption_alone_name_the_missing_baseline() {
        let runs = three_and_three(1, 1);
        let e = eval_min(&runs[3..], 3);
        assert_eq!(
            (e.verdict, e.method, e.before.n, e.after.n),
            (Verdict::Collecting, Some(Method::ConditionalBinomial), 0, 3)
        );
        let reason = e.reason.unwrap();
        assert!(
            reason.starts_with(
                "no baseline: 0 of 3 measured intervals before adoption, 3 of 3 after"
            ),
            "{reason}"
        );
        // Too few after adoption is collecting too.
        let e = eval_min(&runs, 4);
        assert_eq!(e.verdict, Verdict::Collecting);
        assert_eq!(
            e.reason.as_deref(),
            Some(
                "no baseline: 3 of 4 measured intervals before adoption, 3 of 4 after; the rate after adoption has nothing to compare with yet (`lab check --at` can replay commits before adoption)"
            )
        );
        let mut runs = runs;
        // One more commit before adoption, with sentences of its own.
        runs[0].measures[0].measure.count = 50;
        runs.insert(0, measured_runs(&[("2026-09-30", 0, 0)]).remove(0));
        runs[0].header.commit = Some("early".into());
        let e = eval_min(&runs, 4);
        assert_eq!(e.before.n, 4);
        assert_eq!(
            e.reason.as_deref(),
            Some("3 of 4 measured intervals after adoption, 4 of 4 before")
        );
    }

    #[test]
    fn replayed_runs_are_interval_ends_but_not_the_introduced_history() {
        // Live runs from c5 on; c0 to c4 replayed later, so stored after them.
        let mut runs = three_and_three(10, 0);
        let mut replayed: Vec<Run> = runs.drain(..5).collect();
        let live_only = eval_min(&runs, 2);
        assert_eq!((live_only.before.n, live_only.after.n), (0, 1));
        for r in &mut replayed {
            r.header.replayed = true;
        }
        runs.extend(replayed);
        let e = eval_min(&runs, 3);
        assert_eq!((e.before.n, e.after.n), (3, 3));
        assert_eq!((e.verdict, e.reason.as_deref()), (Verdict::Improved, None));
        // `introduced` runs from the first live run since adoption to the
        // latest live one, not to the replay of c4 stored after them.
        let found = e.introduced.unwrap();
        assert_eq!(
            (found.since.as_str(), found.latest_run.as_str()),
            ("2026-11-04T00:00:00Z", "2026-11-05-6.jsonl")
        );
    }

    #[test]
    fn intervals_across_a_checker_version_change_are_left_out() {
        let mut runs = three_and_three(10, 0);
        // c0 and c1 were stored before versions were recorded: unknown, so
        // they still form intervals with each other and with v1.
        let versions = [
            None,
            None,
            Some("v1"),
            Some("v1"),
            Some("v2"),
            Some("v2"),
            Some("v2"),
        ];
        for (r, v) in runs.iter_mut().zip(versions) {
            for c in &mut r.header.checks {
                c.version = v.map(Into::into);
            }
        }
        let e = eval_min(&runs, 2);
        assert_eq!((e.before.n, e.after.n), (3, 2));
        assert_eq!(e.after.new_units, Some(200));
        assert_eq!(e.verdict, Verdict::Improved);
        assert_eq!(
            e.reason.as_deref(),
            Some("1 interval(s) across a checker version change left out")
        );
        // An unknown version between v1 and v2 does not hide the change: it
        // is counted in the interval that reaches v2.
        let mut runs = three_and_three(10, 0);
        let versions = [
            Some("v1"),
            Some("v1"),
            Some("v1"),
            Some("v1"),
            None,
            Some("v2"),
            Some("v2"),
        ];
        for (r, v) in runs.iter_mut().zip(versions) {
            for c in &mut r.header.checks {
                c.version = v.map(Into::into);
            }
        }
        let e = eval_min(&runs, 2);
        assert_eq!((e.before.n, e.after.n), (3, 2));
        assert_eq!(
            e.reason.as_deref(),
            Some("1 interval(s) across a checker version change left out")
        );
        // A change in a check the metric does not need changes nothing.
        let mut runs = three_and_three(10, 0);
        for (i, r) in runs.iter_mut().enumerate() {
            let mut other = r.header.checks[0].clone();
            other.name = "other".into();
            other.version = Some(format!("v{i}"));
            r.header.checks.push(other);
        }
        let e = eval_min(&runs, 3);
        assert_eq!((e.before.n, e.after.n, e.reason), (3, 3, None));
    }

    #[test]
    fn only_the_last_clean_measured_run_on_each_commit_counts() {
        let mut runs = three_and_three(1, 1);
        // An earlier run on c3, a dirty run, a run without a commit and an
        // unmeasured run, all full of findings: none forms an interval.
        let noisy = |at: &str| {
            let sentences: Vec<String> = (0..50).map(|k| format!("x{k}")).collect();
            let f: Vec<(&str, &str, &str)> = sentences
                .iter()
                .map(|s| ("b.md:1", NEW, s.as_str()))
                .collect();
            let mut r = run_with(at, &f);
            r.header.commit = Some("c3".into());
            r.header.dirty = Some(false);
            r.measures = vec![measure_item("citing-sentence", 5000, None)];
            r
        };
        let early = noisy("2026-10-03T12:00:00Z");
        let mut dirty = noisy("2026-10-03T13:00:00Z");
        dirty.header.commit = Some("c9".into());
        dirty.header.dirty = Some(true);
        let mut old = noisy("2026-10-03T14:00:00Z");
        (old.header.commit, old.header.dirty) = (None, None);
        let mut unmeasured = noisy("2026-10-03T15:00:00Z");
        unmeasured.header.commit = Some("c8".into());
        unmeasured.measures.clear();
        // A commit with no new sentences is left out.
        let mut same = measured_runs(&[("2026-11-05", 600, 6)]).remove(0);
        same.header.at = "2026-11-05T01:00:00Z".into();
        same.header.commit = Some("c-same".into());
        for (i, r) in [early, dirty, old, unmeasured].into_iter().enumerate() {
            runs.insert(3 + i, r);
        }
        runs.push(same);
        let e = eval_min(&runs, 3);
        assert_eq!(
            (e.before.n, e.before.findings, e.before.new_units),
            (3, Some(3), Some(300))
        );
        assert_eq!(
            (e.after.n, e.after.findings, e.after.new_units),
            (3, Some(3), Some(300))
        );
        assert_eq!(e.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn keys_count_text_that_replaced_deleted_text_as_new() {
        let run = |keys: &[&str]| {
            let mut r = run_with("2026-11-04T00:00:00Z", &[]);
            r.measures = vec![measure_item(
                "citing-sentence",
                keys.len() as u64,
                Some(keys),
            )];
            r
        };
        let required: BTreeSet<&str> = ["claims"].into();
        let (a, b) = (run(&["aa", "bb"]), run(&["aa", "cc"]));
        let (ma, mb) = (
            measured(&a, "claimcheck", &required),
            measured(&b, "claimcheck", &required),
        );
        assert_eq!(new_units(&ma, &mb), 1);
        // Without keys on both sides, only the count's increase is new.
        let mut c = run(&["aa", "cc"]);
        c.measures[0].measure.keys = None;
        let mc = measured(&c, "claimcheck", &required);
        assert_eq!(new_units(&ma, &mc), 0);
        assert!(measured(&c, "othertool", &required).is_empty());
    }

    #[test]
    fn only_intervals_in_the_latest_unit_count() {
        let mut runs = three_and_three(1, 1);
        // The checker renamed its unit after the fourth run.
        for r in &mut runs[..4] {
            r.measures[0].measure.unit = "sentence".into();
        }
        let e = eval_min(&runs, 3);
        assert_eq!(
            (e.verdict, e.measure.as_deref(), e.before.n, e.after.n),
            (Verdict::Collecting, Some("citing-sentence"), 0, 2)
        );
        let reason = e.reason.unwrap();
        assert!(
            reason.starts_with("no baseline: 0 of 3")
                && reason.ends_with(
                    "; 3 interval(s) measured in another unit than `citing-sentence` left out"
                ),
            "{reason}"
        );
        // A check measuring two units in one run ends no interval.
        let mut runs = three_and_three(1, 1);
        runs[6].measures.push(measure_item("sentence", 1, None));
        let e = eval_min(&runs, 3);
        assert_eq!((e.before.n, e.after.n), (3, 2));
    }

    #[test]
    fn a_missing_reviewed_run_is_named_beside_a_verdict() {
        let cfg = EvaluateConfig {
            min_sessions: 3,
            ..EvaluateConfig::default()
        };
        let mut l = lesson("claimcheck:source-text-missing", "2026-11-03");
        l.meta.reviewed = Some("gone.jsonl".into());
        let runs = three_and_three(10, 0);
        let e = evaluate(
            &l,
            &Inputs {
                runs: &runs,
                configured: &["claims"],
                findings: &[],
                sessions: Err("none"),
                settings: &cfg,
            },
            utc_date,
        );
        assert_eq!(e.verdict, Verdict::Improved);
        assert!(
            e.reason
                .as_deref()
                .is_some_and(|r| r.starts_with("the reviewed run `gone.jsonl` is not a stored")),
            "{:?}",
            e.reason
        );
    }
}
