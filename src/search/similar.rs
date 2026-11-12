//! The duplicate check for new records (docs/archive-design.md 5): the
//! records whose sections hold most of the words of a new record's title and
//! aliases. It runs on the search index and builds the index when it is
//! absent. A failed search never fails the caller: the check is `skipped`.

use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, ExitCode};

use crate::archive::Spec;
use crate::config::Project;

use super::query::words;
use super::{Request, traced};

/// Records the check reads before it filters.
const CANDIDATES: usize = 20;
/// Records it reports.
const SHOWN: usize = 5;
/// The share of the subject's term weight a record must hold.
const MIN_SHARE: f64 = 0.6;
/// Words of the subject a query takes.
const MAX_WORDS: usize = 32;

/// A record like the subject.
#[derive(Debug, Clone)]
pub struct Similar {
    pub path: String,
    pub id: Option<String>,
    pub title: Option<String>,
    /// The share of the subject's term weight the record holds.
    pub share: f64,
}

/// What the check found, or why it did not run.
#[derive(Debug)]
pub enum Outcome {
    Found(Vec<Similar>),
    Skipped(String),
}

/// The title and aliases of the record file `path`, as search words.
pub fn subject_of(project: &Project, path: &str) -> Result<String, String> {
    let bytes = std::fs::read(project.path(path)).map_err(|e| format!("{path}: {e}"))?;
    let spec = Spec::new(project)?;
    let meta = spec.meta_of(path, &bytes);
    let mut parts: Vec<String> = meta.title.into_iter().collect();
    parts.extend(meta.aliases);
    if parts.is_empty() {
        return Err(format!("{path} has no title or alias to compare"));
    }
    Ok(parts.join(" "))
}

/// The records like `subject`, best first, leaving out the files `exclude`.
pub fn similar(project: &Project, subject: &str, exclude: &[String]) -> Outcome {
    let mut seen = Vec::new();
    for w in words(subject) {
        if !seen.contains(&w) && seen.len() < MAX_WORDS {
            seen.push(w);
        }
    }
    if seen.is_empty() {
        return Outcome::Skipped("the subject holds no word".into());
    }
    let phrasing = vec![seen.join(" ")];
    let request = Request {
        phrasings: &phrasing,
        scopes: &["notes".to_string()],
        limit: CANDIDATES,
        records: true,
        statuses: &[],
        rebuild: false,
        weigh: true,
    };
    let answer = match traced(project, "similar", &request) {
        Ok(answer) => answer,
        Err(e) => return Outcome::Skipped(e),
    };
    let total: f64 = answer.weights.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return Outcome::Found(Vec::new());
    }
    let terms = answer.weights.len();
    let mut found = Vec::new();
    for hit in answer.hits {
        if exclude.contains(&hit.path) {
            continue;
        }
        let matched = hit.explain.map(|e| e.matched).unwrap_or_default();
        let held: f64 = answer
            .weights
            .iter()
            .filter(|(t, _)| matched.contains(t))
            .map(|(_, w)| w)
            .sum();
        let share = held / total;
        if share < MIN_SHARE || (terms >= 2 && matched.len() < 2) {
            continue;
        }
        found.push(Similar {
            path: hit.path,
            id: hit.record.id,
            title: hit.record.title,
            share,
        });
    }
    found.sort_by(|a, b| b.share.total_cmp(&a.share));
    found.truncate(SHOWN);
    Outcome::Found(found)
}

/// One line per record.
pub fn render(found: &[Similar]) -> String {
    let mut out = String::new();
    for s in found {
        let _ = writeln!(
            out,
            "- {} {}{} (shares {:.0}% of the words' weight)",
            s.id.as_deref().unwrap_or("-"),
            s.path,
            s.title
                .as_deref()
                .map(|t| format!(" · {t}"))
                .unwrap_or_default(),
            s.share * 100.0
        );
    }
    out
}

/// `lab archive similar`: a record file, or title words. Exit 0 also when
/// the check is skipped.
pub fn run(project: &Project, args: &[String]) -> Result<ExitCode, String> {
    let file = match args {
        [one] if project.path(one).is_file() => Some(one.as_str()),
        _ => None,
    };
    let subject = match file {
        Some(f) => subject_of(project, f)?,
        None => args.join(" "),
    };
    let exclude: Vec<String> = file.map(str::to_string).into_iter().collect();
    match similar(project, &subject, &exclude) {
        Outcome::Found(found) if found.is_empty() => {
            println!("similar: none");
        }
        Outcome::Found(found) => {
            println!("similar: {} record(s) on the same topic", found.len());
            print!("{}", render(&found));
        }
        Outcome::Skipped(why) => println!("skipped: {why}"),
    }
    Ok(ExitCode::SUCCESS)
}

/// The files that git sees as new below `root`: untracked and not ignored,
/// or added in the index. None outside git.
pub fn new_files(root: &Path) -> Vec<String> {
    let list = |args: &[&str]| -> Vec<String> {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .env("LC_ALL", "C")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                o.stdout
                    .split(|&b| b == 0)
                    .filter(|p| !p.is_empty())
                    .map(|p| String::from_utf8_lossy(p).into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut files = list(&["ls-files", "-z", "--others", "--exclude-standard"]);
    files.extend(list(&[
        "diff",
        "--cached",
        "-z",
        "--name-only",
        "--relative",
        "--diff-filter=A",
    ]));
    files.sort();
    files.dedup();
    files
}
