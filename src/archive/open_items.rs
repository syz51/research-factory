//! The `open-item` rule: in a record of a board task (a file with a `task`
//! front-matter field), every list item under an open-item heading, such as
//! "Limits" or "Open questions", names the board task that holds it or says
//! `no action:` and why. It checks that an id is written, not that the task
//! exists or covers the item.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use yaml_serde::Value;

use super::markdown::{self, atx, fence_closes, fence_open, heading_text};
use super::rules::Problem;
use super::{Spec, Texts, front};
use crate::config::OpenItemsSpec;
use crate::finding::Severity;

/// `[lab.archive.open_items]`, validated and compiled.
pub(super) struct OpenItems {
    since: String,
    /// Lowercased, without a trailing colon.
    headings: Vec<String>,
    ids: Regex,
}

/// A list item that names no task, at file line `line`.
#[derive(Debug, PartialEq)]
pub(super) struct Miss {
    pub line: usize,
    /// The heading of the section that holds it, as written.
    pub section: String,
    /// The item's text, shortened.
    pub excerpt: String,
}

static BOARD_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_-]*$").expect("valid regex"));
static ITEM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^( *)(?:[-*+]|(\d{1,9})[.)])(?: +|$)(.*)$").expect("valid regex")
});
/// A thematic break, such as `* * *` or `---`.
static BREAK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ {0,3}(?:(?:\* *){3,}|(?:- *){3,}|(?:_ *){3,})$").expect("valid regex")
});
static NO_ACTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bno action:\s*\S").expect("valid regex"));

fn normalize(heading: &str) -> String {
    heading
        .trim()
        .trim_end_matches(':')
        .trim_end()
        .to_lowercase()
}

impl OpenItems {
    pub(super) fn new(spec: &OpenItemsSpec) -> Result<Self, String> {
        let err = |e: &str| format!("[lab.archive.open_items] {e}");
        if !front::valid_date(&spec.since) {
            return Err(err(&format!(
                "`since` must be YYYY-MM-DD, got `{}`",
                spec.since
            )));
        }
        if spec.boards.is_empty() {
            return Err(err("`boards` must name at least one board"));
        }
        if let Some(b) = spec.boards.iter().find(|b| !BOARD_NAME.is_match(b)) {
            return Err(err(&format!(
                "board name `{b}` must be letters, digits, `-` and `_`, starting with a letter or digit"
            )));
        }
        let headings: Vec<String> = spec.headings.iter().map(|h| normalize(h)).collect();
        if headings.is_empty() || headings.iter().any(String::is_empty) {
            return Err(err(
                "`headings` must be a non-empty list of non-empty headings",
            ));
        }
        let boards = spec
            .boards
            .iter()
            .map(|b| regex::escape(b))
            .collect::<Vec<_>>()
            .join("|");
        Ok(Self {
            since: spec.since.clone(),
            headings,
            // Not inside a longer name: `old-masters-3` names no `masters` task.
            ids: Regex::new(&format!(r"(?:^|[^A-Za-z0-9_-])(?:{boards})-\d+\b"))
                .expect("escaped names form a valid regex"),
        })
    }

    /// Whether the file at `path` holding `bytes` is a record of a board task
    /// dated on or after `since`: its front matter is a mapping with a
    /// non-empty `task` field. The date is the front matter's valid `date`,
    /// else the first date in the path; an undated record is checked.
    pub(super) fn applies(&self, path: &str, bytes: &[u8]) -> bool {
        let Some(yaml) = front::split(bytes).yaml else {
            return false;
        };
        let Ok(Value::Mapping(map)) = yaml_serde::from_slice::<Value>(yaml) else {
            return false;
        };
        let has_task = match map.get("task") {
            None | Some(Value::Null) => false,
            Some(Value::String(s)) => !s.trim().is_empty(),
            Some(Value::Sequence(s)) => !s.is_empty(),
            Some(_) => true,
        };
        let date = match map.get("date") {
            Some(Value::String(d)) if front::valid_date(d.trim()) => Some(d.trim().to_string()),
            _ => front::date_in(path),
        };
        has_task && date.is_none_or(|d| d >= self.since)
    }

    /// An `open-item` error for each miss in the records among the files
    /// that `found` lists.
    pub(super) fn problems(
        &self,
        spec: &Spec,
        found: &[Vec<String>],
        texts: &mut Texts,
        out: &mut Vec<Problem>,
    ) -> Result<(), String> {
        let outputs: BTreeSet<&str> = spec.cfg.index.iter().map(|i| i.output.as_str()).collect();
        for path in scanned(found, &outputs) {
            let bytes = texts.bytes(&spec.root().join(path))?;
            if !self.applies(path, &bytes) {
                continue;
            }
            let split = front::split(&bytes);
            let body = String::from_utf8_lossy(split.body);
            for m in self.misses(&body, split.body_line) {
                let mut p = Problem::new(
                    "open-item",
                    Severity::Error,
                    path,
                    format!(
                        "an item under \"{}\" names no board task: add the id of the task that \
                         holds it, or `no action: <reason>`",
                        m.section
                    ),
                )
                .at(m.line);
                p.data = Some(serde_json::json!({"section": m.section}));
                p.detail = Some(m.excerpt);
                out.push(p);
            }
        }
        Ok(())
    }

