//! `rf.signal/1`: what `lab loop` asks a person or agent to act on, and the
//! state that keeps it from repeating itself.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::evaluate::{Evaluation, Method, Verdict, p_label, p_value};
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
    pub findings: usize,
    pub sample: Vec<String>,
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
        Some(Method::Descriptive) => {
            "mean findings per complete check run, without a significance test (runs repeat the same backlog, so they are not independent)"
        }
        None => "none",
    };
    let _ = writeln!(
        out,
        "- Metric: `{}`, per {unit}",
        e.metric.as_deref().unwrap_or("none")
    );
    let _ = writeln!(out, "- Method: {method}");
    let side = |s: &crate::evaluate::Side| match (s.positives, s.rate, s.median, s.mean) {
        (Some(p), Some(r), _, _) => format!("{p} of {} ({:.1}%)", s.n, r * 100.0),
        (_, _, Some(med), Some(mean)) if e.method == Some(Method::VanElteren) => {
            format!("n {}, median {med}, mean {mean:.2}", s.n)
        }
        (_, _, _, Some(mean)) => format!("n {}, mean {mean:.2}", s.n),
        _ => format!("n {}", s.n),
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
            "- Strata (sessions before → after): {}; {} session(s) in strata without both sides left out",
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
    candidates: &[Candidate],
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
        let (kind, t, act) = match (e.verdict, l.meta.status) {
            (Verdict::Improved, Status::Adopted) => (
                "lesson-verifiable",
                format!("Verify lesson {id}: its metric improved"),
                format!(
                    "Its metric is lower since adoption. Read the numbers, then mark it verified:\n\n\
                     ```sh\nlab lesson set {id} verified --note \"lab evaluate: {note}\"\n```\n"
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
            (Verdict::Unmeasurable, _) => (
                "lesson-unmeasurable",
                format!("Choose a measurable metric for lesson {id}"),
                format!(
                    "This metric cannot show whether the lesson helps. Choose a metric that can (edit `metric:` in `{file}`; \
                     a finding rule as `tool: <tool>, rule: <rule>`, or a numeric session field as `rule: session.<field>`), \
                     or decide on other evidence:\n\n\
                     ```sh\nlab lesson set {id} verified --force --note \"<the evidence>\"\nlab lesson set {id} retired --note \"<why>\"\n```\n"
                ),
            ),
            _ => continue,
        };
        let detail = match e.verdict {
            Verdict::Unmeasurable => format!("unmeasurable: {}", e.reason.as_deref().unwrap_or("")),
            v => v.as_str().to_string(),
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
    if !candidates.is_empty() {
        let mut body = format!(
            "{} recurring finding rule(s) at warn or error have no lesson measuring them:\n\n",
            candidates.len()
        );
        for c in candidates {
            let _ = writeln!(
                body,
                "- {} `{}:{}`: {} distinct finding(s); e.g. {}",
                c.severity,
                c.tool,
                c.rule,
                c.findings,
                c.sample.join(", ")
            );
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
    /// (`reopened`), when its detail changed and it was last sent at least
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
                e.pending = Some(Pending::Reopened);
                e.detail_hash = h;
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
        assert_eq!(st.plan(&mut s, t(27), day), [true], "reappeared");
        assert!(s[0].reopened && !s[0].repeat);
        assert_eq!(s[0].reference.as_deref(), Some("board:42"));
        assert_eq!(s[0].first_seen, t(0).to_string());
        assert_eq!(st.signals["a"].resolved, None);
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
        st.plan(&mut s, t(3), day);
        st.failed("a", "timeout".into());
        let mut s = vec![sig("a", "one")];
        assert_eq!(st.plan(&mut s, t(4), day), [true]);
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
