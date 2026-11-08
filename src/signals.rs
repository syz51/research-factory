//! `rf.signal/1`: what `lab loop` asks a person or agent to act on, and the
//! state that keeps it from repeating itself.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::evaluate::{Evaluation, Method, Unit, Verdict, p_label, p_value};
use crate::lessons::{Lesson, Status};

pub const SCHEMA: &str = "rf.signal/1";
pub const STATE_SCHEMA: &str = "rf.loop-signals/1";
const TITLE_MAX: usize = 72;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub schema: String,
    pub project: String,
    /// Stable signature: the same problem keeps its id across loops.
    pub id: String,
    pub kind: String,
    pub lesson: Option<String>,
    /// Imperative, at most 72 characters.
    pub title: String,
    /// Standalone Markdown: what was measured, the numbers and the commands to act.
    pub body: String,
    /// Short summary whose change makes the signal worth sending again.
    pub detail: String,
    /// A changed signal sent again; `ref` is what the first send returned.
    pub repeat: bool,
    /// A signal that had been resolved and came back.
    pub reopened: bool,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub first_seen: String,
    pub at: String,
}

/// A recurring finding rule that no lesson covers (from `lab retro`).
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub tool: String,
    pub rule: String,
    pub severity: String,
    /// Its count toward the threshold, as `lab retro` describes it.
    pub count: String,
    pub sample: Vec<String>,
}

/// The recurring rules without a lesson, and why agent findings were not
/// counted per session group, if they were not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Uncovered {
    pub candidates: Vec<Candidate>,
    pub ungrouped: Option<String>,
}

/// A measure command that did not succeed.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureFailure {
    pub index: usize,
    pub argv: Vec<String>,
    pub status: String,
    /// The last lines of its output.
    pub tail: String,
}

fn title(s: String) -> String {
    if s.chars().count() <= TITLE_MAX {
        return s;
    }
    let mut t: String = s.chars().take(TITLE_MAX - 1).collect();
    t.push('…');
    t
}