    fn passes(&self, text: &str) -> bool {
        let plain: String = text.chars().filter(|c| !matches!(c, '*' | '`')).collect();
        self.ids.is_match(&plain) || NO_ACTION.is_match(&plain)
    }

    /// The list items under an open-item heading in `body`, whose first line
    /// is file line `first_line`, that name no task. A heading matches by
    /// its visible text, case-insensitive, at any level, with a trailing colon
    /// ignored; only ATX headings (`## Limits`) count. The section runs to the
    /// next heading of the same or higher level. An item passes when its own
    /// text or that of an item it is nested in names a task id or says
    /// `no action:` with a reason; an item with nested items is checked
    /// through them. Fenced code and HTML comments are skipped.
    pub(super) fn misses(&self, body: &str, first_line: usize) -> Vec<Miss> {
        let mut out = Vec::new();
        let mut fence = None;
        let mut in_comment = false;
        // The level and text of the open-item section we are in.
        let mut section: Option<(usize, String)> = None;
        let mut list = List::default();
        for (i, line) in body.split('\n').enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let lineno = first_line + i;
            if let Some(open) = fence {
                if fence_closes(line, open) {
                    fence = None;
                }
                continue;
            }
            if !in_comment && let Some(open) = fence_open(line) {
                fence = Some(open);
                continue;
            }
            let visible = markdown::strip_comments(line, in_comment);
            markdown::mask(line, &mut in_comment);
            if let Some((level, text)) = atx(&visible) {
                list.close(self, section.as_ref(), &mut out);
                if section.as_ref().is_some_and(|(l, _)| level <= *l) {
                    section = None;
                }
                let text = heading_text(text);
                if section.is_none() && self.headings.contains(&normalize(&text)) {
                    section = Some((level, text.trim().to_string()));
                }
                continue;
            }
            if section.is_none() {
                continue;
            }
            if visible.trim().is_empty() {
                list.blank = true;
                list.paragraph = false;
                continue;
            }
            // A list that interrupts a paragraph starts with `-`, `*`, `+`
            // or `1.`, as in CommonMark; so `2026. The pool` there is text.
            let item = ITEM.captures(&visible).filter(|c| {
                !BREAK.is_match(&visible)
                    && !(list.stack.is_empty()
                        && list.paragraph
                        && c.get(2)
                            .is_some_and(|n| n.as_str().trim_start_matches('0') != "1"))
            });
            list.paragraph = item.is_none() && list.stack.is_empty();
            if let Some(c) = item {
                let indent = c[1].len();
                list.push(self, section.as_ref(), &mut out, indent, lineno, &c[3]);
            } else if BREAK.is_match(&visible) {
                list.close(self, section.as_ref(), &mut out);
            } else if list.continues(&visible) {
                list.append(visible.trim());
            } else {
                list.close(self, section.as_ref(), &mut out);
            }
            list.blank = false;
        }
        list.close(self, section.as_ref(), &mut out);
        out
    }
}

struct Item {
    line: usize,
    indent: usize,
    /// Its own text, joined over continuation lines.
    text: String,
    /// The text of the items it is nested in.
    outer: String,
    has_children: bool,
}

/// The list being read, as a stack from the outermost open item inwards.
#[derive(Default)]
struct List {
    stack: Vec<Item>,
    blank: bool,
    /// The previous line is paragraph text outside a list.
    paragraph: bool,
}

impl List {
    /// Pops and checks every item deeper than or as deep as `indent`.
    fn pop_to(
        &mut self,
        rule: &OpenItems,
        section: Option<&(usize, String)>,
        out: &mut Vec<Miss>,
        indent: Option<usize>,
    ) {
        while let Some(top) = self.stack.last() {
            if indent.is_some_and(|k| top.indent < k) {
                break;
            }
            let item = self.stack.pop().expect("the stack is not empty");
            if !item.has_children && !rule.passes(&format!("{} {}", item.outer, item.text)) {
                let excerpt: String = item.text.chars().take(60).collect();
                out.push(Miss {
                    line: item.line,
                    section: section.map_or_else(String::new, |(_, s)| s.clone()),
                    excerpt,
                });
            }
        }
    }

    fn close(&mut self, rule: &OpenItems, section: Option<&(usize, String)>, out: &mut Vec<Miss>) {
        self.pop_to(rule, section, out, None);
        self.blank = false;
    }

    fn push(
        &mut self,
        rule: &OpenItems,
        section: Option<&(usize, String)>,
        out: &mut Vec<Miss>,
        indent: usize,
        line: usize,
        text: &str,
    ) {
        self.pop_to(rule, section, out, Some(indent));
        let outer = match self.stack.last_mut() {
            Some(parent) => {
                parent.has_children = true;
                format!("{} {}", parent.outer, parent.text)
            }
            None => String::new(),
        };
        self.stack.push(Item {
            line,
            indent,
            text: text.trim().to_string(),
            outer,
            has_children: false,
        });
    }

