//! `rf.finding/1` records (see docs/interfaces.md) and their fingerprints.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "rf.finding/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
}

/// Whether `schema` is `<prefix>1` or a `<prefix>1.x` minor version.
pub fn version_1(schema: &str, prefix: &str) -> bool {
    schema
        .strip_prefix(prefix)
        .and_then(|v| v.split('.').next())
        == Some("1")
}

/// Unknown values are `null` (docs/interfaces.md); read them as the empty value.
fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub schema: String,
    pub tool: String,
    pub rule: String,
    pub severity: Severity,
    /// Empty when the checker reported it as unknown (`null`).
    #[serde(deserialize_with = "null_as_default")]
    pub subject: String,
    /// Empty when the checker reported it as unknown (`null`).
    #[serde(deserialize_with = "null_as_default")]
    pub message: String,
    /// Always written, `[]` when there is none, so readers of stored runs
    /// find the key in every record.
    #[serde(default, deserialize_with = "null_as_default")]
    pub evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

impl Finding {
    /// Parses one JSON line, rejecting other schemas and other major versions.
    pub fn parse(line: &str) -> Result<Self, String> {
        Self::checked(serde_json::from_str(line).map_err(|e| e.to_string())?)
    }

    /// Reads an already parsed JSON value, as `parse` does.
    pub fn from_value(value: serde_json::Value) -> Result<Self, String> {
        Self::checked(serde_json::from_value(value).map_err(|e| e.to_string())?)
    }

    fn checked(finding: Self) -> Result<Self, String> {
        if !version_1(&finding.schema, "rf.finding/") {
            return Err(format!("unsupported schema `{}`", finding.schema));
        }
        Ok(finding)
    }

    /// A finding created by `lab` itself.
    pub fn lab(rule: &str, severity: Severity, subject: &str, message: String) -> Self {
        Self {
            schema: SCHEMA.into(),
            tool: "lab".into(),
            rule: rule.into(),
            severity,
            subject: subject.into(),
            message,
            evidence: Vec::new(),
            data: None,
            at: Some(now()),
        }
    }

    /// Hash of the parts that identify a problem: tool, rule, severity, the
    /// subject without its line number, and the message.
    fn identity(&self) -> String {
        let mut h = Sha256::new();
        for part in [
            self.tool.as_str(),
            self.rule.as_str(),
            self.severity.as_str(),
            without_line(&self.subject),
            self.message.as_str(),
        ] {
            h.update(part.as_bytes());
            h.update([0]);
        }
        h.finalize()[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// Fingerprints identifying each finding across runs, in input order.
///
/// A fingerprint covers the tool, rule, severity, the subject without its line
/// number, and the message, so a finding keeps its fingerprint when lines are
/// added above it, and becomes new when its severity or message changes.
/// Evidence, data and time are left out. Identical findings in one input are
/// numbered in order (`<hash>`, `<hash>-2`, ...), so fixing one of several
/// identical problems still counts as one resolved finding.
pub fn fingerprints<'a>(findings: impl IntoIterator<Item = &'a Finding>) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    findings
        .into_iter()
        .map(|f| {
            let id = f.identity();
            let n = seen.entry(id.clone()).or_default();
            *n += 1;
            if *n == 1 { id } else { format!("{id}-{n}") }
        })
        .collect()
}

/// `docs/a.md:42` → `docs/a.md`. Only a subject whose part before the colon
/// looks like a path (it contains `/` or `.`) loses a trailing `:line`,
/// `:line:column` or `:start-end`, so `claude:<id>` or `ref:177` stay whole.
fn without_line(subject: &str) -> &str {
    let mut s = subject;
    for _ in 0..2 {
        let Some((head, tail)) = s.rsplit_once(':') else {
            break;
        };
        let numeric = !tail.is_empty()
            && tail.bytes().all(|b| b.is_ascii_digit() || b == b'-')
            && tail.bytes().next().is_some_and(|b| b.is_ascii_digit());
        if !numeric || !(head.contains('/') || head.contains('.')) {
            break;
        }
        s = head;
    }
    s
}

pub fn now() -> String {
    let t = jiff::Timestamp::now();
    t.round(jiff::Unit::Second).unwrap_or(t).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"schema":"rf.finding/1","tool":"claimcheck","rule":"quote-not-found","severity":"error","subject":"docs/a.md:4","message":"m","evidence":[{"path":"t.txt","locator":"l"}],"at":"2026-11-07T00:00:00Z","extra":1}"#;

