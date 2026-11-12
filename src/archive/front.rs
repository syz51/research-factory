//! Optional YAML front matter at the top of a record: a first line `---`, YAML,
//! and a line `---`. Everything after it is the record's body.

use yaml_serde::Value;

/// A file split into its front matter and body.
#[derive(Debug, PartialEq)]
pub struct Split<'a> {
    /// The YAML between the two `---` lines, when the file has front matter.
    pub yaml: Option<&'a [u8]>,
    pub body: &'a [u8],
    /// The 1-based line number of the body's first line.
    pub body_line: usize,
    /// The file starts with `---` but no closing `---` line follows.
    pub unterminated: bool,
}

fn is_delimiter(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    line == b"---"
}

/// Splits `text` at its front matter. The split is purely syntactic, so it
/// does not depend on whether the YAML is valid.
pub fn split(text: &[u8]) -> Split<'_> {
    let whole = Split {
        yaml: None,
        body: text,
        body_line: 1,
        unterminated: false,
    };
    let mut lines = text.split_inclusive(|&b| b == b'\n');
    let Some(first) = lines.next() else {
        return whole;
    };
    if !is_delimiter(first) || !first.ends_with(b"\n") {
        return whole;
    }
    let mut offset = first.len();
    for (i, line) in lines.enumerate() {
        if is_delimiter(line) {
            return Split {
                yaml: Some(&text[first.len()..offset]),
                body: &text[offset + line.len()..],
                body_line: i + 3,
                unterminated: false,
            };
        }
        offset += line.len();
    }
    Split {
        unterminated: true,
        ..whole
    }
}

/// The known front-matter fields. Unknown fields are ignored.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Meta {
    pub id: Option<String>,
    pub title: Option<String>,
    pub date: Option<String>,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub aliases: Vec<String>,
    pub supersedes: Vec<String>,
    pub superseded_by: Vec<String>,
    pub decisions: Vec<String>,
    pub evidence: Vec<String>,
    pub tags: Vec<String>,
    /// The board tasks the record belongs to (`task`).
    pub tasks: Vec<String>,
    pub kind: Option<String>,
    /// Records this one supersedes in part; they stay in force.
    pub amends: Vec<String>,
    pub amended_by: Vec<String>,
    pub withdrawn_reason: Option<String>,
    /// What is wrong with the fields above. Only archive model 2 reports it,
    /// so a model 1 project's front matter stays valid as it was.
    pub model_2_errors: Vec<String>,
}

/// An invalid front matter: the file line it concerns and what is wrong.
#[derive(Debug, PartialEq)]
pub struct Invalid {
    pub line: usize,
    pub message: String,
}

pub fn valid_date(s: &str) -> bool {
    s.len() == 10 && s.parse::<jiff::civil::Date>().is_ok()
}

/// Parses front matter that starts on file line 2. `statuses` are the allowed
/// values of `status`.
/// The part of a sealed file that the seal covers: the body after front
/// matter that parses as a YAML mapping, else the whole file. A log that starts
/// with a `---` rule therefore stays sealed in full.
pub fn sealed_body(bytes: &[u8]) -> &[u8] {
    let s = split(bytes);
    match s.yaml {
        Some(yaml) if matches!(yaml_serde::from_slice::<Value>(yaml), Ok(Value::Mapping(_))) => {
            s.body
        }
        _ => bytes,
    }
}

pub fn parse(yaml: &[u8], statuses: &[String]) -> Result<Meta, Invalid> {
    let invalid = |message: String| Invalid { line: 1, message };
    let text =
        std::str::from_utf8(yaml).map_err(|_| invalid("front matter is not UTF-8".into()))?;
    let value: Value = yaml_serde::from_str(text).map_err(|e| Invalid {
        line: e.location().map_or(1, |l| l.line() + 1),
        message: format!("front matter is not valid YAML: {e}"),
    })?;
    let map = match value {
        Value::Mapping(map) => map,
        Value::Null => return Ok(Meta::default()),
        _ => return Err(invalid("front matter must be a mapping of fields".into())),
    };
    let field = |name: &str| map.get(name).filter(|v| !v.is_null());
    let scalar = |name: &str| -> Result<Option<String>, Invalid> {
        match field(name) {
            None => Ok(None),
            Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.trim().to_string())),
            Some(_) => Err(invalid(format!(
                "`{name}` must be a string (quote it if it looks like a number or date)"
            ))),
        }
    };
    let list = |name: &str| -> Result<Vec<String>, Invalid> {
        let items = match field(name) {
            None => return Ok(Vec::new()),
            Some(Value::Sequence(items)) => items.iter().collect(),
            Some(one) => vec![one],
        };
        items
            .into_iter()
            .map(|v| match v {
                Value::String(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
                _ => Err(invalid(format!(
                    "`{name}` must be a string or a list of non-empty strings"
                ))),
            })
            .collect()
    };
    // The model 2 fields never make the front matter invalid here.
    let mut model_2_errors = Vec::new();
    let mut lenient = |r: Result<Vec<String>, Invalid>| {
        r.unwrap_or_else(|e| {
            model_2_errors.push(e.message);
            Vec::new()
        })
    };
    let tasks = lenient(list("task"));
    let amends = lenient(list("amends"));
    let amended_by = lenient(list("amended_by"));
    let kind = lenient(scalar("kind").map(|v| v.into_iter().collect())).pop();
    let withdrawn_reason =
        lenient(scalar("withdrawn_reason").map(|v| v.into_iter().collect())).pop();
    let meta = Meta {
        tasks,
        kind,
        amends,
        amended_by,
        withdrawn_reason,
        model_2_errors,
        id: scalar("id")?,
        title: scalar("title")?,
        date: scalar("date")?,
        status: scalar("status")?,
        summary: scalar("summary")?,
        aliases: list("aliases")?,
        supersedes: list("supersedes")?,
        superseded_by: list("superseded_by")?,
        decisions: list("decisions")?,
        evidence: list("evidence")?,
        tags: list("tags")?,
    };
    if let Some(d) = &meta.date
        && !valid_date(d)
    {
        return Err(invalid(format!("`date` must be YYYY-MM-DD, got `{d}`")));
    }
    if let Some(s) = &meta.status
        && !statuses.contains(s)
    {
        return Err(invalid(format!(
            "`status` `{s}` is not one of: {}",
            statuses.join(", ")
        )));
    }
    Ok(meta)
}

