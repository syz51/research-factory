//! `lab check --new-only` against the change: the findings that exist in the
//! work tree and not at the merge base with the default branch, both checked
//! by today's checkers. Errors and warnings among them in files the change
//! touches fail the gate; the others are counted apart.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::check::{Delta, Item, Run};
use crate::config::Project;
use crate::finding::{Finding, Severity, without_line};

/// A finding of the work tree that the base lacks.
pub struct New<'r> {
    pub item: &'r Item,
    /// Where it is: the subject, or, when the subject is not a path, the
    /// evidence paths with their locators and then the subject.
    pub place: String,
    /// The files it names, relative to the project root.
    files: Vec<String>,
    /// The subject's line, when the subject is a path with one.
    line: Option<usize>,
    /// The files it names and its evidence paths, relative to the project root.
    inputs: Vec<String>,
}

/// The new findings, sorted by place, in the classes the gate treats apart.
#[derive(Default)]
pub struct Classes<'r> {
    /// Errors and warnings in files the change touches: these fail.
    pub gated: Vec<New<'r>>,
    /// Errors and warnings there that the baseline accepts.
    pub accepted: Vec<New<'r>>,
    /// Info findings in files the change touches.
    pub info: Vec<New<'r>>,
    /// In text sealed at the base, which no change may edit.
    pub sealed: Vec<New<'r>>,
    /// Naming a file that is missing and that git ignores, such as a
    /// download this checkout lacks.
    pub missing_input: Vec<New<'r>>,
    /// In files the change does not touch, or naming no file.
    pub elsewhere: Vec<New<'r>>,
}

pub struct Change<'r> {
    /// The default branch, such as `main` or `origin/main`.
    pub branch: String,
    /// The merge base of `HEAD` and `branch`.
    pub base: String,
    /// How many files under the project root differ from the base.
    pub changed: usize,
    pub new: Classes<'r>,
    /// Checks that ran now but not to completion at the base: each of their
    /// findings counts as new.
    pub no_base: Vec<String>,
}

pub enum Outcome<'r> {
    /// No comparison was made, for this reason.
    Skipped(String),
    /// No file under the project root differs from the base.
    Unchanged {
        branch: String,
        base: String,
    },
    Compared(Change<'r>),
}

/// Compares `run`, the work tree's run, with the selected checks run on the
/// merge base. `delta` is each check's change since its baseline.
pub fn compare<'r>(
    project: &Project,
    only: &[String],
    run: &'r Run,
    delta: &BTreeMap<String, Option<Delta>>,
) -> Result<Outcome<'r>, String> {
    let root = &project.root;
    if run.header.commit.is_none() {
        return Ok(Outcome::Skipped(
            "the project is not in a git repository with a commit".into(),
        ));
    }
    let Some(branch) = crate::worktree::default_branch(root)? else {
        return Ok(Outcome::Skipped(
            "no default branch: neither origin/HEAD nor main, master, origin/main or \
             origin/master exists"
                .into(),
        ));
    };
    let Some(base) = crate::worktree::merge_base(root, &branch)? else {
        return Ok(Outcome::Skipped(format!(
            "HEAD and {branch} share no commit"
        )));
    };
    let state = project.state_dir.strip_prefix(root).ok();
    let changed: BTreeSet<String> = crate::worktree::changed_since(root, &base)?
        .into_iter()
        .filter(|p| state.is_none_or(|s| !Path::new(p).starts_with(s)))
        .collect();
    if changed.is_empty() {
        return Ok(Outcome::Unchanged { branch, base });
    }
    let ran_now: Vec<&str> = run
        .header
        .checks
        .iter()
        .filter(|c| c.ran())
        .map(|c| c.name.as_str())
        .collect();
    let specs: Vec<_> = crate::check::select(project, only)?
        .into_iter()
        .filter(|s| ran_now.contains(&s.name.as_str()))
        .collect();
    if specs.is_empty() {
        return Ok(Outcome::Skipped("no check ran to completion".into()));
    }
    let Some(at_base) = crate::replay::run_base(project, &specs, &base)? else {
        return Ok(Outcome::Skipped(format!(
            "the merge base {} has no project root",
            short(&base)
        )));
    };
    let mut before: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for outcome in at_base.outcomes.iter().filter(|o| o.ran()) {
        before.insert(outcome.name.as_str(), BTreeSet::new());
    }
    for item in &at_base.items {
        if let Some(set) = before.get_mut(item.check.as_str()) {
            set.insert(item.fingerprint.as_str());
        }
    }
    let no_base: Vec<String> = specs
        .iter()
        .filter(|s| !before.contains_key(s.name.as_str()))
        .map(|s| s.name.clone())
        .collect();
    let mut new = Vec::new();
    for item in &run.items {
        let Some(spec) = specs.iter().find(|s| s.name == item.check) else {
            continue;
        };
        if before
            .get(item.check.as_str())
            .is_some_and(|b| b.contains(item.fingerprint.as_str()))
        {
            continue;
        }
        let (place, files) = place(root, spec.cwd.as_deref(), &item.finding);
        let line = line_of(&item.finding.subject);
        let mut inputs = files.clone();
        for e in &item.finding.evidence {
            if let Some(rel) = relative(root, spec.cwd.as_deref(), &e.path)
                && !inputs.contains(&rel)
            {
                inputs.push(rel);
            }
        }
        new.push(New {
            item,
            place,
            files,
            line,
            inputs,
        });
    }
    new.sort_by(|a, b| sort_key(&a.place).cmp(&sort_key(&b.place)));
    let named: BTreeSet<String> = new.iter().flat_map(|n| n.files.iter().cloned()).collect();
    let sealed = crate::archive::sealed_at(project, &base, &named)?;
    let inputs: BTreeSet<&str> = new
        .iter()
        .flat_map(|n| n.inputs.iter().map(String::as_str))
        .collect();
    let absent: Vec<&str> = inputs
        .into_iter()
        .filter(|p| !root.join(p).exists())
        .collect();
    let missing = crate::worktree::ignored(root, absent)?;
    let mut classes = Classes::default();
    for n in new {
        // Text appended to a sealed file after the base is the change's own.
        let in_sealed = n.files.iter().any(|f| {
            sealed
                .get(f)
                .is_some_and(|&last| n.line.is_none_or(|l| l <= last))
        });
        let class = if in_sealed {
            &mut classes.sealed
        } else if n.inputs.iter().any(|f| missing.contains(f)) {
            &mut classes.missing_input
        } else if !n.files.iter().any(|f| changed.contains(f)) {
            &mut classes.elsewhere
        } else if n.item.finding.severity == Severity::Info {
            &mut classes.info
        } else if delta
            .get(&n.item.check)
            .and_then(Option::as_ref)
            .is_some_and(|d| !d.new.contains(&n.item.fingerprint))
        {
            &mut classes.accepted
        } else {
            &mut classes.gated
        };
        class.push(n);
    }
    Ok(Outcome::Compared(Change {
        branch,
        base,
        changed: changed.len(),
        new: classes,
        no_base,
    }))
}

