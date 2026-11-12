//! Just enough Markdown for link checking: link destinations, heading slugs,
//! explicit anchors and the first title, outside fenced code, code spans and
//! HTML comments.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;

/// A link destination as written, with escapes removed.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub line: usize,
    pub dest: String,
}

#[derive(Debug, Default)]
pub struct Doc {
    pub links: Vec<Link>,
    /// Heading slugs and explicit anchor ids, lowercased.
    pub anchors: BTreeSet<String>,
    /// The text of the first ATX level-1 heading.
    pub title: Option<String>,
}

/// GitHub's slug of a heading's rendered text: lowercase, keep letters,
/// numbers, spaces, `-` and `_`, then turn each space into `-`.
pub fn slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .collect()
}

/// Decodes `%XX` escapes; an invalid escape stays as written.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && let (Some(h), Some(l)) = (
                b.get(i + 1).copied().and_then(hex),
                b.get(i + 2).copied().and_then(hex),
            )
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) {
            continue;
        }
        out.push(c);
    }
    out
}

/// A fence opening: its character and length, when `line` opens one.
pub fn fence_open(line: &str) -> Option<(u8, usize)> {
    let t = line.trim_start().as_bytes();
    let c = *t.first()?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let n = t.iter().take_while(|&&b| b == c).count();
    (n >= 3 && (c == b'~' || !t[n..].contains(&b'`'))).then_some((c, n))
}

pub fn fence_closes(line: &str, (c, n): (u8, usize)) -> bool {
    let t = line.trim().as_bytes();
    t.len() >= n && t.iter().all(|&b| b == c)
}

/// `line` with code spans and HTML comments replaced by spaces and backslash
/// escapes by `x`, byte for byte, so that positions still index into `line`.
fn mask(line: &str, in_comment: &mut bool) -> Vec<u8> {
    let b = line.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| out[from..to].fill(b' ');
    let mut i = 0;
    while i < b.len() {
        if *in_comment {
            match find(b, i, b"-->") {
                Some(j) => {
                    blank(&mut out, i, j + 3);
                    i = j + 3;
                    *in_comment = false;
                }
                None => {
                    blank(&mut out, i, b.len());
                    break;
                }
            }
            continue;
        }
        match b[i] {
            b'\\' if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) => {
                out[i..i + 2].fill(b'x');
                i += 2;
            }
            b'`' => {
                let n = b[i..].iter().take_while(|&&c| c == b'`').count();
                match code_span_end(b, i + n, n) {
                    Some(end) => {
                        blank(&mut out, i, end);
                        i = end;
                    }
                    None => i += n,
                }
            }
            b'<' if b[i..].starts_with(b"<!--") => {
                blank(&mut out, i, i + 4);
                i += 4;
                *in_comment = true;
            }
            _ => i += 1,
        }
    }
    out
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// The end of a code span opened by `n` backticks: just past the next run of
/// exactly `n` backticks.
fn code_span_end(b: &[u8], mut j: usize, n: usize) -> Option<usize> {
    while j < b.len() {
        if b[j] == b'`' {
            let m = b[j..].iter().take_while(|&&c| c == b'`').count();
            if m == n {
                return Some(j + m);
            }
            j += m;
        } else {
            j += 1;
        }
    }
    None
}

/// Parses an inline link's `(dest "title")` from just after `(`: the
/// destination's byte range and the position after `)`.
fn destination(m: &[u8], mut p: usize) -> Option<(usize, usize, usize)> {
    let skip = |p: &mut usize| {
        while m.get(*p).is_some_and(|&c| c == b' ' || c == b'\t') {
            *p += 1;
        }
    };
    skip(&mut p);
    let (ds, de);
    if m.get(p) == Some(&b'<') {
        ds = p + 1;
        de = ds + m[ds..].iter().position(|&c| c == b'>' || c == b'<')?;
        if m[de] != b'>' {
            return None;
        }
        p = de + 1;
    } else {
        ds = p;
        let mut depth = 0usize;
        while let Some(&c) = m.get(p) {
            match c {
                b' ' | b'\t' => break,
                b'(' => depth += 1,
                b')' if depth == 0 => break,
                b')' => depth -= 1,
                _ => {}
            }
            p += 1;
        }
        de = p;
    }
    skip(&mut p);
    if let Some(&q) = m.get(p).filter(|&&c| matches!(c, b'"' | b'\'' | b'(')) {
        let close = if q == b'(' { b')' } else { q };
        p += 1 + m.get(p + 1..)?.iter().position(|&c| c == close)? + 1;
        skip(&mut p);
    }
    (m.get(p) == Some(&b')')).then_some((ds, de, p + 1))
}

