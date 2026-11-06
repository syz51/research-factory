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
    /// A finding rule that recurs in at least this many check runs or sessions
    /// without a lesson covering it becomes a lesson candidate.
    #[serde(default = "default_recurrence")]
    pub recurrence: usize,
}

impl Default for LabConfig {
    fn default() -> Self {
        Self {
            lessons_dir: default_lessons_dir(),
            checks: Vec::new(),
            agent_findings: None,
            recurrence: default_recurrence(),
        }
    }
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
}

impl Project {
    /// Finds `research.toml` by walking up from `start`, or loads `explicit`.
    /// Without either, the project root is `start` and every setting is a default.
    pub fn discover(start: &Path, explicit: Option<&Path>) -> Result<Self, String> {
        let config = match explicit {
            Some(path) => {
                if !path.is_file() {
                    return Err(format!("{}: no such config file", path.display()));
                }
                Some(path.to_path_buf())
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
        if raw.lab.checks.iter().any(|c| c.run.is_empty()) {
            return Err("[lab] every check needs a non-empty `run`".into());
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

    pub fn checks_dir(&self) -> PathBuf {
        self.state_dir.join("lab").join("checks")
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
    }
}
