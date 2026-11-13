//! `lab search --run TOPICS`: answers each topic of a test set and writes a
//! TREC run, `topic Q0 docid rank score tag`, that `ireval eval` scores
//! against a qrels file. A docid is a hit's anchor, `path#Lstart-Lend` (or
//! `ref:<key>` for a scope with keys), or with `--records` the record's path.
//! The run lists the docids in the order the output shows them: each hit,
//! then the units folded under it as `also in` lines.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use serde::Deserialize;

use crate::config::Project;

use super::{Args, Request, refresh_batch, traced};

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

/// The first `limit` docids of hits given as (docid, docids folded under it),
/// in the order the output shows them. A docid that came before is skipped.
fn shown<'a>(
    hits: impl IntoIterator<Item = (&'a str, Vec<&'a str>)>,
    limit: usize,
) -> Vec<&'a str> {
    let mut ids: Vec<&str> = Vec::new();
    for (id, also) in hits {
        for id in std::iter::once(id).chain(also) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids.truncate(limit);
    ids
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
    // Build or refresh once: listing the sources for each topic took most of
    // the run's time.
    let listed = refresh_batch(project, &args.scopes, args.rebuild)?;
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
            rebuild: false,
            weigh: false,
            graph: &args.graph,
            listed: Some(&listed),
        };
        let answer =
            traced(project, "run", &request).map_err(|e| format!("topic `{}`: {e}", topic.id))?;
        let ids = shown(
            answer.hits.iter().map(|hit| {
                let also = hit.also_in.iter().map(|a| {
                    if args.records {
                        a.anchor
                            .split_once("#L")
                            .map_or(a.anchor.as_str(), |(path, _)| path)
                    } else {
                        a.anchor.as_str()
                    }
                });
                let id = if args.records { &hit.path } else { &hit.anchor };
                (id.as_str(), also.collect())
            }),
            limit,
        );
        for (i, id) in ids.iter().enumerate() {
            // Distinct scores, the best the highest: ties would let the scorer reorder.
            let _ = writeln!(
                out,
                "{} Q0 {} {} {} {TAG}",
                topic.id,
                docid(id),
                i + 1,
                limit - i
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
    fn lists_the_units_folded_under_a_hit_after_it_once_up_to_the_limit() {
        let hits = [
            ("a#L1-L2", vec!["b#L1-L2", "c#L1-L2"]),
            ("b#L1-L2", vec![]),
            ("d#L1-L2", vec!["e#L1-L2"]),
        ];
        assert_eq!(
            shown(hits.clone(), 10),
            ["a#L1-L2", "b#L1-L2", "c#L1-L2", "d#L1-L2", "e#L1-L2"]
        );
        assert_eq!(shown(hits, 2), ["a#L1-L2", "b#L1-L2"]);
    }

    #[test]
    fn a_batch_lists_the_sources_once_and_carries_what_its_refresh_said() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |rel: &str, text: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "research.toml",
            "[project]\nname = \"demo\"\n\n[papers]\nregistry = \"refs.jsonl\"\n\n[lab.archive]\nlinks = [\"docs/**/*.md\"]\n",
        );
        // A registry that cannot be read gives a notice in the notes listing.
        std::fs::create_dir(root.join("refs.jsonl")).unwrap();
        write("docs/a.md", "# Lynx\n\nThe lynx hunts.\n");
        let project = Project::discover(root, None).unwrap();
        let scopes = ["notes".to_string()];
        let listed = refresh_batch(&project, &scopes, false).unwrap();
        // A source that is new after the batch's refresh.
        write("docs/b.md", "# Hare\n\nThe hare runs.\n");
        let graph = super::super::GraphArgs::default();
        let ask = |word: &str, listed| {
            let phrasings = [word.to_string()];
            super::super::execute(
                &project,
                &Request {
                    phrasings: &phrasings,
                    scopes: &scopes,
                    limit: 5,
                    records: false,
                    statuses: &[],
                    rebuild: false,
                    weigh: false,
                    graph: &graph,
                    listed,
                },
            )
            .unwrap()
        };
        let notice = |partial: &[String]| {
            partial
                .iter()
                .any(|p| p.contains("the paper texts are not left out of notes"))
        };
        let batch = ask("lynx", Some(&listed));
        assert_eq!(batch.hits.len(), 1);
        assert!(notice(&batch.partial), "{:?}", batch.partial);
        // A query of the batch does not list the sources: it misses the new one.
        assert!(ask("hare", Some(&listed)).hits.is_empty());
        let alone = ask("hare", None);
        assert_eq!(alone.hits.len(), 1);
        assert!(notice(&alone.partial), "{:?}", alone.partial);
    }

    #[test]
    fn escapes_spaces_in_docids() {
        assert_eq!(docid("a b/c%d.md#L1-L2"), "a%20b/c%25d.md#L1-L2");
    }
}