/// Single-quotes a word for a POSIX shell when it needs it.
fn shell_word(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:=@%+,".contains(&b))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn command(argv: &[String]) -> String {
    argv.iter()
        .map(|a| shell_word(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The numbers of an evaluation, as Markdown lines.
/// A paragraph naming a checker version change between the runs introduced
/// findings are counted from, or nothing.
fn version_note(i: &crate::evaluate::Introduced) -> String {
    i.version_text().map_or_else(String::new, |v| {
        format!(
            "\nNote: {v}. Fix the findings that new text caused; `lab lesson review` records \
             all listed findings at once, so run it only when every remaining one comes from \
             the update or cannot be fixed.\n\n"
        )
    })
}

fn measured(e: &Evaluation) -> String {
    let mut out = String::new();
    let unit = e.unit.map_or("observation", |u| u.as_str());
    let method = match e.method {
        Some(Method::StratifiedHypergeometric) => {
            "exact stratified hypergeometric test on the share of sessions with at least one finding, strata = (source, role)"
        }
        Some(Method::VanElteren) => {
            "van Elteren stratified rank-sum test, strata = (source, role); lower is better"
        }
        Some(Method::ClusterPermutation) => {
            "stratified permutation test that moves whole session groups (the subagents below one root session, or one session) between the sides, on the share of sessions with the finding or a field's rank within its stratum, strata = (source, role); lower is better"
        }
        Some(Method::Descriptive) => {
            "findings introduced since the first complete check run after adoption, or since the run of the last `lab lesson review`, keyed by message and data, without a verdict (runs repeat the same backlog, so they are not independent; a verdict needs measured intervals)"
        }
        Some(Method::ConditionalBinomial) => {
            "exact conditional binomial test on findings introduced per new unit of checked text, over intervals between clean check runs on consecutive commits; given the total introduced, the after side's count is binomial with its share of the new units"
        }
        None => "none",
    };
    let _ = writeln!(
        out,
        "- Metric: `{}`, per {unit}",
        e.metric.as_deref().unwrap_or("none")
    );
    let _ = writeln!(out, "- Method: {method}");
    let side = |s: &crate::evaluate::Side| {
        let count = match s.sessions {
            Some(sessions) if sessions == s.n => format!("{sessions} sessions"),
            Some(sessions) => format!("{sessions} sessions in {} {unit}s", s.n),
            None => format!("{} {unit}s", s.n),
        };
        if let (Some(f), Some(u)) = (s.findings, s.new_units) {
            let measure = e.measure.as_deref().unwrap_or("unit");
            return format!("{count}: {f} introduced in {u} new {measure}s");
        }
        match (s.positives, s.rate, s.median, s.mean) {
            (Some(p), Some(r), _, _) => format!("{p} of {count} ({:.1}%)", r * 100.0),
            (_, _, Some(med), Some(mean)) if s.sessions.is_some() => {
                format!("{count}, median {med}, mean {mean:.2}")
            }
            (_, _, _, Some(mean)) => format!("{count}, mean {mean:.2}"),
            _ => count,
        }
    };
    let window = if e.baseline_days == 0 {
        format!("all {unit}s")
    } else {
        format!("{unit}s in the {} days", e.baseline_days)
    };
    let _ = writeln!(
        out,
        "- Before adoption ({window} before {}): {}",
        e.adopted.as_deref().unwrap_or("?"),
        side(&e.before)
    );
    let _ = writeln!(out, "- After adoption: {}", side(&e.after));
    if let (Some(lo), Some(up)) = (e.p_lower, e.p_upper) {
        let _ = writeln!(
            out,
            "- p (lower after) = {}, p (higher after) = {}; each is tested at alpha/2 = {}",
            p_value(lo),
            p_value(up),
            e.alpha / 2.0
        );
    }
    if !e.strata.is_empty() {
        let strata: Vec<String> = e
            .strata
            .iter()
            .map(|s| format!("{}/{} {} → {}", s.source, s.role, s.before, s.after))
            .collect();
        let _ = writeln!(
            out,
            "- Strata ({unit}s before → after): {}; {} {unit}(s) in strata without both sides left out",
            strata.join(", "),
            e.excluded
        );
    }
    let _ = writeln!(
        out,
        "- Verdict: **{}**{}",
        e.verdict.as_str(),
        e.reason
            .as_deref()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default()
    );
    out
}

fn summary(e: &Evaluation) -> String {
    let mut s = format!(
        "{} n {} → {}",
        e.metric.as_deref().unwrap_or("-"),
        e.before.n,
        e.after.n
    );
    match (e.before.rate, e.after.rate) {
        (Some(b), Some(a)) if e.unit == Some(Unit::Interval) => {
            let _ = write!(
                s,
                ", {:.2} → {:.2} per 100 new {}s",
                b * 100.0,
                a * 100.0,
                e.measure.as_deref().unwrap_or("unit")
            );
        }
        (Some(b), Some(a)) => {
            let _ = write!(s, ", {:.1}% → {:.1}% of sessions", b * 100.0, a * 100.0);
        }
        _ => {
            if let (Some(b), Some(a)) = (e.before.mean, e.after.mean) {
                let _ = write!(s, ", mean {b:.2} → {a:.2}");
            }
        }
    }
    let method = match e.method {
        Some(Method::StratifiedHypergeometric) => "stratified hypergeometric",
        Some(Method::VanElteren) => "van Elteren",
        Some(Method::Descriptive) => "descriptive",
        Some(Method::ClusterPermutation) => "cluster permutation",
        Some(Method::ConditionalBinomial) => "conditional binomial",
        None => "",
    };
    match (e.verdict, e.p_lower, e.p_upper) {
        (Verdict::Improved, Some(p), _) | (Verdict::Worse, _, Some(p)) => {
            let _ = write!(s, ", {method} {}", p_label(p));
        }
        _ => {
            let _ = write!(s, ", {method}");
        }
    }
    s
}

const CAVEAT: &str = "The comparison is observational: other changes since adoption can explain a difference, and with several lessons measured some will cross the threshold by chance.";

/// The signals this loop's results call for, before deduplication.
/// `root` is the project root, which lesson paths are shown relative to.
pub fn derive(
    project: &str,
    root: &Path,
    now: &str,
    lessons: &[Lesson],
    evaluations: &[Evaluation],
    uncovered: &Uncovered,
    failures: &[MeasureFailure],
) -> Vec<Signal> {
    let signal =
        |id: String, kind: &str, lesson: Option<&str>, t: String, body: String, detail: String| {
            Signal {
                schema: SCHEMA.into(),
                project: project.into(),
                id,
                kind: kind.into(),
                lesson: lesson.map(Into::into),
                title: title(t),
                body,
                detail,
                repeat: false,
                reopened: false,
                reference: None,
                first_seen: now.into(),
                at: now.into(),
            }
        };
    let mut out = Vec::new();
    for e in evaluations {
        let Some(l) = lessons.iter().find(|l| l.meta.id == e.lesson) else {
            continue;
        };
        let id = &l.meta.id;
        let head = format!(
            "**Lesson `{id}`: {}** ({} since {})\n\nRule: {}\n\n",
            l.meta.title.trim(),
            l.meta.status.as_str(),
            l.meta.adopted.as_deref().unwrap_or("?"),
            l.meta.rule.trim()
        );
        let file = l.path.strip_prefix(root).unwrap_or(&l.path).display();
        let note = summary(e).replace('"', "'");
        let open = e.introduced.as_ref().filter(|i| i.count > 0);
        let (kind, t, act) = match (e.verdict, l.meta.status) {
            (Verdict::Improved, Status::Adopted) => (
                "lesson-verifiable",
                format!("Verify lesson {id}: its metric improved"),
                format!(
                    "Its metric is lower since adoption. Read the numbers, then mark it verified:\n\n\
                     ```sh\nlab lesson set {id} verified --note \"lab evaluate: {note}\"\n```\n{}",
                    open.map(|i| format!(
                        "\n{} finding(s) of its rule were still introduced since {}; fix them too:\n\n```\n{}\n```\n{}",
                        i.count,
                        i.since_text(e.adopted.as_deref()),
                        e.introduced_lines().join("\n"),
                        version_note(i)
                    ))
                    .unwrap_or_default()
                ),
            ),
            (Verdict::Worse, Status::Adopted | Status::Verified) => (
                "lesson-worse",
                format!("Revise or retire lesson {id}: its metric got worse"),
                format!(
                    "Its metric is higher since adoption. Revise the rule in `{file}`, or retire the lesson:\n\n\
                     ```sh\nlab lesson set {id} retired --note \"lab evaluate: {note}\"\n```\n"
                ),
            ),
            // A verified lesson stays verified on other evidence; its
            // missing or blind metric needs no further action.
            (Verdict::Unmeasurable, Status::Adopted) => (
                "lesson-unmeasurable",
                format!("Choose a measurable metric for lesson {id}"),
                format!(
                    "This metric cannot show whether the lesson helps. Choose a metric that can (edit `metric:` in `{file}`; \
                     a finding rule as `tool: <tool>, rule: <rule>`, or a numeric session field as `rule: session.<field>`), \
                     or decide on other evidence:\n\n\
                     ```sh\nlab lesson set {id} verified --force --note \"<the evidence>\"\nlab lesson set {id} retired --note \"<why>\"\n```\n"
                ),
            ),
            // Any other check-run lesson with introduced findings, verified
            // ones included: the findings are what to act on.
            (verdict, Status::Adopted | Status::Verified) if open.is_some() => {
                let i = open.expect("checked above");
                let next = match verdict {
                    Verdict::Inconclusive => "The measured intervals show no change in either direction yet; to verify the lesson sooner, cite other evidence with `--force`.".to_string(),
                    Verdict::Improved => "Its metric is lower since adoption; keep it that way.".to_string(),
                    _ => format!(
                        "A verdict needs {} measured intervals on each side of adoption (`lab evaluate` says how many there are); to verify the lesson sooner, cite other evidence with `--force`.",
                        e.min_sessions
                    ),
                };
                (
                    "lesson-introduced",
                    if i.reviewed.is_some() {
                        format!("Review findings introduced since lesson {id} was last reviewed")
                    } else {
                        format!("Review findings introduced since lesson {id} was adopted")
                    },
                    format!(
                        "{} finding(s) of its rule appeared since {}, by message and data, with where the new ones are:\n\n```\n{}\n```\n\n{}\
                         Fix each, or show that the finding is wrong. When `lab evaluate` lists only findings that cannot be fixed, such as those in append-only files, record their review with their count:\n\n\
                         ```sh\nlab lesson review {id} --count <n> --note \"<what was found>\"\n```\n\n\
                         If such findings keep appearing, revise the rule in `{file}` or retire the lesson. {next}\n",
                        i.count,
                        i.since_text(e.adopted.as_deref()),
                        e.introduced_lines().join("\n"),
                        version_note(i)
                    ),
                )
            }
            _ => continue,
        };
        let detail = match (e.verdict, open) {
            (Verdict::Unmeasurable, _) => {
                format!("unmeasurable: {}", e.reason.as_deref().unwrap_or(""))
            }
            // A changed set of introduced findings, or a review that moved the
            // run they are counted from, is news; their subjects alone are not.
            (_, Some(i)) if kind == "lesson-introduced" => {
                let keys: Vec<String> = i
                    .findings
                    .iter()
                    .map(|f| format!("+{} {}", f.added, f.message))
                    .collect();
                let versions: Vec<String> = i
                    .version_changed
                    .iter()
                    .map(|(check, [old, new])| format!("{check} {old}→{new}"))
                    .collect();
                let versions = if versions.is_empty() {
                    String::new()
                } else {
                    format!(" (checker {})", versions.join(", "))
                };
                format!(
                    "introduced since {}{versions}: {}",
                    i.since,
                    keys.join("; ")
                )
            }
            (v, _) => v.as_str().to_string(),
        };
        let body = format!("{head}{}\n{act}\n{CAVEAT}\n", measured(e));
        out.push(signal(
            format!("{kind}:{id}"),
            kind,
            Some(id),
            t,
            body,
            detail,
        ));
    }
    let candidates = &uncovered.candidates;
    if !candidates.is_empty() {
        let mut body = format!(
            "{} recurring finding rule(s) at warn or error have no lesson measuring them:\n\n",
            candidates.len()
        );
        for c in candidates {
            let _ = writeln!(
                body,
                "- {} `{}:{}`: {}; e.g. {}",
                c.severity,
                c.tool,
                c.rule,
                c.count,
                c.sample.join(", ")
            );
        }
        if let Some(why) = &uncovered.ungrouped {
            let _ = writeln!(body, "\nNote: {why}.");
        }
        body.push_str(
            "\nDraft a lesson for each as `observed`, find the cause, and replace the drafted rule with the practice that prevents it; adopt it only after reading its evidence:\n\n\
             ```sh\nlab retro --write\nlab lesson set <id> adopted --note \"<why this rule prevents it>\"\n```\n",
        );
        let mut keys: Vec<String> = candidates
            .iter()
            .map(|c| format!("{}:{}", c.tool, c.rule))
            .collect();
        keys.sort();
        out.push(signal(
            "uncovered-rules".into(),
            "uncovered-rules",
            None,
            format!(
                "Write lessons for {} recurring finding rule(s)",
                candidates.len()
            ),
            body,
            keys.join(","),
        ));
    }
    for f in failures {
        let cmd = command(&f.argv);
        let body = format!(
            "The measure command {} of `[lab.loop]` failed ({}), so the evaluations may use stale data.\n\n\
             ```sh\n{cmd}\n```\n\nLast output:\n\n```text\n{}\n```\n\n\
             Run it by hand from the project root to see the error, fix it or its configuration in `research.toml`, then run `lab loop`.\n",
            f.index,
            f.status,
            f.tail.trim_end()
        );
        out.push(signal(
            format!("measure-failed:{}", f.index),
            "measure-failed",
            None,
            format!(
                "Fix the failing measure command: {}",
                f.argv.first().map_or("", String::as_str)
            ),
            body,
            format!("{cmd}: {}", f.status),
        ));
    }
    out
}

/// Why a pending signal is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pending {
    New,
    Repeat,
    Reopened,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub first_seen: String,
    pub last_notified: Option<String>,
    /// Hash of the detail last sent, or waiting to be sent.
    pub detail_hash: String,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub resolved: Option<String>,
    pub error: Option<String>,
    /// Set until a send succeeds.
    #[serde(default)]
    pub pending: Option<Pending>,
}

/// `signals.json`: every signal id seen, and what was sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub schema: String,
    pub signals: BTreeMap<String, Entry>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema: STATE_SCHEMA.into(),
            signals: BTreeMap::new(),
        }
    }
}

