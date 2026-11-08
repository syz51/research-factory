//! `lab check --at`: run today's checkers on past commits, each checked out
//! into a temporary git worktree, so `lab evaluate` gets interval ends from
//! before check runs measured their text.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::check::{self, Run, RunHeader};
use crate::config::{CheckSpec, Project};

/// The prefix of a replay worktree's directory name, followed by the pid of
/// the `lab` process that made it.
const WORKTREE_PREFIX: &str = "lab-replay-";

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
/// a revision is one commit, and `A..B` every commit `git rev-list` lists
/// that changed something under the project root (`root`).
pub fn commits(root: &Path, revs: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for rev in revs {
        let listed = if rev.contains("..") {
            git_ok(root, &["rev-list", "--reverse", rev, "--", "."])?
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
            return Err(format!("`{rev}` names no commit that changed the project"));
        }
        for commit in listed.lines() {
            if seen.insert(commit.to_string()) {
                out.push(commit.to_string());
            }
        }
    }
    Ok(out)
}

/// A commit's time as a replay dates its run: its committer time, `%ct`.
fn committed_at(commit: &str, seconds: &str) -> Result<jiff::Timestamp, String> {
    seconds
        .parse::<i64>()
        .map_err(|e| e.to_string())
        .and_then(|s| jiff::Timestamp::from_second(s).map_err(|e| e.to_string()))
        .map_err(|e| format!("commit {commit}: its time `{seconds}`: {e}"))
}

/// The committer times of every commit a replay can check that changed
/// something under the project root (`root`): reachable from any ref or
/// reflog entry. `None` when git history cannot be read, or is incomplete
/// in a shallow clone; a failure other than git missing or no repository is
/// reported on stderr.
pub fn commit_times(root: &Path) -> Option<Vec<jiff::Timestamp>> {
    let read = || -> Result<Option<Vec<jiff::Timestamp>>, String> {
        let shallow = match git(root, &["rev-parse", "--is-shallow-repository"]) {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim() == "true"
            }
            // No git, or no repository: nothing to report.
            Err(_) => return Ok(None),
            Ok(out) if String::from_utf8_lossy(&out.stderr).contains("not a git repository") => {
                return Ok(None);
            }
            Ok(out) => return Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        };
        if shallow {
            eprintln!(
                "lab: a shallow clone does not hold every commit before adoption; \
                 `git fetch --unshallow` lets `lab evaluate` see them"
            );
            return Ok(None);
        }
        // `rev-list` prints no signatures, whatever `log.showSignature` says.
        let listed = git_ok(
            root,
            &[
                "rev-list",
                "--all",
                "--reflog",
                "--format=%H %ct",
                "--",
                ".",
            ],
        )?;
        listed
            .lines()
            .filter(|l| !l.starts_with("commit "))
            .map(|l| {
                let (commit, seconds) = l.split_once(' ').unwrap_or((l, ""));
                committed_at(commit, seconds)
            })
            .collect::<Result<_, _>>()
            .map(Some)
    };
    read().unwrap_or_else(|e| {
        eprintln!("lab: could not read the git history, so the replay advice stays: {e}");
        None
    })
}

/// The administrative directory git keeps for the worktree at `path`, found
/// by the directory name, which is unique to one replay.
fn admin_dir(repo: &Path, path: &Path) -> Option<PathBuf> {
    let common = git_ok(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?;
    let name = path.file_name()?;
    std::fs::read_dir(Path::new(&common).join("worktrees"))
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|admin| {
            std::fs::read_to_string(admin.join("gitdir")).is_ok_and(|gitdir| {
                Path::new(gitdir.trim())
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|n| n == name)
            })
        })
}

/// Removes the replay worktree at `path` and its registration, and only
/// those: unrelated worktrees whose directory is missing stay registered.
fn remove_worktree(repo: &Path, path: &Path) {
    let p = path.to_string_lossy().into_owned();
    if git(repo, &["worktree", "remove", "--force", "--force", &p])
        .is_ok_and(|o| o.status.success())
    {
        return;
    }
    // git refuses a half-made or vanished worktree: remove its directory and
    // its registration directly.
    let admin = admin_dir(repo, path);
    if path.exists()
        && let Err(e) = std::fs::remove_dir_all(path)
    {
        eprintln!(
            "lab: could not remove the replay worktree {}: {e}",
            path.display()
        );
    }
    if let Some(admin) = admin
        && let Err(e) = std::fs::remove_dir_all(&admin)
    {
        eprintln!("lab: {}: {e}", admin.display());
    }
}