/// The `status` in a file's front matter, when it has one that is a string.
pub fn status_of(bytes: &[u8]) -> Option<String> {
    let yaml = split(bytes).yaml?;
    match yaml_serde::from_slice::<Value>(yaml).ok()? {
        Value::Mapping(map) => match map.get("status") {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// The first `YYYY-MM-DD` in `path` that is a real date.
pub fn date_in(path: &str) -> Option<String> {
    let b = path.as_bytes();
    (0..b.len().saturating_sub(9)).find_map(|i| {
        let w = &b[i..i + 10];
        let shape = w.iter().enumerate().all(|(j, c)| match j {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        });
        let bounded = (i == 0 || !b[i - 1].is_ascii_digit())
            && b.get(i + 10).is_none_or(|c| !c.is_ascii_digit());
        // An all-ASCII window starts and ends on character boundaries.
        let s = (shape && bounded).then(|| &path[i..i + 10])?;
        valid_date(s).then(|| s.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statuses() -> Vec<String> {
        vec!["current".into(), "superseded".into()]
    }

    #[test]
    fn splits_front_matter_from_the_body() {
        let s = split(b"---\nid: D001\n---\n# T\n");
        assert_eq!(s.yaml, Some(&b"id: D001\n"[..]));
        assert_eq!(s.body, b"# T\n");
        assert_eq!(s.body_line, 4);
        let crlf = split(b"---\r\nid: x\r\n---\r\nbody");
        assert_eq!(crlf.body, b"body");
        let none = split(b"# T\n---\n");
        assert_eq!((none.yaml, none.body_line), (None, 1));
        let open = split(b"---\nid: x\n");
        assert!(open.unterminated && open.yaml.is_none());
        let empty = split(b"---\n---\n");
        assert_eq!((empty.yaml, empty.body), (Some(&b""[..]), &b""[..]));
    }

    #[test]
    fn parses_known_fields_and_accepts_scalars_as_lists() {
        let m = parse(
            b"id: D002\ntitle: Use BM25\ndate: 2026-11-07\nstatus: current\nsupersedes: D001\naliases: [bm25, lexical]\nother: {a: 1}\n",
            &statuses(),
        )
        .unwrap();
        assert_eq!(m.id.as_deref(), Some("D002"));
        assert_eq!(m.date.as_deref(), Some("2026-11-07"));
        assert_eq!(m.supersedes, ["D001"]);
        assert_eq!(m.aliases, ["bm25", "lexical"]);
        assert_eq!(parse(b"", &statuses()).unwrap(), Meta::default());
    }

    #[test]
    fn model_2_fields_are_read_and_their_errors_kept_apart() {
        let m = parse(
            b"task: rf-1\nkind: decision\namends: [D001, D002]\namended_by: D009\nwithdrawn_reason: wrong data\n",
            &statuses(),
        )
        .unwrap();
        assert_eq!(m.tasks, ["rf-1"]);
        assert_eq!(m.kind.as_deref(), Some("decision"));
        assert_eq!(m.amends, ["D001", "D002"]);
        assert_eq!(m.amended_by, ["D009"]);
        assert_eq!(m.withdrawn_reason.as_deref(), Some("wrong data"));
        assert!(m.model_2_errors.is_empty());
        let bad = parse(b"id: D1\nkind: [a]\ntask: 7\n", &statuses()).unwrap();
        assert_eq!(bad.id.as_deref(), Some("D1"));
        assert_eq!((bad.kind, bad.tasks.len()), (None, 0));
        assert_eq!(bad.model_2_errors.len(), 2, "{:?}", bad.model_2_errors);
        assert_eq!(
            status_of(b"---\nstatus: accepted\n---\nbody").as_deref(),
            Some("accepted")
        );
        assert_eq!(status_of(b"# no front matter\n"), None);
    }

    #[test]
    fn reports_invalid_front_matter() {
        let err = |y: &str| parse(y.as_bytes(), &statuses()).unwrap_err().message;
        assert!(err("id: 7\n").contains("`id` must be a string"));
        assert!(err("date: 2026-02-30\n").contains("YYYY-MM-DD"));
        assert!(err("status: done\n").contains("not one of: current, superseded"));
        assert!(err("tags: [a, [b]]\n").contains("`tags`"));
        assert!(err("- a\n").contains("mapping"));
        let bad = parse(b"id: x\ntitle: [unclosed\n", &statuses()).unwrap_err();
        assert!(bad.message.contains("not valid YAML"), "{}", bad.message);
        assert!(bad.line >= 2, "{bad:?}");
    }

    #[test]
    fn finds_the_first_real_date_in_a_path() {
        assert_eq!(
            date_in("history/100-2026-10-27-90353-x.md").as_deref(),
            Some("2026-10-27")
        );
        assert_eq!(
            date_in("f/2026-13-01-x/2026-10-09-y/README.md").as_deref(),
            Some("2026-10-09")
        );
        assert_eq!(date_in("a/12026-10-09.md"), None);
        assert_eq!(date_in("a.md"), None);
    }
}
