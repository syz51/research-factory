//! `lab check --at`: run today's checkers on past commits, each checked out
//! into a temporary git worktree, so `lab evaluate` gets interval ends from
//! before check runs measured their text.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::check::{self, Run, RunHeader};
use crate::config::{CheckSpec, Project};

/// Runs git in `dir` with a stable message language and no hooks, since a
/// checkout would otherwise run the repository's `post-checkout` hook.
fn git(dir: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("git: {e}"))
}

/// Runs git in `dir` and returns its trimmed stdout, or its stderr as the error.
fn git_ok(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(dir, args)?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The commits `revs` name, oldest first within a range, without repeats:
/// a revision is one commit, and `A..B` every commit `git rev-list` lists.
pub fn commits(root: &Path, revs: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for rev in revs {
        let listed = if rev.contains("..") {
            git_ok(root, &["rev-list", "--reverse", rev, "--"])?
        } else {
            git_ok(
                root,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{rev}^{{commit}}"),
                ],
            )?
        };
        if listed.is_empty() {
            return Err(format!("`{rev}` names no commit"));
        }
        for commit in listed.lines() {
            if seen.insert(commit.to_string()) {
                out.push(commit.to_string());
            }
        }
    }
    Ok(out)
}

/// A temporary git worktree, removed when dropped, so it goes on every path
/// out of a replay, including an error or a panic.
struct Worktree {
    /// A directory in the repository, to run `git worktree` from.
    repo: PathBuf,
    path: PathBuf,
}

impl Worktree {
    fn add(repo: &Path, commit: &str) -> Result<Self, String> {
        let stamp = jiff::Timestamp::now().as_nanosecond();
        let path = std::env::temp_dir().join(format!("lab-replay-{}-{stamp}", std::process::id()));
        // The guard exists before git creates anything, so a half-made
        // worktree is removed too.
        let tree = Self {
            repo: repo.to_path_buf(),
            path,
        };
        let path = tree.path.to_string_lossy().into_owned();
        git_ok(
            repo,
            &["worktree", "add", "--detach", "--quiet", &path, commit],
        )?;
        Ok(tree)
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        let path = self.path.to_string_lossy().into_owned();
        let removed = git(
            &self.repo,
            &["worktree", "remove", "--force", "--force", &path],
        );
        // git refuses a half-made worktree, so remove what is left directly.
        if !removed.as_ref().is_ok_and(|o| o.status.success())
            && self.path.exists()
            && let Err(e) = std::fs::remove_dir_all(&self.path)
        {
            eprintln!(
                "lab: could not remove the replay worktree {}: {e}",
                self.path.display()
            );
        }
        // Forget a registration whose directory is gone.
        if let Err(e) = git_ok(&self.repo, &["worktree", "prune"]) {
            eprintln!("lab: {e}");
        }
    }
}

/// Links each check's uncommitted inputs that the checkout lacks from the
/// project root into `root`, the checkout's project root. Returns, per check,
/// the inputs it reads from the work tree.
fn link_inputs(
    project: &Project,
    root: &Path,
    specs: &[&CheckSpec],
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut linked: BTreeSet<&str> = BTreeSet::new();
    let mut out = BTreeMap::new();
    for spec in specs {
        let mut mine = Vec::new();
        for input in &spec.uncommitted_inputs {
            if !linked.contains(input.as_str()) {
                let target = root.join(input);
                if target.symlink_metadata().is_ok() {
                    // This commit holds the path, so the check reads its copy.
                    continue;
                }
                let source = project.root.join(input);
                if !source.exists() {
                    eprintln!(
                        "lab: check `{}`: uncommitted input `{input}` is missing from the work tree",
                        spec.name
                    );
                    continue;
                }
                if let Some(dir) = target.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                std::os::unix::fs::symlink(&source, &target)
                    .map_err(|e| format!("{}: {e}", target.display()))?;
                linked.insert(input);
            }
            mine.push(input.clone());
        }
        if !mine.is_empty() {
            out.insert(spec.name.clone(), mine);
        }
    }
    Ok(out)
}

/// Replays each commit with the selected checks and stores its run, with that
/// `commit`, `dirty` false, `at` the commit's committer time and `replayed`
/// true. Checks run in the commit's copy of the project root; their programs
/// resolve against the project root, so they are today's.
pub fn replay(project: &Project, only: &[String], revs: &[String]) -> Result<Vec<Run>, String> {
    let specs = check::select(project, only)?;
    let root = &project.root;
    let prefix = git_ok(root, &["rev-parse", "--show-prefix"])?;
    let commits = commits(root, revs)?;
    let versions = check::versions(project, &specs);
    let mut runs = Vec::new();
    for commit in commits {
        let seconds: i64 = git_ok(root, &["show", "-s", "--format=%ct", &commit])?
            .parse()
            .map_err(|e| format!("commit {commit}: its time: {e}"))?;
        let at = jiff::Timestamp::from_second(seconds)
            .map_err(|e| format!("commit {commit}: its time: {e}"))?
            .to_string();
        let tree = Worktree::add(root, &commit)?;
        let replay_root = tree.path.join(&prefix);
        if !replay_root.is_dir() {
            return Err(format!(
                "commit {commit} has no `{}`, the project root",
                prefix.trim_end_matches('/')
            ));
        }
        let linked = link_inputs(project, &replay_root, &specs)?;
        let ran = check::run_checks(project, &replay_root, &specs, &versions, &linked);
        drop(tree);
        let header = RunHeader {
            schema: check::RUN_SCHEMA.into(),
            at,
            commit: Some(commit),
            dirty: Some(false),
            replayed: true,
            checks: Vec::new(),
        };
        runs.push(check::store_run(project, header, ran)?);
    }
    Ok(runs)
}
