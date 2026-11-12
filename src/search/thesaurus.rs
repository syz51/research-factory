//! The thesaurus: a tracked file of equivalent terms that a query applies
//! (docs/state-machines.md 3.4). One entry per line, its terms separated by
//! `=`, such as `fiscal calendar = fiscal year = FY`. A line that starts with
//! `retired:` is kept but no longer applied; `#` starts a comment line.

use std::path::{Path, PathBuf};

use crate::config::Project;

use super::query::words;

/// The prefix of a retired entry.
pub const RETIRED: &str = "retired:";

/// One line of the thesaurus with at least two terms.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Each term as its lowercased words.
    pub terms: Vec<Vec<String>>,
    pub retired: bool,
    /// 1-based line of the file.
    pub line: usize,
}

impl Entry {
    /// The entry as `a = b c`, lowercased: how the trace names it.
    pub fn text(&self) -> String {
        self.terms
            .iter()
            .map(|t| t.join(" "))
            .collect::<Vec<_>>()
            .join(" = ")
    }
}

/// The thesaurus file `[lab.search] thesaurus` names, if any.
pub fn path(project: &Project) -> Option<PathBuf> {
    project
        .lab
        .search
        .thesaurus
        .as_deref()
        .map(|p| project.path(p))
}

/// Parses the thesaurus text. A line with fewer than two terms is an error
/// that names the line.
pub fn parse(text: &str, file: &Path) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (retired, body) = match trimmed.strip_prefix(RETIRED) {
            Some(rest) => (true, rest),
            None => (false, trimmed),
        };
        let mut terms: Vec<Vec<String>> = Vec::new();
        for term in body.split('=').map(words) {
            if !term.is_empty() && !terms.contains(&term) {
                terms.push(term);
            }
        }
        if terms.len() < 2 {
            return Err(format!(
                "{}:{}: a thesaurus entry needs at least two different terms separated by `=`",
                file.display(),
                i + 1
            ));
        }
        entries.push(Entry {
            terms,
            retired,
            line: i + 1,
        });
    }
    Ok(entries)
}

/// The entries of the configured thesaurus; none when no thesaurus is
/// configured or its file does not exist yet.
pub fn load(project: &Project) -> Result<Vec<Entry>, String> {
    let Some(file) = path(project) else {
        return Ok(Vec::new());
    };
    match std::fs::read_to_string(&file) {
        Ok(text) => parse(&text, &file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

const HEADER: &str = "# Equivalent search terms for `lab search`: one entry per line, terms \
                      separated by `=`.\n# `lab retro` proposes entries; a line that starts \
                      with `retired:` is no longer applied.\n";

/// Appends entry lines to the thesaurus, creating the file with a header.
/// Returns the file.
pub fn append(project: &Project, lines: &[String]) -> Result<PathBuf, String> {
    let file = path(project).ok_or(
        "no thesaurus configured: set `thesaurus = \"<path>\"` in [lab.search] of research.toml",
    )?;
    let mut text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HEADER.to_string(),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    // A line this writes must parse.
    parse(&text, &file)?;
    crate::archive::write_atomically(&file, text.as_bytes())?;
    Ok(file)
}

/// Marks the entry on `line` retired. Returns the entry's text.
pub fn retire(project: &Project, line: usize) -> Result<String, String> {
    let file = path(project).ok_or("no thesaurus configured in [lab.search]")?;
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let entries = parse(&text, &file)?;
    let entry = entries
        .iter()
        .find(|e| e.line == line)
        .ok_or_else(|| format!("{}:{line} holds no thesaurus entry", file.display()))?;
    if entry.retired {
        return Err(format!(
            "{}:{line} is retired already: {}",
            file.display(),
            entry.text()
        ));
    }
    let mut out = String::new();
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            out.push_str(RETIRED);
            out.push(' ');
            out.push_str(l.trim_start());
        } else {
            out.push_str(l);
        }
    }
    crate::archive::write_atomically(&file, out.as_bytes())?;
    Ok(entry.text())
}

/// Whether an active entry already holds both terms.
pub fn holds(entries: &[Entry], a: &str, b: &str) -> bool {
    let (a, b) = (words(a), words(b));
    entries
        .iter()
        .any(|e| !e.retired && e.terms.contains(&a) && e.terms.contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_comments_and_retired_lines() {
        let text = "# terms\nfiscal calendar = FY = fiscal year\n\nretired: old = new\n";
        let entries = parse(text, Path::new("t.txt")).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text(), "fiscal calendar = fy = fiscal year");
        assert_eq!(entries[0].line, 2);
        assert!(!entries[0].retired);
        assert!(entries[1].retired);
        assert_eq!(entries[1].line, 4);
        assert!(holds(&entries, "FY", "Fiscal Calendar"));
        assert!(!holds(&entries, "old", "new"));
    }

    #[test]
    fn refuses_an_entry_with_one_term() {
        let err = parse("a\nb = b\n", Path::new("t.txt")).unwrap_err();
        assert!(err.contains("t.txt:1"), "{err}");
    }
}