/// Whether the process `pid` is running.
fn alive(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Removes replay worktrees that a `lab` process killed by a signal, such as
/// Ctrl-C, left behind: `Drop` does not run then.
fn remove_leftovers(repo: &Path) -> Result<(), String> {
    let list = git_ok(repo, &["worktree", "list", "--porcelain"])?;
    for path in list.lines().filter_map(|l| l.strip_prefix("worktree ")) {
        let path = Path::new(path);
        let pid = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix(WORKTREE_PREFIX))
            .and_then(|rest| rest.split_once('-'))
            .and_then(|(pid, _)| pid.parse::<u32>().ok());
        if let Some(pid) = pid
            && pid != std::process::id()
            && !alive(pid)
        {
            eprintln!(
                "lab: removing the replay worktree {} left by a stopped run",
                path.display()
            );
            remove_worktree(repo, path);
        }
    }
    Ok(())
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
        let path =
            std::env::temp_dir().join(format!("{WORKTREE_PREFIX}{}-{stamp}", std::process::id()));
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
        remove_worktree(&self.repo, &self.path);
    }
}

/// Links `source` to `target` when the checkout lacks it; in a directory both
/// have, such as one whose committed `.gitignore` or `.gitkeep` keeps it,
/// links each entry the checkout lacks. Returns whether it linked anything.
fn link_missing(source: &Path, target: &Path) -> Result<bool, String> {
    let Ok(meta) = target.symlink_metadata() else {
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::os::unix::fs::symlink(source, target)
            .map_err(|e| format!("{}: {e}", target.display()))?;
        return Ok(true);
    };
    if !(meta.is_dir() && source.is_dir()) {
        // The commit holds this file, so the check reads its copy.
        return Ok(false);
    }
    let mut any = false;
    let entries = std::fs::read_dir(source).map_err(|e| format!("{}: {e}", source.display()))?;
    for entry in entries {
        let name = entry
            .map_err(|e| format!("{}: {e}", source.display()))?
            .file_name();
        any |= link_missing(&source.join(&name), &target.join(&name))?;
    }
    Ok(any)
}

/// Links each check's uncommitted inputs, or the parts of them the checkout
/// lacks, from the project root into `root`, the checkout's project root.
/// Returns, per check, the inputs it reads from the work tree.
fn link_inputs(
    project: &Project,
    root: &Path,
    specs: &[&CheckSpec],
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut done: BTreeMap<&str, bool> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for spec in specs {
        let mut mine = Vec::new();
        for input in &spec.uncommitted_inputs {
            let linked = match done.get(input.as_str()) {
                Some(&linked) => linked,
                None => {
                    let source = project.root.join(input);
                    let linked = if source.symlink_metadata().is_ok() {
                        link_missing(&source, &root.join(input))?
                    } else {
                        eprintln!(
                            "lab: check `{}`: uncommitted input `{input}` is missing from the work tree",
                            spec.name
                        );
                        false
                    };
                    done.insert(input, linked);
                    linked
                }
            };
            if linked {
                mine.push(input.clone());
            }
        }
        if !mine.is_empty() {
            out.insert(spec.name.clone(), mine);
        }
    }
    Ok(out)
}

/// Replays each commit with the selected checks and stores its run, with that
/// `commit`, `dirty` false, `at` the commit's committer time and `replayed`
/// true, passing each stored run to `stored`. Checks run in the commit's copy
/// of the project root: only their program resolves against the project root,
/// and their versions are read there too. A commit without the project root
/// is skipped with a note; none with it is an error.
pub fn replay(
    project: &Project,
    only: &[String],
    revs: &[String],
    mut stored: impl FnMut(&Run),
) -> Result<(), String> {
    let specs = check::select(project, only)?;
    if specs.len() < project.lab.checks.len() {
        eprintln!(
            "lab: `lab evaluate` uses a run, replayed or not, only when every configured check \
             that reports the metric's tool ran in it; `--only` may leave one out"
        );
    }
    let root = &project.root;
    let prefix = git_ok(root, &["rev-parse", "--show-prefix"])?;
    let commits = commits(root, revs)?;
    remove_leftovers(root)?;
    let mut replayed = 0;
    for commit in commits {
        let seconds = git_ok(root, &["show", "-s", "--format=%ct", &commit])?;
        let at = committed_at(&commit, &seconds)?.to_string();
        let tree = Worktree::add(root, &commit)?;
        let replay_root = tree.path.join(&prefix);
        if !replay_root.is_dir() {
            eprintln!(
                "lab: commit {commit} has no `{}`, the project root; skipped",
                prefix.trim_end_matches('/')
            );
            continue;
        }
        let linked = link_inputs(project, &replay_root, &specs)?;
        let versions = check::versions(project, &replay_root, &specs);
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
        stored(&check::store_run(project, header, ran)?);
        replayed += 1;
    }
    if replayed == 0 {
        return Err(format!(
            "no commit named holds `{}`, the project root",
            prefix.trim_end_matches('/')
        ));
    }
    Ok(())
}
