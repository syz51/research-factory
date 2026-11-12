//! `lab archive next --reserve`: gives each caller its own number.
//!
//! Two linked worktrees that each write a new record would both take the same
//! next number, because neither sees the other's record. A reserved number is
//! therefore recorded in the main checkout's ignored state directory, under a
//! lock. The next number is one more than the largest of: the ids in this
//! checkout, in the other worktrees' work trees (the main checkout among
//! them) and on the default branch, and the numbers given out before.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Spec, Texts, gitseal, index, read_optional, write_atomically};
use crate::config::Project;
use crate::lesson_loop::{Checkout, checkout};

const SCHEMA: &str = "rf.archive-reserved/1";

/// The numbers given out, per id prefix.
#[derive(Serialize, Deserialize)]
struct Reserved {
    schema: String,
    #[serde(default)]
    series: BTreeMap<String, Series>,
}

#[derive(Serialize, Deserialize)]
struct Series {
    /// The largest number given out.
    max: u64,
    /// The width of the zero padding, 0 for none.
    #[serde(default)]
    width: usize,
}

/// The record ids of `project`.
fn ids(project: &Project) -> Result<Vec<String>, String> {
    let spec = Spec::new(project)?;
    let found = spec.walk(&[])?;
    let records = spec.records(&found, &mut Texts::new())?;
    Ok(records.into_iter().filter_map(|r| r.meta.id).collect())
}

/// The record ids that this checkout may lack: those on the default branch,
/// as its local ref has them, and those in the work tree of each other
/// worktree, the main checkout included, with the same config file. A
/// worktree whose project does not load is skipped with a note: it is
/// another thread's work in progress.
pub(super) fn elsewhere(spec: &Spec, config: Option<&Path>) -> Result<Vec<String>, String> {
    let root = &spec.project.root;
    let mut all = Vec::new();
    if let Some(branch) = crate::worktree::default_branch(root)? {
        all.extend(gitseal::ids_at(spec, &branch)?);
    }
    let name = config
        .and_then(Path::file_name)
        .unwrap_or(crate::config::CONFIG_FILE.as_ref());
    for dir in crate::worktree::others(root)? {
        let file = dir.join(name);
        if !file.is_file() {
            continue;
        }
        let read = Project::discover(&dir, Some(&file)).and_then(|p| {
            if p.lab.archive.is_some() {
                ids(&p)
            } else {
                Ok(Vec::new())
            }
        });
        match read {
            Ok(ids) => all.extend(ids),
            Err(e) => eprintln!(
                "note: the records of {} are not counted: {e}",
                dir.display()
            ),
        }
    }
    Ok(all)
}

/// A file that holds an exclusive lock until it is dropped.
fn lock(path: &Path) -> Result<File, String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let file = File::options()
        .create(true)
        .append(true)
        .open(path)
        .map_err(err)?;
    file.lock().map_err(err)?;
    Ok(file)
}

/// The next id with `prefix` that no checkout has and no caller was given,
/// reserved when `record` is set; `None` when no id has the prefix and no
/// `width` starts a series.
pub(super) fn reserve(
    project: &Project,
    config: Option<&Path>,
    prefix: &str,
    width: Option<usize>,
    record: bool,
) -> Result<Option<String>, String> {
    let spec = Spec::new(project)?;
    let mut all = ids(project)?;
    // Other worktrees, the main checkout among them, and the default branch
    // can hold records that this worktree's branch lacks.
    all.extend(elsewhere(&spec, config)?);
    let main = match checkout(project, config)? {
        Checkout::This => None,
        Checkout::Main(main) => Some(main),
        Checkout::Skip(why) => {
            eprintln!(
                "note: {why}; the number is reserved in this worktree only, so another \
                 worktree can get the same one"
            );
            None
        }
    };
    let home = main.as_deref().unwrap_or(project);
    let path = home.reserved_ids();
    let _lock = lock(&path.with_extension("lock"))?;
    // A file that does not parse is from another version; its numbers are lost,
    // which the ids of the two checkouts bound from below.
    let mut reserved = read_optional(&path)?
        .and_then(|b| serde_json::from_slice::<Reserved>(&b).ok())
        .filter(|r| r.schema == SCHEMA)
        .unwrap_or_else(|| Reserved {
            schema: SCHEMA.into(),
            series: BTreeMap::new(),
        });
    let (seen, pad) = index::series(all.iter().map(String::as_str), prefix);
    let given = reserved.series.get(prefix);
    let max = seen.max(given.map(|s| s.max));
    let next = match (max, width) {
        (Some(m), _) => m + 1,
        (None, Some(_)) => 1,
        (None, None) => return Ok(None),
    };
    let width = width.unwrap_or_else(|| pad.max(given.map_or(0, |s| s.width)));
    if record {
        reserved
            .series
            .insert(prefix.into(), Series { max: next, width });
        let text = serde_json::to_string_pretty(&reserved).map_err(|e| e.to_string())?;
        write_atomically(&path, format!("{text}\n").as_bytes())?;
    }
    Ok(Some(format!("{prefix}{next:0width$}")))
}
