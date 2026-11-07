//! The `rf.archive-seal/1` manifest: the length and hash of each append-only
//! file's body. A body that still starts with the sealed bytes was appended
//! to; any other change is an edit. Front matter is outside the body, so a
//! record's status can change without breaking its seal.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "rf.archive-seal/1";

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: String,
    pub files: BTreeMap<String, Entry>,
    /// Every accepted edit, oldest first.
    #[serde(default)]
    pub history: Vec<Change>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Change {
    pub at: String,
    pub path: String,
    pub old_sha256: String,
    pub new_sha256: String,
    pub note: String,
}

/// How a sealed file's body compares with its manifest entry.
#[derive(Debug, PartialEq)]
pub enum State {
    /// Not in the manifest yet.
    New,
    Unchanged,
    /// The body grew and still starts with the sealed bytes.
    Appended,
    /// The body no longer starts with the sealed bytes.
    Edited,
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Entry {
    pub fn of(body: &[u8]) -> Self {
        Self {
            bytes: body.len() as u64,
            sha256: sha256(body),
        }
    }

    pub fn state(entry: Option<&Self>, body: &[u8]) -> State {
        let Some(e) = entry else {
            return State::New;
        };
        let sealed = usize::try_from(e.bytes).ok().and_then(|n| body.get(..n));
        match sealed {
            Some(prefix) if sha256(prefix) == e.sha256 => {
                if prefix.len() == body.len() {
                    State::Unchanged
                } else {
                    State::Appended
                }
            }
            _ => State::Edited,
        }
    }
}

impl Manifest {
    /// The manifest at `path`; empty when the file does not exist.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    schema: SCHEMA.into(),
                    files: BTreeMap::new(),
                    history: Vec::new(),
                });
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let m: Self =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if m.schema != SCHEMA {
            return Err(format!("{}: not a {SCHEMA} record", path.display()));
        }
        Ok(m)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let body = serde_json::to_string_pretty(self).map_err(|e| e.to_string())? + "\n";
        super::write_atomically(path, body.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::front;

    fn body(text: &str) -> &[u8] {
        front::split(text.as_bytes()).body
    }

    #[test]
    fn appends_pass_and_edits_are_caught() {
        let sealed = Entry::of(body("# Log\n\n- one\n"));
        let state = |t: &str| Entry::state(Some(&sealed), body(t));
        assert_eq!(state("# Log\n\n- one\n"), State::Unchanged);
        assert_eq!(state("# Log\n\n- one\n- two\n"), State::Appended);
        assert_eq!(state("# Log\n\n- One\n- two\n"), State::Edited);
        assert_eq!(state("# Log\n"), State::Edited, "a shorter body was cut");
        assert_eq!(Entry::state(None, b"x"), State::New);
    }

    #[test]
    fn front_matter_changes_keep_the_seal() {
        let sealed = Entry::of(body("---\nstatus: current\n---\n# H1\n"));
        let now = body("---\nstatus: superseded\nsuperseded_by: H2\n---\n# H1\n");
        assert_eq!(Entry::state(Some(&sealed), now), State::Unchanged);
        let added = body("---\nstatus: historical\n---\n# H1\n");
        assert_eq!(
            Entry::state(Some(&Entry::of(body("# H1\n"))), added),
            State::Unchanged,
            "adding front matter later leaves the body alone"
        );
    }

    #[test]
    fn manifest_round_trips_with_sorted_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seal.json");
        let mut m = Manifest::load(&path).unwrap();
        m.files.insert("b.md".into(), Entry::of(b"b"));
        m.files.insert("a.md".into(), Entry::of(b"a"));
        m.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("}\n"));
        assert!(text.find("\"a.md\"").unwrap() < text.find("\"b.md\"").unwrap());
        assert_eq!(Manifest::load(&path).unwrap().files.len(), 2);
    }
}
