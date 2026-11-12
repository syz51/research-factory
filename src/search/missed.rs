//! Missed queries (docs/state-machines.md 3.4), read from the query trace.
//! A `lab search` query is missed when it found nothing (`zero`), when no
//! hit held all its terms (`partial-hit`), or when the same session asked
//! again in other words soon after (`reformulated`). Its target is the
//! record the session read next: the first opened hit, else the best hit of
//! the next query that found a hit with every term.

use std::collections::BTreeMap;

use jiff::Timestamp;
use sha2::{Digest, Sha256};

use super::query::words;
use super::trace::{Line, QueryLine};

/// How long after a query a session's next steps still belong to it.
const WINDOW_SECS: i64 = 600;
/// A shared word of at least this many characters makes a later query a
/// reformulation.
const SHARED_WORD: usize = 3;

/// One distinct missed query, with what the trace says about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Missed {
    /// 8 hex digits of the query and its target: stable across runs.
    pub id: String,
    /// The first phrasing, lowercased word by word.
    pub query: String,
    /// How often it was missed.
    pub count: usize,
    /// `zero`, `partial-hit`, `reformulated`, each once.
    pub reasons: Vec<&'static str>,
    /// The path of the record the session read next, if any.
    pub target: Option<String>,
    /// The time of the last miss.
    pub last: String,
}

/// The normal form of a query: its first phrasing's words.
pub fn normal(q: &QueryLine) -> String {
    q.query
        .first()
        .map(|p| words(p).join(" "))
        .unwrap_or_default()
}

/// The id of a proposal: 8 hex digits of the query alone, so it stays the
/// same when later misses change the target.
pub fn proposal_id(query: &str) -> String {
    let digest = Sha256::digest(query);
    digest[..4].iter().map(|b| format!("{b:02x}")).collect()
}

fn path_of(anchor: &str) -> &str {
    anchor.split_once('#').map_or(anchor, |(path, _)| path)
}

fn time(at: &str) -> Option<Timestamp> {
    at.parse().ok()
}

fn shares_word(a: &str, b: &str) -> bool {
    let long = |s: &str| -> Vec<String> {
        words(s)
            .into_iter()
            .filter(|w| w.chars().count() >= SHARED_WORD)
            .collect()
    };
    let b = long(b);
    long(a).iter().any(|w| b.contains(w))
}