    fn fp(f: &Finding) -> String {
        fingerprints([f]).remove(0)
    }

    #[test]
    fn parses_and_ignores_unknown_fields() {
        let f = Finding::parse(LINE).unwrap();
        assert_eq!(f.severity, Severity::Error);
        assert_eq!(f.evidence[0].locator.as_deref(), Some("l"));
    }

    #[test]
    fn null_values_are_unknown_not_invalid() {
        let line = r#"{"schema":"rf.finding/1","tool":"t","rule":"r","severity":"error","subject":null,"message":null,"evidence":null,"data":null,"at":null}"#;
        let f = Finding::parse(line).unwrap();
        assert!(f.subject.is_empty() && f.message.is_empty() && f.evidence.is_empty());
    }

    #[test]
    fn evidence_is_written_even_when_empty() {
        let line = r#"{"schema":"rf.finding/1","tool":"t","rule":"r","severity":"warn","subject":"s","message":"m"}"#;
        let f = Finding::parse(line).unwrap();
        let out: serde_json::Value = serde_json::to_value(&f).unwrap();
        assert_eq!(out["evidence"], serde_json::json!([]));
    }

    #[test]
    fn rejects_other_schemas() {
        assert!(Finding::parse(&LINE.replace("rf.finding/1", "rf.finding/2")).is_err());
        assert!(Finding::parse(&LINE.replace("rf.finding/1", "rf.session/1")).is_err());
        assert!(Finding::parse(&LINE.replace("rf.finding/1", "rf.finding/1.3")).is_ok());
        assert!(Finding::parse(&LINE.replace("\"error\"", "\"Error\"")).is_err());
    }

    #[test]
    fn fingerprint_ignores_time_evidence_and_line_but_not_severity() {
        let a = Finding::parse(LINE).unwrap();
        let mut b = a.clone();
        b.at = None;
        b.evidence.clear();
        b.subject = "docs/a.md:17".into();
        assert_eq!(fp(&a), fp(&b), "a line shift keeps the fingerprint");
        b.severity = Severity::Warn;
        assert_ne!(fp(&a), fp(&b), "a severity change is a new finding");
        let mut c = a.clone();
        c.message = "other".into();
        assert_ne!(fp(&a), fp(&c));
        let mut d = a.clone();
        d.subject = "docs/b.md:4".into();
        assert_ne!(fp(&a), fp(&d));
    }

    #[test]
    fn identical_findings_are_numbered() {
        let a = Finding::parse(LINE).unwrap();
        let mut b = a.clone();
        b.subject = "docs/a.md:9".into();
        let fps = fingerprints([&a, &b]);
        assert_eq!(fps[1], format!("{}-2", fps[0]));
    }

    #[test]
    fn only_path_subjects_lose_their_line() {
        assert_eq!(without_line("docs/a.md:42"), "docs/a.md");
        assert_eq!(without_line("a.md:42:7"), "a.md");
        assert_eq!(without_line("a.md:40-45"), "a.md");
        assert_eq!(without_line("ref:177"), "ref:177");
        assert_eq!(without_line("t3:12345"), "t3:12345");
        assert_eq!(without_line("claude:0221dfc4-e1df"), "claude:0221dfc4-e1df");
        assert_eq!(without_line("docs/a.md"), "docs/a.md");
    }
}
