//! Units of the `notes` scope: one per Markdown section, from a heading to
//! the line before the next heading, with the heading path as its context.

use yaml_serde::Value;

use crate::archive::{front, markdown};

/// A section longer than this many lines is cut into parts.
const MAX_LINES: usize = 120;
/// A part ends at a blank line when one lies within this many lines of the limit.
const CUT_WINDOW: usize = 40;

/// One searchable passage of a file.
#[derive(Debug, PartialEq)]
pub struct Unit {
    /// 1-based line numbers of the source file, both inclusive.
    pub start: usize,
    pub end: usize,
    /// The headings above the passage, outermost first, joined by ` > `.
    pub heading: String,
    /// The text below the heading line.
    pub body: String,
}

/// The sections of `text`, whose Markdown starts on file line `body_line`
/// (after the front matter). The front matter opens the first unit, the
/// record's head: it starts on line 1 and holds the front-matter lines and any
/// text before the first heading. Headings inside fenced code are text. A
/// section without text below its heading is not a unit.
pub fn sections(text: &str, body_line: usize) -> Vec<Unit> {
    let mut units = Vec::new();
    let mut path: Vec<(usize, String)> = Vec::new();
    let mut heading = String::new();
    // The section being read: its heading line and the lines below it.
    let mut start = None;
    let mut lines: Vec<(usize, &str)> = Vec::new();
    if body_line > 1 {
        // The lines between the `---` fences on line 1 and line `body_line - 1`.
        start = Some(1);
        lines.extend(
            text.split('\n')
                .enumerate()
                .take(body_line - 2)
                .skip(1)
                .map(|(i, l)| (i + 1, l.strip_suffix('\r').unwrap_or(l))),
        );
    }
    let mut fence = None;
    for (i, line) in text.split('\n').enumerate().skip(body_line - 1) {
        let lineno = i + 1;
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(open) = fence {
            if markdown::fence_closes(line, open) {
                fence = None;
            }
        } else if let Some(open) = markdown::fence_open(line) {
            fence = Some(open);
        } else if let Some((level, raw)) = markdown::atx(line) {
            push(&mut units, start, &heading, &lines);
            lines.clear();
            path.retain(|(l, _)| *l < level);
            path.push((level, markdown::heading_text(raw)));
            heading = path
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join(" > ");
            start = Some(lineno);
            continue;
        }
        lines.push((lineno, line));
    }
    push(&mut units, start, &heading, &lines);
    units
}

/// Adds the section under the heading on line `start` as one unit, or as
/// several when it is long.
fn push(units: &mut Vec<Unit>, start: Option<usize>, heading: &str, lines: &[(usize, &str)]) {
    let blank = |l: &(usize, &str)| l.1.trim().is_empty();
    let mut rest = lines;
    let mut start = start;
    loop {
        let cut = if rest.len() > MAX_LINES {
            (MAX_LINES - CUT_WINDOW..MAX_LINES)
                .rev()
                .find(|&i| blank(&rest[i]))
                .unwrap_or(MAX_LINES)
        } else {
            rest.len()
        };
        let (part, tail) = rest.split_at(cut);
        if let Some(last) = part.iter().rposition(|l| !blank(l)) {
            let first = part.iter().position(|l| !blank(l)).unwrap_or(0);
            units.push(Unit {
                // A heading line belongs to its section's first part.
                start: start.take().unwrap_or(part[first].0),
                end: part[last].0,
                heading: heading.to_string(),
                body: part[first..=last]
                    .iter()
                    .map(|l| l.1)
                    .collect::<Vec<_>>()
                    .join("\n"),
            });
        }
        if tail.is_empty() {
            break;
        }
        rest = tail;
    }
}

/// The front-matter keys `task` and `kind`, when they are strings.
pub fn task_and_kind(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let Some(yaml) = front::split(bytes).yaml else {
        return (None, None);
    };
    let Ok(Value::Mapping(map)) = yaml_serde::from_slice::<Value>(yaml) else {
        return (None, None);
    };
    let field = |name: &str| {
        map.get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    (field("task"), field("kind"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_headings_with_file_line_numbers_and_heading_paths() {
        let text = "---\nid: D1\n---\n# Title\n\nIntro.\n\n## Method\n\nA.\nB.\n\n### Detail\n\nC.\n\n## Result\n\n```\n# not a heading\n```\n";
        let units = sections(text, 4);
        let seen: Vec<(usize, usize, &str, &str)> = units
            .iter()
            .map(|u| (u.start, u.end, u.heading.as_str(), u.body.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                (1, 2, "", "id: D1"),
                (4, 6, "Title", "Intro."),
                (8, 11, "Title > Method", "A.\nB."),
                (13, 15, "Title > Method > Detail", "C."),
                (17, 21, "Title > Result", "```\n# not a heading\n```"),
            ]
        );
    }

    #[test]
    fn text_before_the_first_heading_is_a_unit_and_empty_sections_are_not() {
        let units = sections("\nLead.\n\n# Empty\n\n# Full\nx\n", 1);
        assert_eq!(units.len(), 2);
        assert_eq!((units[0].start, units[0].end), (2, 2));
        assert_eq!(units[0].heading, "");
        assert_eq!((units[1].start, units[1].end), (6, 7));
    }

    #[test]
    fn the_front_matter_and_the_lead_text_are_one_head_unit_from_line_1() {
        let units = sections("---\nid: D1\ntask: rf-9\n---\nLead.\n\n# T\nx\n", 5);
        let seen: Vec<(usize, usize, &str, &str)> = units
            .iter()
            .map(|u| (u.start, u.end, u.heading.as_str(), u.body.as_str()))
            .collect();
        assert_eq!(
            seen,
            [(1, 5, "", "id: D1\ntask: rf-9\nLead."), (7, 8, "T", "x"),]
        );
        // Empty front matter makes no unit.
        assert_eq!(sections("---\n---\n# T\nx\n", 3)[0].start, 3);
    }

    #[test]
    fn a_long_section_is_cut_at_a_blank_line_near_the_limit() {
        let mut text = String::from("# Long\n");
        for i in 0..300 {
            text.push_str(if i % 50 == 49 { "\n" } else { "row\n" });
        }
        let units = sections(&text, 1);
        let ranges: Vec<(usize, usize)> = units.iter().map(|u| (u.start, u.end)).collect();
        // Blank lines are file lines 51, 101, 151, 201, 251 and 301.
        assert_eq!(ranges, [(1, 100), (102, 200), (202, 300)]);
        assert!(units.iter().all(|u| u.heading == "Long"));
    }

    #[test]
    fn reads_task_and_kind_as_optional_strings() {
        assert_eq!(
            task_and_kind(b"---\ntask: rf-1\nkind: result\n---\nx"),
            (Some("rf-1".into()), Some("result".into()))
        );
        assert_eq!(task_and_kind(b"---\ntask: 12\n---\nx"), (None, None));
        assert_eq!(task_and_kind(b"# no front matter\n"), (None, None));
    }
}