/// The missed queries of the trace, most frequent first. Only queries of
/// `lab search` count; a topics run and the duplicate check do not.
/// `since` keeps the queries last missed on or after that time; their
/// counts and targets come from the whole trace.
pub fn missed(lines: &[Line], since: Option<Timestamp>) -> Vec<Missed> {
    let mut by_query: BTreeMap<String, Missed> = BTreeMap::new();
    let mut targets: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    for (i, line) in lines.iter().enumerate() {
        let Line::Query(q) = line else { continue };
        if q.source != "cli" || q.state == "error" {
            continue;
        }
        let Some(at) = time(&q.at) else { continue };
        let query = normal(q);
        if query.is_empty() {
            continue;
        }
        // The later lines of the same session in the window. Lines without
        // a session cannot be told apart, so nothing follows them.
        let later: Vec<&Line> = lines[i + 1..]
            .iter()
            .filter(|l| q.session.is_some() && l.session() == q.session.as_deref())
            .take_while(|l| {
                time(l.at()).is_some_and(|t| t.duration_since(at).as_secs() <= WINDOW_SECS)
            })
            .collect();
        let mut reasons = Vec::new();
        if q.state == "zero" {
            reasons.push("zero");
        } else if q.hit_count > 0 && q.complete == 0 {
            reasons.push("partial-hit");
        }
        let next_query = later.iter().find_map(|l| match l {
            Line::Query(n) if n.source == "cli" => Some(n),
            _ => None,
        });
        if let Some(n) = next_query {
            let other = normal(n);
            if other != query && shares_word(&query, &other) {
                reasons.push("reformulated");
            }
        }
        if reasons.is_empty() {
            continue;
        }
        let target = later.iter().find_map(|l| match l {
            Line::Open(o) => Some(path_of(&o.anchor).to_string()),
            Line::Query(n) if n.source == "cli" && n.complete > 0 => {
                n.hits.first().map(|h| path_of(h).to_string())
            }
            Line::Query(_) => None,
        });
        targets.entry(query.clone()).or_default().push(target);
        let entry = by_query.entry(query.clone()).or_insert_with(|| Missed {
            id: String::new(),
            query: query.clone(),
            count: 0,
            reasons: Vec::new(),
            target: None,
            last: q.at.clone(),
        });
        entry.count += 1;
        entry.last.clone_from(&q.at);
        for r in reasons {
            if !entry.reasons.contains(&r) {
                entry.reasons.push(r);
            }
        }
    }
    let mut out: Vec<Missed> = by_query
        .into_values()
        .map(|mut m| {
            // The target most misses led to; the latest on a tie.
            let mut counts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            for (i, t) in targets[&m.query].iter().enumerate() {
                if let Some(t) = t {
                    let c = counts.entry(t).or_default();
                    c.0 += 1;
                    c.1 = i;
                }
            }
            m.target = counts
                .into_iter()
                .max_by_key(|(_, c)| *c)
                .map(|(t, _)| t.to_string());
            m.id = proposal_id(&m.query);
            m
        })
        .filter(|m: &Missed| since.is_none_or(|s| time(&m.last).is_some_and(|t| t >= s)))
        .collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| b.last.cmp(&a.last))
            .then_with(|| a.query.cmp(&b.query))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::trace::{OpenLine, QUERY_SCHEMA};

    fn q(
        at: &str,
        session: &str,
        query: &str,
        state: &str,
        complete: usize,
        hits: &[&str],
    ) -> Line {
        Line::Query(Box::new(QueryLine {
            schema: QUERY_SCHEMA.into(),
            at: at.into(),
            session: Some(session.into()),
            source: "cli".into(),
            query: vec![query.into()],
            scopes: vec!["notes".into()],
            state: state.into(),
            stages: vec!["lexical".into()],
            missing_stages: Vec::new(),
            expansions: Vec::new(),
            candidates: BTreeMap::new(),
            hit_count: hits.len(),
            complete,
            hits: hits.iter().map(|h| h.to_string()).collect(),
            unknown: Vec::new(),
            notices: Vec::new(),
            error: None,
            latency_ms: 1,
        }))
    }

    #[test]
    fn a_zero_hit_query_followed_by_a_read_targets_the_read() {
        let lines = vec![
            q("2026-11-01T10:00:00Z", "s1", "FY shift", "zero", 0, &[]),
            Line::Open(OpenLine {
                schema: "rf.search-open/1".into(),
                at: "2026-11-01T10:01:00Z".into(),
                session: Some("s1".into()),
                anchor: "notes/a.md#L1-L5".into(),
            }),
            q("2026-11-02T10:00:00Z", "s2", "fy shift", "zero", 0, &[]),
            q(
                "2026-11-02T11:00:00Z",
                "s2",
                "other",
                "hits",
                1,
                &["x.md#L1-L2"],
            ),
        ];
        let missed = missed(&lines, None);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].query, "fy shift");
        assert_eq!(missed[0].count, 2);
        assert_eq!(missed[0].target.as_deref(), Some("notes/a.md"));
        assert_eq!(missed[0].id, proposal_id("fy shift"));
    }

    #[test]
    fn a_reformulated_query_targets_the_answer_of_the_next() {
        let lines = vec![
            q(
                "2026-11-01T10:00:00Z",
                "s1",
                "calendar lag",
                "hits",
                1,
                &["b.md#L1-L2"],
            ),
            q(
                "2026-11-01T10:02:00Z",
                "s1",
                "fiscal calendar",
                "hits",
                1,
                &["c.md#L3-L4"],
            ),
            // Another session and a later time do not count.
            q(
                "2026-11-01T10:03:00Z",
                "s2",
                "calendar",
                "hits",
                1,
                &["d.md#L1-L1"],
            ),
        ];
        let missed = missed(&lines, None);
        assert_eq!(missed.len(), 1, "{missed:?}");
        assert_eq!(missed[0].query, "calendar lag");
        assert_eq!(missed[0].reasons, ["reformulated"]);
        assert_eq!(missed[0].target.as_deref(), Some("c.md"));
    }

    #[test]
    fn a_partial_hit_is_missed_and_since_filters() {
        let lines = vec![
            q("2026-10-01T10:00:00Z", "s1", "old miss", "zero", 0, &[]),
            q(
                "2026-11-01T10:00:00Z",
                "s1",
                "alpha beta",
                "hits",
                0,
                &["a.md#L1-L2"],
            ),
        ];
        let since = "2026-10-15T00:00:00Z".parse().ok();
        let missed = missed(&lines, since);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].reasons, ["partial-hit"]);
        assert_eq!(missed[0].target, None);
    }

    #[test]
    fn queries_without_a_session_are_not_linked() {
        let mut lines = vec![
            q(
                "2026-11-01T10:00:00Z",
                "",
                "calendar lag",
                "hits",
                1,
                &["b.md#L1-L2"],
            ),
            q(
                "2026-11-01T10:02:00Z",
                "",
                "fiscal calendar",
                "hits",
                1,
                &["c.md#L3-L4"],
            ),
        ];
        for line in &mut lines {
            if let Line::Query(q) = line {
                q.session = None;
            }
        }
        assert!(missed(&lines, None).is_empty());
    }

    #[test]
    fn since_keeps_the_target_of_the_whole_trace() {
        let lines = vec![
            q("2026-10-01T10:00:00Z", "s1", "fy", "zero", 0, &[]),
            q(
                "2026-10-01T10:01:00Z",
                "s1",
                "fiscal year",
                "hits",
                1,
                &["a.md#L1-L2"],
            ),
            q("2026-10-02T10:00:00Z", "s2", "fy", "zero", 0, &[]),
            q(
                "2026-10-02T10:01:00Z",
                "s2",
                "fiscal year",
                "hits",
                1,
                &["a.md#L1-L2"],
            ),
            q("2026-11-01T10:00:00Z", "s3", "fy", "zero", 0, &[]),
            q(
                "2026-11-01T10:01:00Z",
                "s3",
                "fiscal year",
                "hits",
                1,
                &["b.md#L1-L2"],
            ),
        ];
        let all = missed(&lines, None);
        let recent = missed(&lines, "2026-10-15T00:00:00Z".parse().ok());
        assert_eq!(recent, all);
        assert_eq!(recent[0].target.as_deref(), Some("a.md"));
        assert_eq!(recent[0].count, 3);
    }
}
