//! The lessons ledger: one Markdown file per lesson with YAML frontmatter.
//!
//! A lesson records something that went well (`good`) or badly (`bad`), the
//! practice that follows from it, the evidence, and optionally the finding rule
//! whose frequency shows whether the practice works. Lessons move through
//! `observed → proposed → adopted → verified`, or to `retired`; every change is
//! appended to the file's History section.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A practice that worked and should be kept.
    Good,
    /// A failure or waste to avoid.
    Bad,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Seen at least once; not yet assessed.
    Observed,
    /// Assessed and worth adopting; not yet in the workflow.
    Proposed,
    /// In the workflow; `lab brief` shows its rule to every session.
    Adopted,
    /// Adopted, and later evidence showed it helps.
    Verified,
    /// No longer applies, or did not help.
    Retired,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Proposed => "proposed",
            Self::Adopted => "adopted",
            Self::Verified => "verified",
            Self::Retired => "retired",
        }
    }

    /// Whether the rule is part of the current workflow.
    pub fn active(self) -> bool {
        matches!(self, Self::Adopted | Self::Verified)
    }
}

/// The finding rule whose frequency measures a lesson.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metric {
    pub tool: String,
    pub rule: String,
}

impl std::str::FromStr for Metric {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.split_once(':') {
            Some((tool, rule)) if !tool.is_empty() && !rule.is_empty() => Ok(Self {
                tool: tool.into(),
                rule: rule.into(),
            }),
            _ => Err(format!("metric must be `tool:rule`, got `{s}`")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub kind: Kind,
    pub status: Status,
    /// What the lesson changes: workflow, tool, instruction, experiment, writing, ...
    pub scope: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The practice, as one imperative sentence an agent can follow.
    pub rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<Metric>,
    pub created: String,
    pub updated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adopted: Option<String>,
    /// Check-run lessons: the file name of the stored check run whose
    /// introduced findings were reviewed; `lab evaluate` counts introduced
    /// findings from that run on. Cleared when the lesson is adopted again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed: Option<String>,
    /// Pointers: `path:line`, session ids, finding fingerprints, URLs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Lesson {
    pub path: PathBuf,
    pub meta: Meta,
    pub body: String,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn valid_date(s: &str) -> bool {
    s.len() == 10 && s.parse::<jiff::civil::Date>().is_ok()
}

impl Lesson {
    pub fn parse(path: &Path, text: &str) -> Result<Self, String> {
        let where_ = path.display();
        let rest = text
            .strip_prefix("---\n")
            .ok_or_else(|| format!("{where_}: missing YAML frontmatter"))?;
        let end = rest
            .find("\n---\n")
            .ok_or_else(|| format!("{where_}: unterminated frontmatter"))?;
        let meta: Meta =
            yaml_serde::from_str(&rest[..end]).map_err(|e| format!("{where_}: {e}"))?;
        let body = rest[end + 5..].to_string();
        let lesson = Self {
            path: path.to_path_buf(),
            meta,
            body,
        };
        lesson.validate()?;
        Ok(lesson)
    }

    fn validate(&self) -> Result<(), String> {
        let m = &self.meta;
        let where_ = self.path.display();
        let stem = self.path.file_stem().map(|s| s.to_string_lossy());
        if !valid_id(&m.id) {
            return Err(format!(
                "{where_}: id `{}` must be lowercase-hyphenated",
                m.id
            ));
        }
        if stem.as_deref() != Some(m.id.as_str()) {
            return Err(format!("{where_}: id `{}` must match the file name", m.id));
        }
        for (field, value) in [("title", &m.title), ("rule", &m.rule), ("scope", &m.scope)] {
            if value.trim().is_empty() {
                return Err(format!("{where_}: `{field}` must not be empty"));
            }
        }
        for (field, value) in [
            ("created", Some(&m.created)),
            ("updated", Some(&m.updated)),
            ("adopted", m.adopted.as_ref()),
        ] {
            if let Some(v) = value
                && !valid_date(v)
            {
                return Err(format!("{where_}: `{field}` must be YYYY-MM-DD, got `{v}`"));
            }
        }
        if let Some(r) = &m.reviewed
            && (!r.ends_with(".jsonl") || r.contains(['/', '\\']))
        {
            return Err(format!(
                "{where_}: `reviewed` must be a check run's file name, got `{r}`"
            ));
        }
        if m.status.active() && m.adopted.is_none() {
            return Err(format!(
                "{where_}: an {} lesson needs `adopted`",
                m.status.as_str()
            ));
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, String> {
        let yaml = yaml_serde::to_string(&self.meta).map_err(|e| e.to_string())?;
        Ok(format!("---\n{yaml}---\n{}", self.body))
    }

    /// Changes the status, stamps the dates, and appends a History line.
    pub fn transition(&mut self, to: Status, note: &str, date: &str) -> Result<(), String> {
        if note.trim().is_empty() {
            return Err("a status change needs a note saying why".into());
        }
        let from = self.meta.status;
        if from == to {
            return Err(format!("`{}` is already {}", self.meta.id, to.as_str()));
        }
        if to == Status::Verified && !from.active() {
            return Err("only an adopted lesson can be verified".into());
        }
        // Status moves forward. Two moves back are allowed: a retired lesson
        // reopens at any status, and a verified lesson returns to adopted when
        // later evidence no longer shows it helps. Any other way back goes
        // through `retired`, so the history says the lesson was given up.
        let back = to < from
            && from != Status::Retired
            && !(from == Status::Verified && to == Status::Adopted);
        if back {
            return Err(format!(
                "`{}` cannot move back from {} to {}; retire it, then reopen it at {}",
                self.meta.id,
                from.as_str(),
                to.as_str(),
                to.as_str()
            ));
        }
        self.meta.status = to;
        self.meta.updated = date.into();
        // Adoption starts the period `lab evaluate` measures, so a lesson that
        // is adopted again after being retired gets the new date. Moving back
        // from verified to adopted keeps it.
        if to == Status::Adopted && !from.active() {
            self.meta.adopted = Some(date.into());
            self.meta.reviewed = None;
        }
        self.append_history(&format!(
            "{date}: {} → {}. {}",
            from.as_str(),
            to.as_str(),
            note.trim()
        ));
        self.validate()
    }

    /// Whether this lesson can take a review with this note.
    pub fn reviewable(&self, note: &str) -> Result<(), String> {
        if note.trim().is_empty() {
            return Err("a review needs a note saying what was found".into());
        }
        if !self.meta.status.active() {
            return Err(format!(
                "`{}` is {}; only an adopted or verified lesson is reviewed",
                self.meta.id,
                self.meta.status.as_str()
            ));
        }
        Ok(())
    }

    /// Records that the introduced findings up to the stored check run named
    /// `run` were reviewed, and appends a History line.
    pub fn review(
        &mut self,
        run: &str,
        summary: &str,
        note: &str,
        date: &str,
    ) -> Result<(), String> {
        self.reviewable(note)?;
        self.meta.reviewed = Some(run.into());
        self.meta.updated = date.into();
        self.append_history(&format!("{date}: reviewed {summary}. {}", note.trim()));
        self.validate()
    }

    fn append_history(&mut self, line: &str) {
        if !self.body.contains("\n## History\n") && !self.body.starts_with("## History\n") {
            if !self.body.ends_with('\n') && !self.body.is_empty() {
                self.body.push('\n');
            }
            self.body.push_str("\n## History\n\n");
        } else if !self.body.ends_with('\n') {
            self.body.push('\n');
        }
        let _ = writeln!(self.body, "- {line}");
    }
}

/// Loads every `*.md` lesson except `README.md`. A missing directory is empty.
pub fn load(dir: &Path) -> Result<Vec<Lesson>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let p = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if p.extension().is_some_and(|x| x == "md")
            && p.file_name().is_some_and(|n| n != "README.md")
        {
            paths.push(p);
        }
    }
    paths.sort();
    let mut lessons = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        match std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|text| Lesson::parse(&path, &text))
        {
            Ok(lesson) => lessons.push(lesson),
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() {
        Ok(lessons)
    } else {
        Err(errors.join("\n"))
    }
}

/// Writes a new lesson file; never overwrites.
pub fn create(dir: &Path, meta: Meta, body: &str) -> Result<PathBuf, String> {
    if !valid_id(&meta.id) {
        return Err(format!("id `{}` must be lowercase-hyphenated", meta.id));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.md", meta.id));
    let mut lesson = Lesson {
        path: path.clone(),
        meta,
        body: if body.trim().is_empty() {
            "\nWhat happened, why it matters, and how the rule follows from the evidence.\n".into()
        } else {
            format!("\n{}\n", body.trim())
        },
    };
    let created = lesson.meta.created.clone();
    let status = lesson.meta.status;
    lesson.append_history(&format!("{created}: recorded as {}.", status.as_str()));
    lesson.validate()?;
    write_new(&path, &lesson.render()?)?;
    Ok(path)
}

fn write_new(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                format!("{} exists; refusing to overwrite", path.display())
            }
            _ => format!("{}: {e}", path.display()),
        })?;
    file.write_all(text.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Rewrites an existing lesson through a temporary file and rename.
pub fn save(lesson: &Lesson) -> Result<(), String> {
    let tmp = lesson.path.with_extension("md.tmp");
    std::fs::write(&tmp, lesson.render()?).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &lesson.path).map_err(|e| format!("{}: {e}", lesson.path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str) -> Meta {
        Meta {
            id: id.into(),
            title: "Dedupe before adding a source".into(),
            kind: Kind::Bad,
            status: Status::Observed,
            scope: "workflow".into(),
            tags: vec!["literature".into()],
            rule: "Run `papers seen` before allocating a catalogue number.".into(),
            metric: Some("papers:registry-duplicate".parse().unwrap()),
            created: "2026-11-07".into(),
            updated: "2026-11-07".into(),
            adopted: None,
            reviewed: None,
            evidence: vec!["references.md:3013".into()],
        }
    }

    #[test]
    fn create_load_and_transition_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = create(
            dir.path(),
            meta("dedupe-first"),
            "Two keys named one paper.",
        )
        .unwrap();
        assert!(
            create(dir.path(), meta("dedupe-first"), "").is_err(),
            "never overwrites"
        );
        let mut lessons = load(dir.path()).unwrap();
        assert_eq!(lessons.len(), 1);
        let lesson = &mut lessons[0];
        assert_eq!(lesson.meta, meta("dedupe-first"));
        assert!(
            lesson
                .transition(Status::Verified, "x", "2026-11-08")
                .is_err()
        );
        lesson
            .transition(Status::Adopted, "agreed in review", "2026-11-08")
            .unwrap();
        assert_eq!(lesson.meta.adopted.as_deref(), Some("2026-11-08"));
        save(lesson).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("- 2026-11-07: recorded as observed."));
        assert!(text.contains("- 2026-11-08: observed → adopted. agreed in review"));
        assert_eq!(load(dir.path()).unwrap()[0].meta.status, Status::Adopted);
    }

