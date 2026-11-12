//! The main checkout of a linked git worktree, and the gitignored files that a
//! new linked worktree lacks.
//!
//! Files that git ignores, such as agent-session scans, downloaded sources and
//! generated data, exist only in the checkout that made them: usually the main
//! checkout. lab looks for such a file in the main checkout when a linked
//! worktree lacks it. It only checks that the named path exists there, or reads
//! that one file; it never lists, copies or changes anything in the main
//! checkout.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where a directory is, relative to git worktrees.
pub enum Location {
    /// Not in a linked git worktree: in a main checkout, outside git, or git
    /// is not installed.
    NotLinked,
    /// In a linked worktree of a bare repository, which has no main checkout.
    Bare,
    /// In a linked worktree: `main` is the same directory in the main
    /// checkout; `replay` says the worktree is one `lab check --at` made.
    Linked { main: PathBuf, replay: bool },
}

/// Runs git in `dir` with a stable message language. `None` when git is not
/// installed or `dir` is not in a git repository.
fn git(dir: &Path, args: &[&str]) -> Result<Option<String>, String> {
    let out = match Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .output()
    {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("git: {e}")),
    };
    let err = String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
    } else if err.contains("not a git repository") {
        Ok(None)
    } else {
        Err(format!("git {}: {}", args.join(" "), err.trim()))
    }
}

/// The files below `dir` that git neither tracks nor ignores, relative to
/// `dir`: they exist only in this checkout until someone commits them. Empty
/// outside a git repository or without git.
pub fn untracked(dir: &Path) -> Result<BTreeSet<String>, String> {
    Ok(
        git(dir, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .map(|out| {
                out.split('\0')
                    .filter(|p| !p.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
    )
}

/// Where `dir` is: the main checkout is the first entry of `git worktree list`.
pub fn locate(dir: &Path) -> Result<Location, String> {
    let Some(dirs) = git(
        dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
            "--show-prefix",
        ],
    )?
    else {
        return Ok(Location::NotLinked);
    };
    let mut lines = dirs.lines();
    let (Some(git_dir), Some(common)) = (lines.next(), lines.next()) else {
        return Err(format!("git rev-parse printed too little: {dirs:?}"));
    };
    if Path::new(git_dir) == Path::new(common) {
        return Ok(Location::NotLinked);
    }
    // `dir` below the top of the work tree, empty at the top.
    let prefix = lines.next().unwrap_or("");
    let list = git(dir, &["worktree", "list", "--porcelain"])?.unwrap_or_default();
    let main = list.split("\n\n").next().unwrap_or("");
    match main.lines().find_map(|l| l.strip_prefix("worktree ")) {
        Some(p) if !main.lines().any(|l| l == "bare") => Ok(Location::Linked {
            main: Path::new(p).join(prefix),
            // git names a worktree's administrative directory after its own.
            replay: Path::new(git_dir).file_name().is_some_and(|n| {
                n.to_string_lossy()
                    .starts_with(crate::replay::WORKTREE_PREFIX)
            }),
        }),
        _ => Ok(Location::Bare),
    }
}

/// Finds gitignored files that a linked worktree lacks in its main checkout.
pub struct Fallback {
    /// The directory in the linked worktree that paths are relative to.
    here: PathBuf,
    /// The same directory in the main checkout.
    main: PathBuf,
}

impl Fallback {
    /// For paths relative to `dir`; `None` unless `dir` is in a linked
    /// worktree that has a main checkout. A replay worktree gets none: a
    /// replay measures a past commit with only the inputs it links itself.
    pub fn find(dir: &Path) -> Result<Option<Self>, String> {
        Ok(match locate(dir)? {
            Location::Linked {
                main,
                replay: false,
            } => Some(Self {
                here: dir.to_path_buf(),
                main,
            }),
            Location::Linked { replay: true, .. } | Location::NotLinked | Location::Bare => None,
        })
    }

    /// Those of `paths` that git ignores in the worktree and that exist in the
    /// main checkout. Each path is relative to the worktree directory, below
    /// it, and missing there. A tracked path is never ignored, so a tracked
    /// file deleted in the worktree stays missing. The only access to the main
    /// checkout is one existence check per ignored path.
    pub fn resolve<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> Result<BTreeSet<String>, String> {
        let ignored = self.ignored(paths)?;
        Ok(ignored
            .into_iter()
            .filter(|p| self.main.join(p).exists())
            .collect())
    }

    /// `path` in the main checkout, when it is missing in the worktree, git
    /// ignores it there, and it exists in the main checkout.
    pub fn file(&self, rel: &str) -> Result<Option<PathBuf>, String> {
        if self.here.join(rel).exists() {
            return Ok(None);
        }
        Ok(self.resolve([rel])?.pop_first().map(|p| self.main.join(p)))
    }

    /// `rel` in the main checkout when git ignores it in the worktree: for
    /// state that every checkout of the project shares, such as the search
    /// trace. It may not exist yet.
    pub fn shared(&self, rel: &str) -> Result<Option<PathBuf>, String> {
        Ok(self.ignored([rel])?.pop_first().map(|p| self.main.join(p)))
    }

    /// `rel` in the main checkout, for a path that `resolve` returned.
    pub fn main_file(&self, rel: &str) -> PathBuf {
        self.main.join(rel)
    }

    /// The ignored ones of `paths`, from one `git check-ignore --stdin`.
    fn ignored<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> Result<BTreeSet<String>, String> {
        let mut input = Vec::new();
        for p in paths {
            input.extend_from_slice(p.as_bytes());
            input.push(0);
        }
        if input.is_empty() {
            return Ok(BTreeSet::new());
        }
        let mut child = Command::new("git")
            .args(["check-ignore", "-z", "--stdin"])
            .current_dir(&self.here)
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("git check-ignore: {e}"))?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        // Written from another thread, so a full stdout pipe cannot block both sides.
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let out = child
            .wait_with_output()
            .map_err(|e| format!("git check-ignore: {e}"))?;
        writer
            .join()
            .expect("the writer thread does not panic")
            .map_err(|e| format!("git check-ignore: {e}"))?;
        // 0: some paths are ignored, 1: none is.
        match out.status.code() {
            Some(0) => Ok(out
                .stdout
                .split(|&b| b == 0)
                .filter(|p| !p.is_empty())
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .collect()),
            Some(1) => Ok(BTreeSet::new()),
            _ => Err(format!(
                "git check-ignore: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        }
    }
}
