//! `research.toml` discovery and the `[project]` and `[lab]` tables.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const CONFIG_FILE: &str = "research.toml";

/// A resolved project: its root, shared settings and the `[lab]` table.
#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub name: String,
    /// Zone in which stored UTC times become calendar dates.
    pub tz: jiff::tz::TimeZone,
    pub state_dir: PathBuf,
    pub lab: LabConfig,
}

/// The shared `[project]` table. Unknown keys are left to other tools.
#[derive(Debug, Default, Deserialize)]
struct ProjectTable {
    name: Option<String>,
    /// IANA time zone for calendar dates (lesson dates, `--since`); default: the system zone.
    timezone: Option<String>,
    state_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    project: ProjectTable,
    #[serde(default)]
    lab: LabConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabConfig {
    /// Directory of lesson files, relative to the project root.
    #[serde(default = "default_lessons_dir")]
    pub lessons_dir: String,
    /// Checks `lab check` runs, in order.
    #[serde(default)]
    pub checks: Vec<CheckSpec>,
    /// `rf.finding/1` JSON Lines file written by `agentprof scan`, for `lab retro`.
    #[serde(default)]
    pub agent_findings: Option<String>,
    /// `rf.session/1` JSON Lines file written by `agentprof scan`, for measuring
    /// lessons whose metric is an agent-session rule (default: `sessions.jsonl`
    /// next to `agent_findings`).
    #[serde(default)]
    pub agent_sessions: Option<String>,
    /// A finding rule with at least this many distinct findings (by fingerprint)
    /// and no lesson covering it becomes a lesson candidate.
    #[serde(default = "default_recurrence")]
    pub recurrence: usize,
    /// The research archive that `lab archive` indexes and checks.
    #[serde(default)]
    pub archive: Option<ArchiveConfig>,
    /// How `lab evaluate` decides whether a lesson helped.
    #[serde(default)]
    pub evaluate: EvaluateConfig,
    /// What `lab loop` runs and whom it tells.
    #[serde(default, rename = "loop")]
    pub lesson_loop: Option<LoopConfig>,
}

impl Default for LabConfig {
    fn default() -> Self {
        Self {
            lessons_dir: default_lessons_dir(),
            checks: Vec::new(),
            agent_findings: None,
            agent_sessions: None,
            recurrence: default_recurrence(),
            archive: None,
            evaluate: EvaluateConfig::default(),
            lesson_loop: None,
        }
    }
}

/// `[lab.evaluate]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluateConfig {
    /// Session groups needed on each side of the adoption date before a
    /// verdict; for check runs, the commits an `improved` verdict would need.
    #[serde(default = "default_min_sessions")]
    pub min_sessions: usize,
    /// Two-sided significance level; each direction is tested at `alpha / 2`.
    #[serde(default = "default_alpha")]
    pub alpha: f64,
    /// The before-window: observations within this many days before adoption
    /// (0: all of them).
    #[serde(default = "default_baseline_days")]
    pub baseline_days: u32,
}

impl Default for EvaluateConfig {
    fn default() -> Self {
        Self {
            min_sessions: default_min_sessions(),
            alpha: default_alpha(),
            baseline_days: default_baseline_days(),
        }
    }
}

fn default_min_sessions() -> usize {
    10
}

fn default_alpha() -> f64 {
    0.05
}

fn default_baseline_days() -> u32 {
    90
}

/// `[lab.loop]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopConfig {
    /// Commands (program and arguments) run in order from the project root
    /// before evaluating, such as `agentprof scan`.
    #[serde(default)]
    pub measure: Vec<Vec<String>>,
    /// Command that receives each new or changed `rf.signal/1` on stdin.
    #[serde(default)]
    pub notify: Option<Vec<String>>,
    /// A signal whose detail changed is sent again only once this many hours
    /// have passed since it was last sent.
    #[serde(default = "default_renotify_hours")]
    pub renotify_hours: u64,
}

fn default_renotify_hours() -> u64 {
    24
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            measure: Vec::new(),
            notify: None,
            renotify_hours: default_renotify_hours(),
        }
    }
}

/// Upper bounds that keep the date and duration arithmetic in range.
const MAX_BASELINE_DAYS: u32 = 36_500;
const MAX_RENOTIFY_HOURS: u64 = 87_600;