    #[test]
    fn status_moves_back_only_from_verified_or_retired() {
        let mut l = Lesson {
            path: PathBuf::from("x.md"),
            meta: meta("x"),
            body: String::new(),
        };
        l.transition(Status::Proposed, "a", "2026-11-01").unwrap();
        let err = l
            .transition(Status::Observed, "b", "2026-11-02")
            .unwrap_err();
        assert!(
            err.contains("cannot move back from proposed to observed"),
            "{err}"
        );
        l.transition(Status::Adopted, "c", "2026-11-03").unwrap();
        for to in [Status::Observed, Status::Proposed] {
            assert!(l.transition(to, "d", "2026-11-04").is_err());
        }
        l.transition(Status::Verified, "e", "2026-11-05").unwrap();
        assert!(l.transition(Status::Proposed, "f", "2026-11-06").is_err());
        // Later evidence no longer shows it helps: back to adopted, same date.
        l.transition(Status::Adopted, "g", "2026-11-07").unwrap();
        assert_eq!(l.meta.adopted.as_deref(), Some("2026-11-03"));
        // Given up, then reopened at the start.
        l.transition(Status::Retired, "h", "2026-11-08").unwrap();
        l.transition(Status::Observed, "i", "2026-11-09").unwrap();
        assert_eq!(l.meta.status, Status::Observed);
    }