/// A commit's short form for messages.
pub fn short(commit: &str) -> &str {
    &commit[..commit.len().min(10)]
}

/// Where a finding is, and the files it names relative to the project root:
/// the subject when it is a path, else the evidence paths.
fn place(root: &Path, cwd: Option<&str>, f: &Finding) -> (String, Vec<String>) {
    let subject = without_line(&f.subject);
    if looks_like_path(subject) {
        return (
            f.subject.clone(),
            relative(root, cwd, subject).into_iter().collect(),
        );
    }
    let mut files = Vec::new();
    let mut shown = Vec::new();
    for e in &f.evidence {
        if let Some(rel) = relative(root, cwd, &e.path)
            && !files.contains(&rel)
        {
            files.push(rel);
        }
        shown.push(match &e.locator {
            Some(l) => format!("{} ({l})", e.path),
            None => e.path.clone(),
        });
    }
    shown.dedup();
    let place = if shown.is_empty() {
        f.subject.clone()
    } else {
        format!("{} [{}]", shown.join(", "), f.subject)
    };
    (place, files)
}

/// A path has a directory part or an extension, as in `without_line`.
fn looks_like_path(s: &str) -> bool {
    !s.is_empty() && (s.contains('/') || s.contains('.'))
}

/// `p`, as a checker run in `cwd` (relative to the project root) printed it,
/// relative to the project root; `None` for an absolute path outside it.
fn relative(root: &Path, cwd: Option<&str>, p: &str) -> Option<String> {
    let path = Path::new(p);
    let rel = if path.is_absolute() {
        path.strip_prefix(root).ok()?.to_path_buf()
    } else {
        match cwd {
            Some(c) => Path::new(c).join(path),
            None => path.to_path_buf(),
        }
    };
    let rel = rel.to_string_lossy();
    Some(rel.strip_prefix("./").unwrap_or(&rel).to_string())
}

/// The first line number after a path's `:`, as in `docs/a.md:42` or
/// `docs/a.md:42:7`.
fn line_of(subject: &str) -> Option<usize> {
    let file = without_line(subject);
    subject[file.len()..]
        .strip_prefix(':')?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// Sorts places by file, then by line number.
fn sort_key(place: &str) -> (&str, usize, &str) {
    (without_line(place), line_of(place).unwrap_or(0), place)
}