/// `[lab.archive]`. Paths and globs are relative to the project root.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveConfig {
    /// The entry page every session reads first.
    #[serde(default)]
    pub start: Option<String>,
    /// Where `lab archive seal` keeps the sealed files' lengths and hashes
    /// (default: `<state_dir>/lab/archive-seal.json`).
    #[serde(default)]
    pub seal_manifest: Option<String>,
    /// Append-only files.
    #[serde(default)]
    pub sealed: Vec<String>,
    /// Files whose relative links are checked.
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub links_exclude: Vec<String>,
    /// Pages that must not link inactive records as current.
    #[serde(default)]
    pub current_pages: Vec<String>,
    /// Allowed record statuses, in the order `group_by = "status"` shows them.
    #[serde(default = "default_statuses")]
    pub statuses: Vec<String>,
    /// Statuses of records that no longer apply.
    #[serde(default = "default_inactive_statuses")]
    pub inactive_statuses: Vec<String>,
    #[serde(default)]
    pub index: Vec<IndexSpec>,
    #[serde(default)]
    pub budget: Vec<BudgetSpec>,
    /// Lists of files moved out of the project, such as to cloud storage,
    /// that links may still point to.
    #[serde(default)]
    pub offloaded: Vec<OffloadedSpec>,
}

/// `[[lab.archive.offloaded]]`: files moved to `location`, listed in `list`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffloadedSpec {
    /// A plain-text file, relative to the project root, with one moved path
    /// per line, relative to the project root.
    pub list: String,
    /// Where the files are now, such as a `storagectl` location
    /// (`cloud:archive/air-2026-11-08/masters/`).
    pub location: String,
}

fn default_statuses() -> Vec<String> {
    [
        "current",
        "draft",
        "proposed",
        "accepted",
        "open",
        "resolved",
        "superseded",
        "rejected",
        "withdrawn",
        "historical",
    ]
    .map(String::from)
    .to_vec()
}

fn default_inactive_statuses() -> Vec<String> {
    ["superseded", "rejected", "withdrawn", "historical"]
        .map(String::from)
        .to_vec()
}

/// `[[lab.archive.index]]`: one generated index over a set of records.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexSpec {
    pub name: String,
    pub output: String,
    pub title: String,
    #[serde(default)]
    pub intro: String,
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Regex with one capture group, matched against the file name (the parent
    /// directory's name for `README.md`), that derives a record's id.
    #[serde(default)]
    pub id_from: Option<String>,
    #[serde(default)]
    pub id_prefix: String,
    #[serde(default)]
    pub trim_leading_zeros: bool,
    #[serde(default)]
    pub sort: SortOrder,
    #[serde(default)]
    pub group_by: Option<GroupBy>,
    #[serde(default)]
    pub require: Vec<Field>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortOrder {
    #[default]
    Id,
    IdDesc,
    Date,
    DateDesc,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupBy {
    Status,
}

/// A record field that an index can require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Id,
    Title,
    Date,
    Status,
    Summary,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Title => "title",
            Self::Date => "date",
            Self::Status => "status",
            Self::Summary => "summary",
        }
    }
}

/// `[[lab.archive.budget]]`: a size limit for the files matching `path`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetSpec {
    pub path: String,
    pub max_bytes: u64,
    #[serde(default = "default_budget_severity")]
    pub severity: crate::finding::Severity,
}

fn default_budget_severity() -> crate::finding::Severity {
    crate::finding::Severity::Warn
}

fn default_lessons_dir() -> String {
    "lessons".into()
}