    #[test]
    fn a_review_is_recorded_and_cleared_by_readoption() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = meta("checked");
        m.status = Status::Adopted;
        m.adopted = Some("2026-11-01".into());
        let path = create(dir.path(), m, "").unwrap();
        let mut l = load(dir.path()).unwrap().remove(0);
        assert!(
            l.review(
                "20261105T000000.000Z.jsonl",
                "2 finding(s)",
                " ",
                "2026-11-06"
            )
            .is_err()
        );
        l.review(
            "20261105T000000.000Z.jsonl",
            "2 finding(s)",
            "both in sealed lines",
            "2026-11-06",
        )
        .unwrap();
        save(&l).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("reviewed: 20261105T000000.000Z.jsonl\n"),
            "{text}"
        );
        assert!(
            text.contains("- 2026-11-06: reviewed 2 finding(s). both in sealed lines\n"),
            "{text}"
        );
        let mut l = load(dir.path()).unwrap().remove(0);
        assert_eq!(
            l.meta.reviewed.as_deref(),
            Some("20261105T000000.000Z.jsonl")
        );
        l.transition(Status::Retired, "superseded", "2026-11-07")
            .unwrap();
        assert!(
            l.review("20261107T000000.000Z.jsonl", "1", "x", "2026-11-07")
                .is_err()
        );
        l.transition(Status::Adopted, "again", "2026-11-08")
            .unwrap();
        assert_eq!(l.meta.reviewed, None);
        let bad = text.replace(
            "reviewed: 20261105T000000.000Z.jsonl",
            "reviewed: ../x.jsonl",
        );
        assert!(
            Lesson::parse(&path, &bad)
                .unwrap_err()
                .contains("`reviewed` must be a check run's file name")
        );
    }

    #[test]
    fn readoption_restarts_the_measured_period() {
        let dir = tempfile::tempdir().unwrap();
        create(dir.path(), meta("again"), "").unwrap();
        let lesson = &mut load(dir.path()).unwrap()[0];
        lesson
            .transition(Status::Adopted, "a", "2026-01-01")
            .unwrap();
        lesson
            .transition(Status::Retired, "b", "2026-02-01")
            .unwrap();
        lesson
            .transition(Status::Adopted, "c", "2026-06-01")
            .unwrap();
        assert_eq!(lesson.meta.adopted.as_deref(), Some("2026-06-01"));
    }

    #[test]
    fn validation_errors_name_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrong-name.md");
        let mut lesson = Lesson {
            path: path.clone(),
            meta: meta("other"),
            body: String::new(),
        };
        std::fs::write(&path, lesson.render().unwrap()).unwrap();
        let err = load(dir.path()).unwrap_err();
        assert!(
            err.contains("wrong-name.md") && err.contains("match the file name"),
            "{err}"
        );
        lesson.meta.id = "wrong-name".into();
        lesson.meta.status = Status::Adopted;
        std::fs::write(&path, lesson.render().unwrap()).unwrap();
        assert!(load(dir.path()).unwrap_err().contains("needs `adopted`"));
    }

    #[test]
    fn ids_and_metrics_are_checked() {
        assert!(valid_id("a-b-2"));
        assert!(!valid_id("A-b") && !valid_id("a--b") && !valid_id("-a") && !valid_id(""));
        assert!("papers".parse::<Metric>().is_err());
        assert!("a:b".parse::<Metric>().is_ok());
    }
}
