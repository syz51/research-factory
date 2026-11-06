//! `rf.finding/1` records (see docs/interfaces.md) and their fingerprints.

use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub schema: String,
    pub tool: String,
    pub rule: String,
    pub severity: Severity,
    pub subject: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

impl Finding {
    /// Parses one JSON line, rejecting other schemas and other major versions.
    pub fn parse(line: &str) -> Result<Self, String> {
        let finding: Self = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let major = finding
            .schema
            .strip_prefix("rf.finding/")
            .map(|v| v.split('.').next());
        if major != Some(Some("1")) {
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

    /// Identity of a finding across runs: tool, rule, subject and message.
    /// Evidence, data and time are excluded so that a re-run of an unchanged
    /// problem keeps its fingerprint; editing the subject's line or the message
    /// makes it a new finding.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        for part in [&self.tool, &self.rule, &self.subject, &self.message] {
            h.update(part.as_bytes());
            h.update([0]);
        }
        h.finalize()[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

pub fn now() -> String {
    let t = jiff::Timestamp::now();
    t.round(jiff::Unit::Second).unwrap_or(t).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"schema":"rf.finding/1","tool":"claimcheck","rule":"quote-not-found","severity":"error","subject":"docs/a.md:4","message":"m","evidence":[{"path":"t.txt","locator":"l"}],"at":"2026-11-07T00:00:00Z","extra":1}"#;

    #[test]
    fn parses_and_ignores_unknown_fields() {
        let f = Finding::parse(LINE).unwrap();
        assert_eq!(f.severity, Severity::Error);
        assert_eq!(f.evidence[0].locator.as_deref(), Some("l"));
    }

    #[test]
    fn rejects_other_schemas() {
        assert!(Finding::parse(&LINE.replace("rf.finding/1", "rf.finding/2")).is_err());
        assert!(Finding::parse(&LINE.replace("rf.finding/1", "rf.session/1")).is_err());
        assert!(Finding::parse(&LINE.replace("rf.finding/1", "rf.finding/1.3")).is_ok());
    }

    #[test]
    fn fingerprint_ignores_time_and_evidence() {
        let a = Finding::parse(LINE).unwrap();
        let mut b = a.clone();
        b.at = None;
        b.evidence.clear();
        assert_eq!(a.fingerprint(), b.fingerprint());
        b.message = "other".into();
        assert_ne!(a.fingerprint(), b.fingerprint());
    }
}