/// Index of the `]` matching the `[` at `i`.
fn closing_bracket(m: &[u8], i: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (j, &c) in m.iter().enumerate().skip(i) {
        match c {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

/// Inline links, outer ones before the images or links nested in their text.
fn inline_links(line: &str, m: &[u8], lineno: usize, out: &mut Vec<Link>) {
    // The `(dest "title")` part of the last link found, which holds no link.
    let mut skip = 0..0;
    for i in 0..m.len() {
        if m[i] != b'[' || skip.contains(&i) {
            continue;
        }
        let Some(j) = closing_bracket(m, i) else {
            continue;
        };
        if m.get(j + 1) == Some(&b'(')
            && let Some((ds, de, end)) = destination(m, j + 2)
        {
            out.push(Link {
                line: lineno,
                dest: unescape(&line[ds..de]),
            });
            skip = j + 1..end;
        }
    }
}

/// `[label]: dest`, except footnotes (`[^1]: text`).
fn reference_definition(line: &str, m: &[u8]) -> Option<String> {
    let indent = m.iter().take_while(|&&c| c == b' ').count();
    if indent > 3 || m.get(indent) != Some(&b'[') || m.get(indent + 1) == Some(&b'^') {
        return None;
    }
    let close = indent + m[indent..].iter().position(|&c| c == b']')?;
    if close == indent + 1 || m.get(close + 1) != Some(&b':') {
        return None;
    }
    let mut p = close + 2;
    while m.get(p).is_some_and(|&c| c == b' ' || c == b'\t') {
        p += 1;
    }
    let (ds, de, rest) = if m.get(p) == Some(&b'<') {
        let e = p + 1 + m[p + 1..].iter().position(|&c| c == b'>')?;
        (p + 1, e, e + 1)
    } else {
        let len = m[p..]
            .iter()
            .position(|c| c.is_ascii_whitespace())
            .unwrap_or(m.len() - p);
        (p, p + len, p + len)
    };
    // Only an optional title may follow the destination; `[TODO]: check this` is prose.
    let title = line[rest..].trim();
    let is_title = title.is_empty()
        || (title.len() >= 2
            && matches!(
                (title.as_bytes()[0], title.as_bytes()[title.len() - 1]),
                (b'"', b'"') | (b'\'', b'\'') | (b'(', b')')
            ));
    (de > ds && is_title).then(|| unescape(&line[ds..de]))
}

/// `line` without the parts inside HTML comments, given whether a comment
/// was open when the line began.
fn strip_comments(line: &str, mut in_comment: bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        if in_comment {
            match rest.find("-->") {
                Some(j) => {
                    rest = &rest[j + 3..];
                    in_comment = false;
                }
                None => return out,
            }
        } else {
            match rest.find("<!--") {
                Some(j) => {
                    out.push_str(&rest[..j]);
                    rest = &rest[j + 4..];
                    in_comment = true;
                }
                None => {
                    out.push_str(rest);
                    return out;
                }
            }
        }
    }
}

/// An ATX heading's level and text.
pub fn atx(line: &str) -> Option<(usize, &str)> {
    let indent = line.bytes().take_while(|&c| c == b' ').count();
    if indent > 3 {
        return None;
    }
    let t = &line[indent..];
    let level = t.bytes().take_while(|&c| c == b'#').count();
    let rest = &t[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let text = rest.trim();
    // A closing run of `#` needs a space before it (or is the whole text).
    let stripped = text.trim_end_matches('#');
    let text = if stripped.is_empty() || stripped.ends_with([' ', '\t']) {
        stripped.trim_end()
    } else {
        text
    };
    Some((level, text))
}

fn setext_underline(m: &[u8]) -> Option<usize> {
    let t = std::str::from_utf8(m).ok()?.trim();
    let indent = m.iter().take_while(|&&c| c == b' ').count();
    if indent > 3 || t.is_empty() {
        return None;
    }
    if t.bytes().all(|c| c == b'=') {
        Some(1)
    } else if t.len() >= 2 && t.bytes().all(|c| c == b'-') {
        Some(2)
    } else {
        None
    }
}

/// A line that can be the text of a setext heading: plain paragraph text,
/// not a list item, quote, table row, HTML block or heading.
fn paragraph_text(line: &str) -> bool {
    let t = line.trim_start();
    if t.is_empty() || line.len() - t.len() > 3 || atx(line).is_some() {
        return false;
    }
    let list = t.starts_with(['-', '*', '+']) && (t.len() == 1 || t[1..].starts_with(' '))
        || t.split_once(['.', ')']).is_some_and(|(n, rest)| {
            !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) && rest.starts_with(' ')
        });
    !(list || t.starts_with(['>', '|', '<']))
}

pub(super) static LINK_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!?\[([^\]]*)\](?:\([^)]*\)|\[[^\]]*\])").expect("valid regex"));
static HTML_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"</?[A-Za-z][^>]*>").expect("valid regex"));
static ATTR_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{#([^}\s]+)\}").expect("valid"));
static HTML_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<[a-z][^>]*?\s(?:id|name)\s*=\s*["']([^"']+)["']"#).expect("valid regex")
});