fn default_recurrence() -> usize {
    3
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckSpec {
    /// Short name shown in reports and used by `lab check --only`.
    pub name: String,
    /// Program and arguments. The program must print `rf.finding/1` JSON Lines on stdout.
    pub run: Vec<String>,
    /// Working directory relative to the project root (default: the root).
    #[serde(default)]
    pub cwd: Option<String>,
    /// Program and arguments whose first stdout line is the checker's
    /// version, recorded in each run; `[]` records none. Default:
    /// `<program> --version` for a program named without a directory part
    /// that is not an interpreter such as `sh`.
    #[serde(default)]
    pub version: Option<Vec<String>>,
    /// Paths relative to the project root that the check reads but no commit
    /// holds, such as git-ignored source texts. `lab check --at` links them
    /// from the current work tree into a replayed commit and records them.
    #[serde(default)]
    pub uncommitted_inputs: Vec<String>,
}

impl Project {
    /// Finds `research.toml` by walking up from `start`, or loads `explicit`.
    /// Without either, the project root is `start` and every setting is a default.
    pub fn discover(start: &Path, explicit: Option<&Path>) -> Result<Self, String> {
        let config = match explicit {
            Some(path) => {
                // A bare relative name such as `research.toml` has an empty parent.
                let path = start.join(path);
                if !path.is_file() {
                    return Err(format!("{}: no such config file", path.display()));
                }
                Some(path)
            }
            None => start
                .ancestors()
                .map(|dir| dir.join(CONFIG_FILE))
                .find(|candidate| candidate.is_file()),
        };
        let (root, raw) = match &config {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                let raw: RawConfig =
                    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
                let root = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                (root, raw)
            }
            None => (start.to_path_buf(), RawConfig::default()),
        };
        let name = raw.project.name.unwrap_or_else(|| {
            root.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "project".into())
        });
        let tz = match raw.project.timezone.as_deref() {
            Some(name) => jiff::tz::TimeZone::get(name)
                .map_err(|e| format!("[project] timezone `{name}`: {e}"))?,
            None => jiff::tz::TimeZone::system(),
        };
        let state_dir = root.join(raw.project.state_dir.as_deref().unwrap_or(".research"));
        let mut names = std::collections::BTreeSet::new();
        for c in &raw.lab.checks {
            if c.name.trim().is_empty() {
                return Err("[lab] every check needs a non-empty `name`".into());
            }
            if !names.insert(c.name.as_str()) {
                return Err(format!(
                    "[lab] two checks are named `{}`; names must be unique",
                    c.name
                ));
            }
            if c.run.is_empty() {
                return Err(format!("[lab] check `{}` needs a non-empty `run`", c.name));
            }
            if let Some(p) = c
                .uncommitted_inputs
                .iter()
                .find(|p| !below_root(Path::new(p)))
            {
                return Err(format!(
                    "[lab] check `{}`: uncommitted input `{p}` must be a relative path below \
                     the project root, without `..`",
                    c.name
                ));
            }
        }
        let e = &raw.lab.evaluate;
        if e.min_sessions == 0 {
            return Err("[lab.evaluate] min_sessions must be at least 1".into());
        }
        if !(e.alpha > 0.0 && e.alpha < 1.0) {
            return Err(format!(
                "[lab.evaluate] alpha must be between 0 and 1, got {}",
                e.alpha
            ));
        }
        if e.baseline_days > MAX_BASELINE_DAYS {
            return Err(format!(
                "[lab.evaluate] baseline_days must be at most {MAX_BASELINE_DAYS}, got {}",
                e.baseline_days
            ));
        }
        if let Some(l) = &raw.lab.lesson_loop {
            if l.renotify_hours > MAX_RENOTIFY_HOURS {
                return Err(format!(
                    "[lab.loop] renotify_hours must be at most {MAX_RENOTIFY_HOURS}, got {}",
                    l.renotify_hours
                ));
            }
            if l.measure.iter().any(Vec::is_empty) {
                return Err("[lab.loop] every `measure` command needs a program".into());
            }
            if l.notify.as_ref().is_some_and(Vec::is_empty) {
                return Err("[lab.loop] `notify` needs a program".into());
            }
        }
        Ok(Self {
            root,
            name,
            tz,
            state_dir,
            lab: raw.lab,
        })
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn lessons_dir(&self) -> PathBuf {
        self.path(&self.lab.lessons_dir)
    }

    /// A check's program: a relative path with a directory part (`scripts/x`)
    /// resolves against the project root; a bare name is looked up on `PATH`.
    pub fn program(&self, spec: &CheckSpec) -> PathBuf {
        self.resolve_program(&spec.run[0])
    }

    /// A command's program, resolved as for a check.
    pub fn resolve_program(&self, program: &str) -> PathBuf {
        let program = Path::new(program);
        if program.is_relative() && program.components().count() > 1 {
            self.root.join(program)
        } else {
            program.to_path_buf()
        }
    }

    pub fn baseline_path(&self) -> PathBuf {
        self.state_dir.join("lab").join("baseline.json")
    }

    pub fn checks_dir(&self) -> PathBuf {
        self.state_dir.join("lab").join("checks")
    }

    /// `lab loop`'s lock, last run record, signal state and log.
    pub fn loop_dir(&self) -> PathBuf {
        self.state_dir.join("lab").join("loop")
    }

    pub fn default_seal_manifest(&self) -> PathBuf {
        self.state_dir.join("lab").join("archive-seal.json")
    }

    /// Today's date in the project's zone.
    pub fn today(&self) -> jiff::civil::Date {
        jiff::Timestamp::now().to_zoned(self.tz.clone()).date()
    }

    /// The calendar date, in the project's zone, of a stored RFC 3339 time.
    pub fn date_of(&self, timestamp: &str) -> Result<jiff::civil::Date, String> {
        timestamp
            .parse::<jiff::Timestamp>()
            .map(|t| t.to_zoned(self.tz.clone()).date())
            .map_err(|e| format!("bad timestamp `{timestamp}`: {e}"))
    }
}

