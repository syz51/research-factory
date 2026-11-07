//! `lab evaluate`: whether each active lesson's metric changed after adoption.
//!
//! Agent-session metrics are measured per session and tested within strata
//! of comparable sessions (same source and role). The subagents below one
//! root session share its task, so they are not independent: they form one
//! session group, the unit counted and moved by the test. Without such
//! groups, a finding rule is tested by the exact stratified hypergeometric
//! test on per-session indicators and a numeric session field by the van
//! Elteren test; with them, both by a stratified cluster permutation test. Check runs repeat the same backlog and are not
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

pub const SCHEMA: &str = "rf.evaluation/2";

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
    /// One agent session: `rf.evaluation/1` records.
    Session,
    /// A root session, or the subagents below one root session.
    #[serde(rename = "session-group")]
    SessionGroup,
    Run,
}

impl Unit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::SessionGroup => "session group",
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
    /// Whole session groups permuted between the sides (agent sessions
    /// with subagents).
    ClusterPermutation,
}

/// One side of the adoption date.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Side {
    /// Observations: session groups, sessions or check runs.
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
}

/// A stratum that contributes to the test: observations before and after.
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
    /// Observations left out because their stratum has none on the other side.
    pub excluded: usize,
    /// Estimated clean observations after adoption still needed to show a
    /// decrease, in the evaluation's unit.
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
            if let (Some(sb), Some(sa)) = (b.sessions, a.sessions) {
                let _ = write!(out, " {unit}s from {sb} → {sa} sessions");
            }
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
            let unit = self.unit.map_or("session", Unit::as_str);
            let _ = write!(
                out,
                "; about {m} more clean {unit}(s) in the baseline's mix would show a decrease"
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
        sessions: None,
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
        sessions: None,
        positives: Some(positives),
        rate: (!values.is_empty()).then(|| positives as f64 / values.len() as f64),
        mean: None,
        median: None,
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

/// The root of a subagent's tree: its parent's parent and so on, up to a
/// session without a parent or a parent that `by_id` does not have; `None`
/// for a session without a parent. The bound stops a cycle.
fn root_of<'a>(by_id: &BTreeMap<(&str, &str), &'a Session>, s: &'a Session) -> Option<&'a str> {
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
    let by_id: BTreeMap<(&str, &str), &Session> = sessions
        .iter()
        .map(|s| ((s.source.as_str(), s.id.as_str()), s))
        .collect();
    let root = |s| root_of(&by_id, s);
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