/// An emoji shortcode such as `:mega:`, which GitHub renders as an emoji that
/// the slug then drops.
static SHORTCODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|\s):[a-z][a-z0-9_+-]*:(\s|$)").expect("valid regex"));

/// The text GitHub renders for a heading: link text without destinations, no
/// HTML tags, `{#id}` or emoji shortcodes, and no emphasis underscores
/// (intraword ones stay).
pub fn heading_text(raw: &str) -> String {
    let t = ATTR_ID.replace_all(raw, "");
    let t = LINK_TEXT.replace_all(t.trim(), "$1");
    let t = HTML_TAG.replace_all(&t, "");
    let t = SHORTCODE.replace_all(&t, "$1$2");
    let chars: Vec<char> = t.chars().collect();
    let mut out = String::with_capacity(t.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '_' {
            let run = chars[i..].iter().take_while(|&&c| c == '_').count();
            let before = i.checked_sub(1).map(|k| chars[k]);
            let after = chars.get(i + run).copied();
            if before.is_some_and(char::is_alphanumeric) && after.is_some_and(char::is_alphanumeric)
            {
                out.extend(&chars[i..i + run]);
            }
            i += run;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Parses `body`, whose first line is file line `first_line`.
pub fn parse(body: &str, first_line: usize) -> Doc {
    let mut doc = Doc::default();
    let mut fence = None;
    let mut in_comment = false;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut add_heading = |doc: &mut Doc, text: &str| {
        let base = slug(&heading_text(text));
        let n = counts.entry(base.clone()).or_insert(0);
        let s = if *n == 0 { base } else { format!("{base}-{n}") };
        *n += 1;
        doc.anchors.insert(s);
    };
    let mut previous: Option<&str> = None;
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
            previous = None;
            continue;
        }
        let visible = strip_comments(line, in_comment);
        let m = mask(line, &mut in_comment);
        let masked = String::from_utf8_lossy(&m);
        if let Some((level, text)) = atx(&visible) {
            if level == 1 && doc.title.is_none() {
                doc.title = Some(ATTR_ID.replace_all(text, "").trim().to_string());
            }
            add_heading(&mut doc, text);
        } else if let (Some(_), Some(prev)) = (setext_underline(&m), previous) {
            add_heading(&mut doc, prev.trim());
            previous = None;
            continue;
        }
        for c in ATTR_ID
            .captures_iter(&masked)
            .chain(HTML_ID.captures_iter(&masked))
        {
            doc.anchors.insert(c[1].to_lowercase());
        }
        if let Some(dest) = reference_definition(line, &m) {
            doc.links.push(Link { line: lineno, dest });
        } else {
            inline_links(line, &m, lineno, &mut doc.links);
        }
        previous = paragraph_text(&masked).then_some(line);
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dests(text: &str) -> Vec<String> {
        parse(text, 1).links.into_iter().map(|l| l.dest).collect()
    }

    #[test]
    fn slugs_follow_github() {
        assert_eq!(slug("Hello, World!"), "hello-world");
        assert_eq!(slug("2. Results (v2)"), "2-results-v2");
        assert_eq!(slug("📣 Overview"), "-overview");
        assert_eq!(
            slug("APG-first priority — 21 October 2026"),
            "apg-first-priority--21-october-2026"
        );
        assert_eq!(slug("snake_case and-dash"), "snake_case-and-dash");
        assert_eq!(slug("日本語の見出し：概要"), "日本語の見出し概要");
        assert_eq!(slug("Ünïcödé Straße"), "ünïcödé-straße");
        assert_eq!(slug("a  b"), "a--b");
        assert_eq!(slug("C++ & Rust 🚀"), "c--rust-");
    }

    #[test]
    fn duplicate_and_setext_headings_get_numbered_slugs() {
        let doc = parse(
            "# Intro\n\n## Intro\n\nIntro\n-----\n\n## :mega: Overview\n\n### 10:30: Start\n\n## [Linked](x.md) `code` __bold__ snake_case\n\nText\n===\n\n- item\n---\n",
            1,
        );
        let a: Vec<&str> = doc.anchors.iter().map(String::as_str).collect();
        for s in [
            "intro",
            "intro-1",
            "intro-2",
            "-overview",
            "1030-start",
            "linked-code-bold-snake_case",
            "text",
        ] {
            assert!(a.contains(&s), "{s} missing from {a:?}");
        }
        assert!(!a.contains(&"item"), "{a:?}");
        assert_eq!(doc.title.as_deref(), Some("Intro"));
    }

    #[test]
    fn headings_in_fences_do_not_count_and_explicit_anchors_do() {
        let doc = parse(
            "```md\n# Not a heading\n```\n~~~~\n## Nor this\n~~~~\n<a id=\"Custom\"></a>\n<a name='old-name'>\n## Title {#short}\n# Closing ##\n",
            1,
        );
        let a: Vec<&str> = doc.anchors.iter().map(String::as_str).collect();
        assert_eq!(a, ["closing", "custom", "old-name", "short", "title"]);
        assert_eq!(doc.title.as_deref(), Some("Closing"));
    }

    #[test]
    fn finds_inline_angle_image_and_reference_links() {
        let text = "See [a](a.md) and ![img](img/p.png \"Title\") and [b](<dir/with space.md>).\n\
                    [nested ![i](i.png)](n.md) [paren](f(1).md) [t](x.md 'single') [e]()\n\
                    [ref]: refs/r.md \"title\"\n[^1]: footnote, not a link\n   [angle]: <a b.md>\n\
                    [not](a link because of a space) [also not] (x.md)\n";
        assert_eq!(
            dests(text),
            [
                "a.md",
                "img/p.png",
                "dir/with space.md",
                "n.md",
                "i.png",
                "f(1).md",
                "x.md",
                "",
                "refs/r.md",
                "a b.md"
            ]
        );
    }

    #[test]
    fn skips_code_comments_and_escapes() {
        let text = "`[a](code.md)` and ``[b](x.md) ` still code`` [c](real.md)\n\
                    ```\n[d](fenced.md)\n```\n<!-- [e](comment.md)\nstill [f](comment.md) -->[g](after.md)\n\
                    \\[h](escaped.md) [i](a\\_b.md)\n";
        assert_eq!(dests(text), ["real.md", "after.md", "a_b.md"]);
        let lines: Vec<usize> = parse(text, 10).links.iter().map(|l| l.line).collect();
        assert_eq!(lines, [10, 15, 16]);
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("a%20b%E6%97%A5.md"), "a b日.md");
        assert_eq!(percent_decode("100%.md%2"), "100%.md%2");
    }
}