fn hash(s: &str) -> String {
    Sha256::digest(s.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl State {
    /// The state in `path`; empty when the file does not exist.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let s: Self =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if s.schema != STATE_SCHEMA {
            return Err(format!("{}: not a {STATE_SCHEMA} record", path.display()));
        }
        Ok(s)
    }

    /// Records this loop's signals and returns, for each, whether to send it.
    ///
    /// A signal is sent when its id is new, when it was resolved and came back
    /// at least `renotify` after its last send (`reopened`), when its detail changed and it was last sent at least
    /// `renotify` ago (`repeat`, with the stored `ref`), and on every loop
    /// until a send succeeds. An id that this loop did not produce is
    /// resolved.
    pub fn plan(
        &mut self,
        signals: &mut [Signal],
        now: jiff::Timestamp,
        renotify: jiff::SignedDuration,
    ) -> Vec<bool> {
        let at = now.to_string();
        let mut send = Vec::with_capacity(signals.len());
        for s in signals.iter_mut() {
            let h = hash(&s.detail);
            let e = self.signals.entry(s.id.clone()).or_insert_with(|| Entry {
                first_seen: at.clone(),
                last_notified: None,
                detail_hash: h.clone(),
                reference: None,
                resolved: None,
                error: None,
                pending: Some(Pending::New),
            });
            if e.resolved.is_some() {
                e.resolved = None;
                // A signal that went away and came back within `renotify` of
                // its last send is flapping: stay quiet. The stored hash is
                // still the one sent, so a changed detail repeats once
                // `renotify` has passed.
                let recent = e
                    .last_notified
                    .as_deref()
                    .and_then(|t| t.parse::<jiff::Timestamp>().ok())
                    .is_some_and(|t| now.duration_since(t) < renotify);
                if !recent {
                    e.pending = Some(Pending::Reopened);
                    e.detail_hash = h;
                }
            } else if e.pending.is_some() {
                e.detail_hash = h;
            } else if e.detail_hash != h {
                let due = e
                    .last_notified
                    .as_deref()
                    .and_then(|t| t.parse::<jiff::Timestamp>().ok())
                    .is_none_or(|t| now.duration_since(t) >= renotify);
                if due {
                    e.pending = Some(Pending::Repeat);
                    e.detail_hash = h;
                }
            }
            s.first_seen = e.first_seen.clone();
            s.reference = e.reference.clone();
            s.repeat = e.pending == Some(Pending::Repeat);
            s.reopened = e.pending == Some(Pending::Reopened);
            send.push(e.pending.is_some());
        }
        for (id, e) in &mut self.signals {
            if e.resolved.is_none() && !signals.iter().any(|s| &s.id == id) {
                e.resolved = Some(at.clone());
                e.pending = None;
            }
        }
        send
    }

    /// A send succeeded; `reference` is the notify command's answer, if any.
    pub fn sent(&mut self, id: &str, now: jiff::Timestamp, reference: Option<String>) {
        if let Some(e) = self.signals.get_mut(id) {
            e.last_notified = Some(now.to_string());
            e.pending = None;
            e.error = None;
            if reference.is_some() {
                e.reference = reference;
            }
        }
    }

    /// A send failed; it stays pending and is retried on the next loop.
    pub fn failed(&mut self, id: &str, error: String) {
        if let Some(e) = self.signals.get_mut(id) {
            e.error = Some(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(id: &str, detail: &str) -> Signal {
        Signal {
            schema: SCHEMA.into(),
            project: "p".into(),
            id: id.into(),
            kind: "k".into(),
            lesson: None,
            title: "t".into(),
            body: "b".into(),
            detail: detail.into(),
            repeat: false,
            reopened: false,
            reference: None,
            first_seen: String::new(),
            at: String::new(),
        }
    }

    #[test]
    fn a_review_changes_the_introduced_signal_title_and_detail() {
        use crate::evaluate::{Introduced, IntroducedFinding, Method, Side, Unit};
        use crate::lessons::{Kind, Meta};
        let lesson = Lesson {
            path: "q.md".into(),
            meta: Meta {
                id: "q".into(),
                title: "t".into(),
                kind: Kind::Bad,
                status: Status::Adopted,
                scope: "writing".into(),
                tags: vec![],
                rule: "r".into(),
                metric: Some("claimcheck:quote-not-found".parse().unwrap()),
                created: "2026-11-01".into(),
                updated: "2026-11-01".into(),
                adopted: Some("2026-11-01".into()),
                reviewed: None,
                evidence: vec![],
            },
            body: String::new(),
        };
        let eval = |since: &str, reviewed: Option<&str>| Evaluation {
            schema: crate::evaluate::SCHEMA.into(),
            lesson: "q".into(),
            lesson_status: Status::Adopted,
            metric: Some("claimcheck:quote-not-found".into()),
            unit: Some(Unit::Run),
            method: Some(Method::Descriptive),
            verdict: Verdict::Collecting,
            reason: None,
            adopted: Some("2026-11-01".into()),
            before: Side::default(),
            after: Side::default(),
            p_lower: None,
            p_upper: None,
            strata: vec![],
            excluded: 0,
            sessions_needed: None,
            min_sessions: 10,
            alpha: 0.05,
            baseline_days: 90,
            introduced: Some(Introduced {
                since: since.into(),
                since_date: Some(since[..10].into()),
                latest: "2026-11-05T00:00:00Z".into(),
                latest_run: "r5.jsonl".into(),
                reviewed: reviewed.map(Into::into),
                count: 1,
                findings: vec![IntroducedFinding {
                    message: "quote missing".into(),
                    added: 1,
                    subjects: vec!["a.md:1".into()],
                    more: 0,
                }],
                version_changed: Default::default(),
            }),
            measure: None,
        };
        let one = |e: Evaluation| {
            derive(
                "p",
                Path::new("."),
                "now",
                std::slice::from_ref(&lesson),
                &[e],
                &Uncovered::default(),
                &[],
            )
            .remove(0)
        };
        let before = one(eval("2026-11-01T00:00:00Z", None));
        let after = one(eval("2026-11-04T00:00:00Z", Some("r4.jsonl")));
        assert_eq!(before.kind, "lesson-introduced");
        assert!(before.title.contains("was adopted"), "{}", before.title);
        assert!(after.title.contains("was last reviewed"), "{}", after.title);
        // The same message after a review is a new detail, so it is sent again.
        assert_ne!(before.detail, after.detail);
        assert!(
            after.body.contains("lab lesson review q --count <n>"),
            "{}",
            after.body
        );
        // A checker update between the two runs is named in the body.
        assert!(!before.body.contains("version changed"), "{}", before.body);
        let mut changed = eval("2026-11-01T00:00:00Z", None);
        if let Some(i) = &mut changed.introduced {
            i.version_changed = [("claims".to_string(), ["v1".into(), "v2".into()])].into();
        }
        let s = one(changed);
        assert_eq!(s.kind, "lesson-introduced");
        // The change alone is news: it changes the detail.
        assert_ne!(s.detail, before.detail);
        assert!(
            s.body.contains(
                "Note: the checker version changed since that run (claims v1 → v2), so the \
                 update alone can add or remove findings"
            ) && s.body.contains("records all listed findings at once"),
            "{}",
            s.body
        );

        // Measured intervals: an inconclusive verdict still lists the
        // introduced findings; an improved one asks for verification.
        let interval = |verdict: Verdict| {
            let side = |findings: u64| Side {
                n: 10,
                rate: Some(findings as f64 / 300.0),
                findings: Some(findings),
                new_units: Some(300),
                ..Side::default()
            };
            Evaluation {
                unit: Some(Unit::Interval),
                method: Some(Method::ConditionalBinomial),
                measure: Some("citing-sentence".into()),
                verdict,
                before: side(30),
                after: side(3),
                p_lower: Some(0.000_001),
                p_upper: Some(1.0),
                ..eval("2026-11-01T00:00:00Z", None)
            }
        };
        let s = one(interval(Verdict::Inconclusive));
        assert_eq!(s.kind, "lesson-introduced");
        assert_eq!(s.detail, before.detail);
        let s = one(interval(Verdict::Improved));
        assert_eq!(s.kind, "lesson-verifiable");
        for text in [
            "exact conditional binomial test",
            "Before adoption (intervals in the 90 days before 2026-11-01): 10 intervals: 30 introduced in 300 new citing-sentences",
            "After adoption: 10 intervals: 3 introduced in 300 new citing-sentences",
        ] {
            assert!(s.body.contains(text), "{text:?} in\n{}", s.body);
        }
        assert!(s.body.contains("were still introduced since"), "{}", s.body);
        // A verified lesson keeps the introduced signal after an improvement.
        let mut verified = lesson.clone();
        verified.meta.status = Status::Verified;
        let s = derive(
            "p",
            Path::new("."),
            "now",
            std::slice::from_ref(&verified),
            &[interval(Verdict::Improved)],
            &Uncovered::default(),
            &[],
        )
        .remove(0);
        assert_eq!(
            (s.kind.as_str(), s.detail),
            ("lesson-introduced", before.detail)
        );
        assert!(s.body.contains("keep it that way"), "{}", s.body);
    }

    #[test]
    fn the_uncovered_rules_signal_says_why_findings_were_not_grouped() {
        let candidate = Candidate {
            tool: "agentprof".into(),
            rule: "reread-file".into(),
            severity: "warn".into(),
            count: "3 distinct finding(s)".into(),
            sample: vec!["claude:a".into()],
        };
        let body = |ungrouped: Option<&str>| {
            let uncovered = Uncovered {
                candidates: vec![candidate.clone()],
                ungrouped: ungrouped.map(Into::into),
            };
            derive("p", Path::new("."), "now", &[], &[], &uncovered, &[])
                .remove(0)
                .body
        };
        let noted = body(Some("sessions file unreadable"));
        assert!(
            noted.contains("`agentprof:reread-file`: 3 distinct finding(s); e.g. claude:a")
                && noted.matches("Note: sessions file unreadable.").count() == 1,
            "{noted}"
        );
        assert!(!body(None).contains("Note:"));
    }

    fn t(h: i64) -> jiff::Timestamp {
        jiff::Timestamp::from_second(1_800_000_000 + h * 3600).unwrap()
    }

    #[test]
    fn dedupe_sends_new_changed_and_reopened_signals_once() {
        let day = jiff::SignedDuration::from_hours(24);
        let mut st = State::default();
        let mut s = vec![sig("a", "one")];
        assert_eq!(st.plan(&mut s, t(0), day), [true], "new");
        assert!(!s[0].repeat && !s[0].reopened);
        st.sent("a", t(0), Some("board:42".into()));

        let mut s = vec![sig("a", "one")];
        assert_eq!(st.plan(&mut s, t(1), day), [false], "same detail");
        assert_eq!(s[0].reference.as_deref(), Some("board:42"));

        let mut s = vec![sig("a", "two")];
        assert_eq!(
            st.plan(&mut s, t(2), day),
            [false],
            "changed within renotify_hours"
        );
        let mut s = vec![sig("a", "two")];
        assert_eq!(
            st.plan(&mut s, t(25), day),
            [true],
            "changed, renotify_hours passed"
        );
        assert!(s[0].repeat && !s[0].reopened);
        assert_eq!(s[0].reference.as_deref(), Some("board:42"));
        st.sent("a", t(25), None);
        assert_eq!(
            st.signals["a"].reference.as_deref(),
            Some("board:42"),
            "an empty answer keeps the ref"
        );

        let mut s = vec![];
        assert!(st.plan(&mut s, t(26), day).is_empty());
        assert_eq!(
            st.signals["a"].resolved,
            Some(t(26).to_string()),
            "absent: resolved"
        );

        let mut s = vec![sig("a", "two")];
        assert_eq!(
            st.plan(&mut s, t(27), day),
            [false],
            "reappeared within renotify_hours of the last send: flapping"
        );
        assert_eq!(st.signals["a"].resolved, None);
        st.plan(&mut [], t(28), day);

        let mut s = vec![sig("a", "two")];
        assert_eq!(st.plan(&mut s, t(49), day), [true], "reappeared later");
        assert!(s[0].reopened && !s[0].repeat);
        assert_eq!(s[0].reference.as_deref(), Some("board:42"));
        assert_eq!(s[0].first_seen, t(0).to_string());
        assert_eq!(st.signals["a"].resolved, None);
    }

    #[test]
    fn a_flapping_signal_with_a_new_detail_repeats_after_renotify() {
        let day = jiff::SignedDuration::from_hours(24);
        let mut st = State::default();
        st.plan(&mut [sig("a", "one")], t(0), day);
        st.sent("a", t(0), Some("r".into()));
        st.plan(&mut [], t(1), day);
        let mut s = vec![sig("a", "two")];
        assert_eq!(st.plan(&mut s, t(2), day), [false], "back within a day");
        let mut s = vec![sig("a", "two")];
        assert_eq!(st.plan(&mut s, t(25), day), [true], "the change is sent");
        assert!(s[0].repeat && !s[0].reopened);
    }

    #[test]
    fn a_failed_send_is_retried_with_its_flags() {
        let day = jiff::SignedDuration::from_hours(24);
        let mut st = State::default();
        let mut s = vec![sig("a", "one")];
        st.plan(&mut s, t(0), day);
        st.failed("a", "exit 1: board down".into());
        assert_eq!(st.signals["a"].last_notified, None);
        let mut s = vec![sig("a", "one")];
        assert_eq!(st.plan(&mut s, t(1), day), [true], "retried next loop");
        st.sent("a", t(1), Some("r1".into()));
        assert_eq!(st.signals["a"].error, None);
        // A reopen whose send fails stays a reopen.
        st.plan(&mut [], t(2), day);
        let mut s = vec![sig("a", "one")];
        assert_eq!(st.plan(&mut s, t(30), day), [true]);
        st.failed("a", "timeout".into());
        let mut s = vec![sig("a", "one")];
        assert_eq!(st.plan(&mut s, t(31), day), [true]);
        assert!(s[0].reopened);
    }

    #[test]
    fn titles_are_short_and_commands_quoted() {
        let long = title("x".repeat(100));
        assert_eq!(long.chars().count(), 72);
        assert_eq!(
            command(&[
                "agentprof".into(),
                "scan".into(),
                "a b".into(),
                "it's".into()
            ]),
            "agentprof scan 'a b' 'it'\\''s'"
        );
    }
}
