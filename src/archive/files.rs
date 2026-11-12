//! Glob-selected files under the project root, found in one pruned walk.
//!
//! Globs use `globset` semantics with `*` never crossing `/` and `**` matching
//! any depth. The walk enters a directory only when some glob can still match
//! below it, so a glob such as `docs/followups/*/README.md` never lists the
//! data directories inside each follow-up.

use std::path::{Path, PathBuf};

use globset::{Glob, GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};

fn glob(pattern: &str) -> Result<Glob, String> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|e| format!("glob `{pattern}`: {e}"))
}

fn glob_set<'a>(patterns: impl IntoIterator<Item = &'a str>) -> Result<GlobSet, String> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(glob(p)?);
    }
    b.build().map_err(|e| e.to_string())
}

/// The path components of a glob: a matcher per component, or `None` from the
/// first component that can match any number of directories (`**`, or a brace
/// alternative that may contain `/`).
fn components(pattern: &str) -> Result<Vec<Option<GlobMatcher>>, String> {
    let mut parts = Vec::new();
    for part in pattern.split('/') {
        if part.contains("**") || part.contains('{') {
            parts.push(None);
            break;
        }
        parts.push(Some(glob(part)?.compile_matcher()));
    }
    Ok(parts)
}

/// Files matching any `include` glob and no `exclude` glob.
#[derive(Debug)]
pub struct FileSet {
    include: GlobSet,
    exclude: GlobSet,
    parts: Vec<Vec<Option<GlobMatcher>>>,
    /// Directories excluded with everything below them (from `<dir>/**` excludes).
    pruned: GlobSet,
}

impl FileSet {
    pub fn new(include: &[String], exclude: &[String]) -> Result<Self, String> {
        Ok(Self {
            include: glob_set(include.iter().map(String::as_str))?,
            exclude: glob_set(exclude.iter().map(String::as_str))?,
            parts: include
                .iter()
                .map(|p| components(p))
                .collect::<Result<_, _>>()?,
            pruned: glob_set(exclude.iter().filter_map(|p| p.strip_suffix("/**")))?,
        })
    }

    pub fn matches(&self, rel: &str) -> bool {
        self.include.is_match(rel) && !self.exclude.is_match(rel)
    }

    /// Whether an `include` or `exclude` glob names `rel`: the set decides it.
    pub fn names(&self, rel: &str) -> bool {
        self.include.is_match(rel) || self.exclude.is_match(rel)
    }

    /// Whether a file below the directory `rel` (components `dir`) can match.
    fn enters(&self, rel: &str, dir: &[&str]) -> bool {
        if self.pruned.is_match(rel) {
            return false;
        }
        self.parts.iter().any(|parts| {
            for (i, name) in dir.iter().enumerate() {
                match parts.get(i) {
                    Some(None) => return true,
                    Some(Some(m)) if m.is_match(name) => {}
                    _ => return false,
                }
            }
            dir.len() < parts.len()
        })
    }
}

/// The files under `root` matching each set, as sorted `/`-separated paths
/// relative to `root`. Symbolic links to directories and `.git` are not
/// entered; a symbolic link to a file counts only when it resolves inside `root`.
pub fn walk(root: &Path, sets: &[&FileSet]) -> Result<Vec<Vec<String>>, String> {
    let mut found = vec![Vec::new(); sets.len()];
    let canonical_root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            let Ok(name) = entry.file_name().into_string() else {
                continue; // A name that is not UTF-8 cannot match a glob.
            };
            let child = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let kind = entry
                .file_type()
                .map_err(|e| format!("{}: {e}", entry.path().display()))?;
            let is_file = if kind.is_dir() {
                let parts: Vec<&str> = child.split('/').collect();
                if name != ".git" && sets.iter().any(|s| s.enters(&child, &parts)) {
                    stack.push((entry.path(), child));
                }
                continue;
            } else if kind.is_symlink() {
                let path = entry.path();
                path.canonicalize()
                    .is_ok_and(|t| t.starts_with(&canonical_root) && t.is_file())
            } else {
                kind.is_file()
            };
            if is_file {
                for (set, out) in sets.iter().zip(&mut found) {
                    if set.matches(&child) {
                        out.push(child.clone());
                    }
                }
            }
        }
    }
    for list in &mut found {
        list.sort();
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(include: &[&str], exclude: &[&str]) -> FileSet {
        let s = |v: &[&str]| v.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        FileSet::new(&s(include), &s(exclude)).unwrap()
    }

    #[test]
    fn matches_with_literal_separators_and_prunes_directories() {
        let s = set(&["docs/followups/*/README.md"], &[]);
        assert!(s.matches("docs/followups/2026-01-01-x/README.md"));
        assert!(!s.matches("docs/followups/a/b/README.md"));
        assert!(s.enters("docs/followups/a", &["docs", "followups", "a"]));
        assert!(!s.enters("docs/followups/a/data", &["docs", "followups", "a", "data"]));
        assert!(!s.enters("src", &["src"]));

        let deep = set(&["docs/**/*.md"], &["docs/a/inputs/**"]);
        assert!(deep.matches("docs/x.md") && deep.matches("docs/a/b/c.md"));
        assert!(!deep.matches("docs/a/inputs/c.md"));
        assert!(deep.enters("docs/a/b", &["docs", "a", "b"]));
        assert!(!deep.enters("docs/a/inputs", &["docs", "a", "inputs"]));
    }

    #[test]
    fn walks_only_matching_files() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        for f in [
            "AGENTS.md",
            "docs/a.md",
            "docs/f/x/README.md",
            "docs/f/x/data/README.md",
            "docs/f/x/notes.txt",
        ] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), "x").unwrap();
        }
        let readmes = set(&["docs/f/*/README.md"], &[]);
        let all = set(&["docs/**/*.md", "AGENTS.md"], &["docs/f/*/data/**"]);
        let found = walk(d, &[&readmes, &all]).unwrap();
        assert_eq!(found[0], ["docs/f/x/README.md"]);
        assert_eq!(found[1], ["AGENTS.md", "docs/a.md", "docs/f/x/README.md"]);
    }
}
