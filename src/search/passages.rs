//! Units of plain text (scopes `texts` and `sessions`): passages of about
//! `WINDOW_WORDS` words, cut at line ends within spans of lines.

use super::units::Unit;

/// A passage of plain text holds about this many words.
pub const WINDOW_WORDS: usize = 200;

/// A span of lines of a text, 1-based and inclusive, under one heading.
pub struct Span {
    pub first: usize,
    pub last: usize,
    pub heading: String,
}

/// Lines joined for matching: a word that a hyphen split at a line end is one
/// word again (a compound such as `state-of-` keeps its hyphen), and the
/// lines of a paragraph run on. Line numbers stay with the passage, not with
/// this text.
pub fn unwrap<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let head = out.strip_suffix('-').filter(|head| {
            head.chars().next_back().is_some_and(char::is_alphabetic)
                && line.chars().next().is_some_and(char::is_lowercase)
        });
        if let Some(head) = head {
            let word = head.rsplit(char::is_whitespace).next().unwrap_or("");
            if !word.contains('-') {
                out.pop();
            }
        } else if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line);
    }
    out
}

/// Passages of about `WINDOW_WORDS` words within each span, cut at line ends.
/// Lines outside every span are left out. A last passage of a span with fewer
/// than a quarter of the words joins the passage before it.
pub fn windows(text: &str, spans: &[Span]) -> Vec<Unit> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut units: Vec<Unit> = Vec::new();
    for span in spans {
        let last = span.last.min(lines.len());
        if span.first == 0 || span.first > last {
            continue;
        }
        let first_unit = units.len();
        let mut start = span.first;
        let mut words = 0;
        for n in span.first..=last {
            words += lines[n - 1].split_whitespace().count();
            if words < WINDOW_WORDS && n < last {
                continue;
            }
            if words > 0 {
                let from = if words < WINDOW_WORDS / 4 && units.len() > first_unit {
                    units.pop().map_or(start, |u| u.start)
                } else {
                    start
                };
                units.push(Unit {
                    start: from,
                    end: n,
                    heading: span.heading.clone(),
                    body: unwrap(lines[from - 1..n].iter().copied()),
                });
            }
            start = n + 1;
            words = 0;
        }
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_words_split_at_a_line_end_and_unwraps_lines() {
        assert_eq!(
            unwrap(["Dense re-", "trieval helps  ", "", "Finance-", "Bench too"]),
            "Dense retrieval helps Finance- Bench too"
        );
        assert_eq!(unwrap(["state-of-", "the-art"]), "state-of-the-art");
        assert_eq!(unwrap(["2-", "x"]), "2- x");
    }

    #[test]
    fn cuts_spans_into_windows_at_line_ends_and_keeps_raw_line_numbers() {
        // 30 lines of 10 words: windows of 20 lines (200 words), then 10 lines.
        let text: String = (0..30).map(|_| "w ".repeat(10) + "\n").collect();
        let spans = [Span {
            first: 1,
            last: 30,
            heading: "Body".into(),
        }];
        let ranges: Vec<(usize, usize)> = windows(&text, &spans)
            .iter()
            .map(|u| (u.start, u.end))
            .collect();
        assert_eq!(ranges, [(1, 20), (21, 30)]);
        // A short tail joins the window before it.
        let spans = [Span {
            first: 1,
            last: 23,
            heading: String::new(),
        }];
        let units = windows(&text, &spans);
        assert_eq!((units.len(), units[0].start, units[0].end), (1, 1, 23));
    }

    #[test]
    fn leaves_out_lines_outside_the_spans_and_empty_spans() {
        let text = "a\nb\n\nc\nd\n";
        let spans = [
            Span {
                first: 1,
                last: 2,
                heading: "A".into(),
            },
            Span {
                first: 3,
                last: 3,
                heading: "empty".into(),
            },
            Span {
                first: 5,
                last: 9,
                heading: "D".into(),
            },
        ];
        let seen: Vec<(usize, usize, String, String)> = windows(text, &spans)
            .into_iter()
            .map(|u| (u.start, u.end, u.heading, u.body))
            .collect();
        assert_eq!(
            seen,
            [
                (1, 2, "A".into(), "a b".into()),
                (5, 6, "D".into(), "d".into())
            ]
        );
    }
}