    /// Whether a line that is not an item continues the innermost item: a
    /// lazy continuation, or an indented line after a blank line.
    fn continues(&self, line: &str) -> bool {
        let Some(top) = self.stack.last() else {
            return false;
        };
        let indent = line.len() - line.trim_start_matches(' ').len();
        !self.blank || indent > top.indent
    }

    fn append(&mut self, text: &str) {
        if let Some(top) = self.stack.last_mut() {
            top.text.push(' ');
            top.text.push_str(text);
        }
    }
}

/// The paths that `lab archive check` scans in any of its file lists, once
/// each, without index outputs and files that are not Markdown.
fn scanned<'a>(found: &'a [Vec<String>], outputs: &BTreeSet<&str>) -> BTreeSet<&'a str> {
    found
        .iter()
        .flatten()
        .map(String::as_str)
        .filter(|p| p.ends_with(".md") && !outputs.contains(p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> OpenItems {
        OpenItems::new(&OpenItemsSpec {
            since: "2026-11-12".into(),
            boards: vec!["research-factory".into(), "masters".into()],
            headings: crate::config::default_open_item_headings(),
        })
        .unwrap()
    }

    fn lines(body: &str) -> Vec<usize> {
        rule().misses(body, 1).iter().map(|m| m.line).collect()
    }

    #[test]
    fn items_need_an_id_or_a_no_action_reason() {
        let body = "# T\n\n## Limits\n\n- names research-factory-12\n- **No action:** a rule\n- names nothing\n- no action:\n- `masters-3` in code\n\n## Method\n\n- not checked\n";
        assert_eq!(lines(body), vec![7, 8]);
        let m = &rule().misses(body, 1)[0];
        assert_eq!(m.section, "Limits");
        assert_eq!(m.excerpt, "names nothing");
    }

    #[test]
    fn continuation_lines_and_nesting_count() {
        let body = "## Open questions:\n\
            1. wraps onto\n   a second line with research-factory-4\n\
            2. lazy\ncontinuation masters-9\n\
            - parent research-factory-1\n  - child covered by parent\n\
            - parent without id\n  - child research-factory-2\n  - child without\n\
            \n\
            Paragraph text is not an item.\n\
            - a later list without id\n";
        assert_eq!(lines(body), vec![10, 13]);
    }

    #[test]
    fn sections_end_at_a_heading_of_the_same_or_higher_level() {
        let body = "## Limits\n### Sub\n- in sub\n## Other\n- outside\n# Next steps\n## Deeper\n- still inside\n";
        assert_eq!(lines(body), vec![3, 8]);
    }

    #[test]
    fn code_comments_and_ids_of_other_boards_do_not_count() {
        let body = "## Limits\n```\n- in code\n```\n<!--\n- in a comment\n-->\n- other-board-5 and research-factory-x\n";
        assert_eq!(lines(body), vec![8]);
    }

    #[test]
    fn longer_names_breaks_and_numbered_text_are_not_items_or_ids() {
        let body = "## Limits\n- old-masters-3 only\n- (masters-4) passes\n\n* * *\n- - -\n\nA paragraph wraps in\n2026. The pool is text.\n\n1. but this is an item\n";
        assert_eq!(lines(body), vec![2, 11]);
    }

    #[test]
    fn applies_to_dated_records_of_a_task() {
        let r = rule();
        assert!(r.applies(
            "a.md",
            b"---\ntask: research-factory-1\ndate: 2026-11-12\n---\n"
        ));
        assert!(r.applies("2026-11-13-x/README.md", b"---\ntask: masters-1\n---\n"));
        assert!(r.applies("x.md", b"---\ntask: masters-1\n---\n"));
        assert!(!r.applies(
            "a.md",
            b"---\ntask: research-factory-1\ndate: 2026-11-11\n---\n"
        ));
        assert!(!r.applies("2026-11-11-x/README.md", b"---\ntask: masters-1\n---\n"));
        assert!(!r.applies("a.md", b"---\ndate: 2026-11-12\n---\n"));
        assert!(!r.applies("a.md", b"# no front matter\n"));
    }

    #[test]
    fn configuration_is_validated() {
        let spec = |since: &str, boards: &[&str], headings: &[&str]| OpenItemsSpec {
            since: since.into(),
            boards: boards.iter().map(|s| s.to_string()).collect(),
            headings: headings.iter().map(|s| s.to_string()).collect(),
        };
        assert!(OpenItems::new(&spec("2026-11-12", &["a"], &["Limits"])).is_ok());
        assert!(OpenItems::new(&spec("12 Nov", &["a"], &["Limits"])).is_err());
        assert!(OpenItems::new(&spec("2026-11-12", &[], &["Limits"])).is_err());
        assert!(OpenItems::new(&spec("2026-11-12", &["a b"], &["Limits"])).is_err());
        assert!(OpenItems::new(&spec("2026-11-12", &["a"], &[])).is_err());
        assert!(OpenItems::new(&spec("2026-11-12", &["a"], &[" : "])).is_err());
    }
}