/// Whether `path` is a non-empty relative path made only of plain names, so
/// joined to a directory it stays below it: no root, `.` or `..`.
pub fn below_root(path: &Path) -> bool {
    path.components().next().is_some()
        && path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_up_to_the_config_and_resolves_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CONFIG_FILE),
            "[project]\nname = \"demo\"\ntimezone = \"Asia/Tokyo\"\nstate_dir = \"state\"\n[lab]\nlessons_dir = \"notes/lessons\"\n\
             [[lab.checks]]\nname = \"a\"\nrun = [\"true\"]\n[papers]\nnotes = \"x\"\n",
        )
        .unwrap();
        let nested = dir.path().join("docs/deep");
        std::fs::create_dir_all(&nested).unwrap();
        let p = Project::discover(&nested, None).unwrap();
        assert_eq!(p.root, dir.path());
        assert_eq!(p.name, "demo");
        assert_eq!(p.state_dir, dir.path().join("state"));
        assert_eq!(p.lessons_dir(), dir.path().join("notes/lessons"));
        assert_eq!(p.lab.checks.len(), 1);
        // 2026-11-06T17:00Z is already the 7th in Tokyo.
        assert_eq!(
            p.date_of("2026-11-06T17:00:00Z").unwrap().to_string(),
            "2026-11-07"
        );
    }

    #[test]
    fn defaults_without_a_config() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::discover(dir.path(), None).unwrap();
        assert_eq!(p.state_dir, dir.path().join(".research"));
        assert_eq!(p.lab.recurrence, 3);
    }

    #[test]
    fn rejects_unknown_lab_keys_and_empty_checks() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join(CONFIG_FILE);
        std::fs::write(&cfg, "[lab]\nlesson_dir = \"x\"\n").unwrap();
        assert!(Project::discover(dir.path(), None).is_err());
        std::fs::write(&cfg, "[project]\ntimezone = \"Mars/Base\"\n").unwrap();
        assert!(Project::discover(dir.path(), None).is_err());
        std::fs::write(&cfg, "[[lab.checks]]\nname = \"a\"\nrun = []\n").unwrap();
        assert!(Project::discover(dir.path(), None).is_err());
        std::fs::write(
            &cfg,
            "[[lab.checks]]\nname = \"x\"\nrun = [\"a\"]\n[[lab.checks]]\nname = \"x\"\nrun = [\"b\"]\n",
        )
        .unwrap();
        let err = Project::discover(dir.path(), None).unwrap_err();
        assert!(err.contains("two checks are named `x`"), "{err}");
        for input in ["../up", "/abs", "", "a/../b", "./a"] {
            std::fs::write(
                &cfg,
                format!(
                    "[[lab.checks]]\nname = \"x\"\nrun = [\"a\"]\nuncommitted_inputs = [\"{input}\"]\n"
                ),
            )
            .unwrap();
            let err = Project::discover(dir.path(), None).unwrap_err();
            assert!(
                err.contains("relative path below the project root"),
                "{input}: {err}"
            );
        }
        for bad in [
            "[lab.evaluate]\nmin_session = 3\n",
            "[lab.evaluate]\nalpha = 1.5\n",
            "[lab.loop]\nmeasure = [[]]\n",
            "[lab.loop]\nnotfy = [\"x\"]\n",
            "[lab.evaluate]\nbaseline_days = 10000000\n",
            "[lab.loop]\nrenotify_hours = 18446744073709551615\n",
        ] {
            std::fs::write(&cfg, bad).unwrap();
            assert!(Project::discover(dir.path(), None).is_err(), "{bad}");
        }
    }

    #[test]
    fn evaluate_and_loop_tables() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CONFIG_FILE),
            "[lab.evaluate]\nmin_sessions = 5\nbaseline_days = 0\n\
             [lab.loop]\nmeasure = [[\"agentprof\", \"scan\"]]\nnotify = [\"sh\", \"n.sh\"]\n",
        )
        .unwrap();
        let p = Project::discover(dir.path(), None).unwrap();
        let e = &p.lab.evaluate;
        assert_eq!((e.min_sessions, e.alpha, e.baseline_days), (5, 0.05, 0));
        let l = p.lab.lesson_loop.unwrap();
        assert_eq!((l.measure.len(), l.renotify_hours), (1, 24));
        let empty = tempfile::tempdir().unwrap();
        let defaults = Project::discover(empty.path(), None).unwrap();
        assert!(defaults.lab.lesson_loop.is_none());
        assert_eq!(defaults.lab.evaluate.min_sessions, 10);
    }

    #[test]
    fn a_bare_relative_config_resolves_against_the_start_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CONFIG_FILE),
            "[project]\nname = \"demo\"\n[[lab.checks]]\nname = \"s\"\nrun = [\"scripts/check.sh\"]\n",
        )
        .unwrap();
        let p = Project::discover(dir.path(), Some(Path::new(CONFIG_FILE))).unwrap();
        assert_eq!(p.root, dir.path());
        assert_eq!(p.name, "demo");
        assert_eq!(
            p.program(&p.lab.checks[0]),
            dir.path().join("scripts/check.sh")
        );
    }
}
