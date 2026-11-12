//! `lab search --run TOPICS`: answers each topic of a test set and writes a
//! TREC run, `topic Q0 docid rank score tag`, that `ireval eval` scores
//! against a qrels file. A docid is a hit's anchor, `path#Lstart-Lend` (or
//! `ref:<key>` for a scope with keys), or with `--records` the record's path.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use serde::Deserialize;

use crate::config::Project;

use super::{Args, Request, traced};

/// Hits per topic unless `--limit` says otherwise.
const DEFAULT_LIMIT: usize = 10;
/// The run tag of the sixth column.
const TAG: &str = "lab-search";

/// A topic line in JSON, as the test set's `topics.jsonl` holds it. Other
/// fields are ignored.
#[derive(Deserialize)]
struct JsonTopic {
    id: serde_json::Value,
    query: String,
    #[serde(default)]
    phrasings: Vec<String>,
}

#[derive(Debug)]
struct Topic {
    id: String,
    query: String,
    phrasings: Vec<String>,
}

/// Reads topics: JSON objects with `id` and `query` (and optional
/// `phrasings`), one per line, or `id<TAB>query` lines. `#` starts a comment.
fn parse(text: &str, file: &Path) -> Result<Vec<Topic>, String> {
    let mut topics: Vec<Topic> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = || format!("{}:{}", file.display(), i + 1);
        let topic = if line.starts_with('{') {
            let t: JsonTopic = serde_json::from_str(line).map_err(|e| format!("{}: {e}", at()))?;
            let id = match t.id {
                serde_json::Value::String(s) => s,
                serde_json::Value::Number(n) => n.to_string(),
                _ => return Err(format!("{}: `id` must be a string or a number", at())),
            };
            Topic {
                id,
                query: t.query,
                phrasings: t.phrasings,
            }
        } else {
            let (id, query) = line
                .split_once('\t')
                .ok_or_else(|| format!("{}: expected a JSON object or `id<TAB>query`", at()))?;
            Topic {
                id: id.trim().into(),
                query: query.trim().into(),
                phrasings: Vec::new(),
            }
        };
        if topic.id.is_empty() || topic.id.contains(char::is_whitespace) {
            return Err(format!("{}: a topic id must be one word", at()));
        }
        if topics.iter().any(|t| t.id == topic.id) {
            return Err(format!("{}: topic `{}` repeats", at(), topic.id));
        }
        topics.push(topic);
    }
    if topics.is_empty() {
        return Err(format!("{} holds no topic", file.display()));
    }
    Ok(topics)
}

/// A docid as one TREC column: a space would split it.
fn docid(id: &str) -> String {
    id.replace('%', "%25").replace(' ', "%20")
}

pub fn run(project: &Project, args: &Args, file: &Path) -> Result<ExitCode, String> {
    if !args.query.is_empty() {
        return Err("--run takes its queries from the topics file; give no query words".into());
    }
    if args.json || args.explain {
        return Err("--run writes a TREC run; --json and --explain do not apply".into());
    }
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let topics = parse(&text, file)?;
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
    let mut out = String::new();
    let mut hits = 0;
    let mut rebuild = args.rebuild;
    for topic in &topics {
        let mut phrasings = vec![topic.query.clone()];
        if args.phrasings {
            phrasings.extend(topic.phrasings.iter().cloned());
        }
        let request = Request {
            phrasings: &phrasings,
            scopes: &args.scopes,
            limit,
            records: args.records,
            statuses: &args.statuses,
            rebuild,
            weigh: false,
        };
        let answer =
            traced(project, "run", &request).map_err(|e| format!("topic `{}`: {e}", topic.id))?;
        // Build at most once.
        rebuild = false;
        for hit in &answer.hits {
            let id = if args.records { &hit.path } else { &hit.anchor };
            // Distinct scores, the best the highest: ties would let the scorer reorder.
            let _ = writeln!(
                out,
                "{} Q0 {} {} {} {TAG}",
                topic.id,
                docid(id),
                hit.rank,
                limit + 1 - hit.rank
            );
            hits += 1;
        }
    }
    match &args.out {
        Some(path) => {
            std::fs::write(path, &out).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        None => print!("{out}"),
    }
    eprintln!("lab: {} topic(s), {hits} hit(s) in the run.", topics.len());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_json_and_tab_topics() {
        let text = "# test set\n{\"id\": \"T1\", \"query\": \"fiscal lag\", \"phrasings\": [\"lag\"], \"need\": \"x\"}\nT2\tcalendar shift\n";
        let topics = parse(text, Path::new("t.jsonl")).unwrap();
        assert_eq!(topics.len(), 2);
        assert_eq!(topics[0].id, "T1");
        assert_eq!(topics[0].phrasings, ["lag"]);
        assert_eq!(topics[1].query, "calendar shift");
    }

    #[test]
    fn refuses_a_repeated_topic() {
        let err = parse("A\tx\nA\ty\n", Path::new("t")).unwrap_err();
        assert!(err.contains("repeats"), "{err}");
    }

    #[test]
    fn escapes_spaces_in_docids() {
        assert_eq!(docid("a b/c%d.md#L1-L2"), "a%20b/c%25d.md#L1-L2");
    }
}
