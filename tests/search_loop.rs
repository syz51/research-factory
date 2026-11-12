//! End-to-end: the learning loop of `lab search` (docs/state-machines.md 3.3,
//! 3.4, 5.7): the query trace, TREC runs, the thesaurus, missed queries and
//! their aliases in `lab retro`, dismissed lesson candidates, and the
//! duplicate check for new records.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn lab_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lab"));
    cmd.args(args)
        .current_dir(dir)
        .env_remove("LAB_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("run lab")
}

fn lab(dir: &Path, args: &[&str]) -> Output {
    lab_env(dir, args, &[])
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The stdout of a command that must exit 0.
fn ok(dir: &Path, args: &[&str]) -> String {
    let o = lab(dir, args);
    assert_eq!(
        o.status.code(),
        Some(0),
        "lab {args:?}: {}{}",
        stdout(&o),
        stderr(&o)
    );
    stdout(&o)
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

const TRACE: &str = ".research/lab/search-queries.jsonl";
const THESAURUS: &str = "docs/thesaurus.txt";

const CONFIG: &str = r#"[project]
name = "demo"

[lab.archive]
links = ["docs/**/*.md"]
inactive_statuses = ["superseded"]

[[lab.archive.index]]
name = "decisions"
output = "docs/decisions/INDEX.md"
title = "Decisions"
include = ["docs/decisions/D*.md"]
id_from = '^(D\d+)-'
require = ["id", "title", "status"]

[[lab.archive.index]]
name = "history"
output = "docs/history/INDEX.md"
title = "History reports"
include = ["docs/history/[0-9]*.md"]
id_from = '^(\d+[a-z]?)-'
id_prefix = "H"
trim_leading_zeros = true
require = ["id", "title"]

[lab.search]
thesaurus = "docs/thesaurus.txt"
"#;

const D002: &str = "---\ntitle: Use BM25\nstatus: accepted\nsupersedes: [D001]\naliases: [lexical baseline]\nsummary: BM25 is the first-stage retriever\n---\n# D002\n\n## Why\n\nIt is strong on the pilot.\n";

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "research.toml", CONFIG);
    write(
        d,
        "docs/decisions/D001-use-tfidf.md",
        "---\ntitle: Use TF-IDF\nstatus: superseded\nsuperseded_by: D002\n---\n# D001\n\nThe first-stage retriever is TF-IDF.\n",
    );
    write(d, "docs/decisions/D002-use-bm25.md", D002);
    write(
        d,
        "docs/history/01-2026-10-02-start.md",
        "# Start of the project\n\nWe began.\n\n## Plan\n\nMeasure the retriever on the wombat questions.\n",
    );
    write(
        d,
        "docs/history/02-2026-10-03-haiku-opus.md",
        "# The Haiku/Opus model rule\n\nLight work goes to the small model.\n",
    );
    dir
}

fn trace_lines(d: &Path) -> Vec<Value> {
    read(d, TRACE)
        .lines()
        .map(|l| serde_json::from_str(l).expect("a JSON line"))
        .collect()
}

/// A query line as `lab search` writes it, for traces made by hand.
fn query_line(at: &str, session: &str, query: &str, state: &str, hits: &[&str]) -> String {
    let complete = if state == "hits" { hits.len() } else { 0 };
    serde_json::json!({
        "schema": "rf.search-query/1", "at": at, "session": session, "source": "cli",
        "query": [query], "scopes": ["notes"], "state": state, "stages": ["lexical"],
        "hit_count": hits.len(), "complete": complete, "hits": hits, "latency_ms": 3
    })
    .to_string()
}

fn open_line(at: &str, session: &str, anchor: &str) -> String {
    serde_json::json!({"schema": "rf.search-open/1", "at": at, "session": session, "anchor": anchor})
        .to_string()
}

/// Two misses of `fy shift`, each followed by a read of D002 or D001.
fn missed_trace(d: &Path, target: &str) {
    let lines = [
        query_line("2026-11-01T10:00:00Z", "s1", "FY shift", "zero", &[]),
        open_line("2026-11-01T10:01:00Z", "s1", &format!("{target}#L1-L9")),
        query_line("2026-11-02T09:00:00Z", "s2", "fy shift", "zero", &[]),
        open_line("2026-11-02T09:02:00Z", "s2", target),
    ];
    write(d, TRACE, &(lines.join("\n") + "\n"));
}

/// The id of the proposal for `query` in the `lab retro` output.
fn proposal_id(retro: &str, query: &str) -> String {
    let line = retro
        .lines()
        .find(|l| l.contains(&format!("\"{query}\"")))
        .unwrap_or_else(|| panic!("no proposal for {query}: {retro}"));
    line.trim_start_matches("- ")
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

// 1. Each query appends one rf.search-query/1 line; writing it never fails a query.
#[test]
fn two_queries_append_two_valid_trace_lines() {
    let dir = fixture();
    let d = dir.path();
    let env = [("LAB_SESSION", "s-1")];
    assert!(lab_env(d, &["search", "wombat"], &env).status.success());
    assert!(lab_env(d, &["search", "zzzqqq"], &env).status.success());
    let lines = trace_lines(d);
    assert_eq!(lines.len(), 2);
    for line in &lines {
        assert_eq!(line["schema"], "rf.search-query/1");
        assert_eq!(line["session"], "s-1");
        assert_eq!(line["source"], "cli");
        assert_eq!(line["stages"], serde_json::json!(["lexical"]));
        assert!(line["at"].as_str().unwrap().ends_with('Z'));
        assert!(line["latency_ms"].is_u64());
    }
    assert_eq!(lines[0]["state"], "hits");
    assert_eq!(lines[0]["query"], serde_json::json!(["wombat"]));
    assert_eq!(
        lines[0]["hits"][0],
        "docs/history/01-2026-10-02-start.md#L5-L7"
    );
    assert_eq!(lines[0]["complete"], 1);
    assert!(lines[0]["candidates"]["lexical"][0]["score"].is_f64());
    assert_eq!(lines[1]["state"], "zero");
    assert_eq!(lines[1]["unknown"], serde_json::json!(["zzzqqq"]));

    // An error is traced too.
    let bad = lab(d, &["search", "wombat", "--status", "nonesuch"]);
    assert_eq!(bad.status.code(), Some(2));
    let lines = trace_lines(d);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[2]["state"], "error");
    assert_eq!(lines[2]["session"], Value::Null);
    assert!(lines[2]["error"].as_str().unwrap().contains("nonesuch"));

    // A trace that cannot be written leaves the query as it was.
    std::fs::remove_file(d.join(TRACE)).unwrap();
    std::fs::create_dir(d.join(TRACE)).unwrap();
    let o = lab(d, &["search", "wombat"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).contains("wombat"));
    assert!(stderr(&o).contains("not written to the search trace"));
}

// 1. A linked worktree appends to the main checkout's trace.
#[test]
fn a_linked_worktree_traces_into_the_main_checkout() {
    let dir = fixture();
    let main = dir.path().join("main");
    std::fs::create_dir(&main).unwrap();
    for entry in ["research.toml", "docs"] {
        std::fs::rename(dir.path().join(entry), main.join(entry)).unwrap();
    }
    write(&main, ".gitignore", ".research/\n");
    let git = |cwd: &Path, args: &[&str]| {
        let o = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", stderr(&o));
    };
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "fixture"]);
    git(&main, &["worktree", "add", "-q", "../wt"]);
    let wt = dir.path().join("wt");
    ok(&wt, &["search", "wombat"]);
    assert!(!wt.join(TRACE).exists());
    assert_eq!(trace_lines(&main).len(), 1);
}

// 1. `--run` answers a topics file as a TREC run that ireval scores.
#[test]
fn run_writes_a_trec_run_that_ireval_scores() {
    let dir = fixture();
    let d = dir.path();
    write(
        d,
        "topics.jsonl",
        "{\"id\": \"T1\", \"type\": \"keyword\", \"query\": \"wombat\", \"phrasings\": [\"marsupial\"]}\n{\"id\": \"T2\", \"query\": \"BM25 pilot\"}\nT3\tzzzqqq\n",
    );
    let o = lab(
        d,
        &[
            "search",
            "--run",
            "topics.jsonl",
            "--out",
            "lab.run",
            "--limit",
            "5",
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stderr(&o).contains("3 topic(s)"), "{}", stderr(&o));
    let run = read(d, "lab.run");
    let rows: Vec<Vec<&str>> = run.lines().map(|l| l.split(' ').collect()).collect();
    assert!(
        rows.iter()
            .all(|r| r.len() == 6 && r[1] == "Q0" && r[5] == "lab-search")
    );
    assert_eq!(
        rows[0][..5],
        [
            "T1",
            "Q0",
            "docs/history/01-2026-10-02-start.md#L5-L7",
            "1",
            "5"
        ]
    );
    let t2: Vec<&Vec<&str>> = rows.iter().filter(|r| r[0] == "T2").collect();
    assert_eq!(t2[0][2], "docs/decisions/D002-use-bm25.md#L10-L12");
    assert!(
        rows.iter().all(|r| r[0] != "T3"),
        "a zero topic adds no row"
    );
    // Every line of the run is traced with source `run`.
    let lines = trace_lines(d);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l["source"] == "run"));

    // With --records the docid is the record's path.
    ok(
        d,
        &[
            "search",
            "--run",
            "topics.jsonl",
            "--records",
            "--out",
            "records.run",
        ],
    );
    assert!(
        read(d, "records.run").contains("T2 Q0 docs/decisions/D002-use-bm25.md 1 10 lab-search")
    );

    write(
        d,
        "qrels.txt",
        "T1 0 docs/history/01-2026-10-02-start.md#L5-L7 1\nT2 0 docs/decisions/D002-use-bm25.md#L10-L12 2\nT3 0 docs/history/02-2026-10-03-haiku-opus.md#L1-L3 1\n",
    );
    // ireval is a separate tool; score with it where it is installed.
    match Command::new("ireval")
        .args([
            "eval",
            "qrels.txt",
            "lab.run",
            "-c",
            "-m",
            "success@5,MRR,nDCG@10",
        ])
        .current_dir(d)
        .output()
    {
        Ok(o) => {
            assert!(o.status.success(), "ireval: {}", stderr(&o));
            assert!(stdout(&o).contains("Success@5"), "{}", stdout(&o));
        }
        Err(e) => eprintln!("ireval not run: {e}"),
    }

    let bad = lab(d, &["search", "--run", "topics.jsonl", "wombat"]);
    assert_eq!(bad.status.code(), Some(2));
}

// 2. A thesaurus line makes one term find a record that holds only the other,
// and the output names the added term.
#[test]
fn a_thesaurus_entry_expands_a_query_and_is_named() {
    let dir = fixture();
    let d = dir.path();
    let before = ok(d, &["search", "marsupial"]);
    assert!(before.contains("0 results"), "{before}");
    write(
        d,
        THESAURUS,
        "# terms\nwombat = marsupial\nretired: tfidf = haiku\n",
    );
    let out = ok(d, &["search", "marsupial"]);
    assert!(
        out.contains("Also searched: wombat (for marsupial)"),
        "{out}"
    );
    assert!(
        out.contains("docs/history/01-2026-10-02-start.md#L5-L7"),
        "{out}"
    );
    let explained = ok(d, &["search", "marsupial", "--explain"]);
    assert!(
        explained.contains("thesaurus: wombat = marsupial"),
        "{explained}"
    );
    let report: Value = serde_json::from_str(&ok(d, &["search", "--json", "marsupial"])).unwrap();
    assert_eq!(
        report["expansions"][0]["added"],
        serde_json::json!(["wombat"])
    );
    let line = trace_lines(d).pop().unwrap();
    assert_eq!(line["expansions"][0]["entry"], "wombat = marsupial");
    // A retired entry is not applied.
    let out = ok(d, &["search", "haiku"]);
    assert!(!out.contains("Also searched"), "{out}");

    // A bad entry is an error that names its line.
    write(d, THESAURUS, "wombat\n");
    let o = lab(d, &["search", "marsupial"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("thesaurus.txt:1"), "{}", stderr(&o));
}

// 2. `lab retro` lists zero-hit and partial-hit queries, most frequent first.
#[test]
fn retro_prints_missed_queries_most_frequent_first() {
    let dir = fixture();
    let d = dir.path();
    // A partial hit: a hit without every term.
    let partial = query_line(
        "2026-11-01T08:00:00Z",
        "s0",
        "alpha beta",
        "hits",
        &["x.md#L1-L2"],
    )
    .replace("\"complete\":1", "\"complete\":0");
    missed_trace(d, "docs/decisions/D002-use-bm25.md");
    let mut text = partial + "\n" + &read(d, TRACE);
    // A topics run and the duplicate check are not missed queries.
    text.push_str(
        &query_line("2026-11-03T09:00:00Z", "s3", "run only", "zero", &[])
            .replace("\"cli\"", "\"run\""),
    );
    text.push('\n');
    write(d, TRACE, &text);
    let out = ok(d, &["retro"]);
    let fy = out.find("\"fy shift\": 2 miss(es) (zero); target docs/decisions/D002-use-bm25.md");
    let alpha = out.find("\"alpha beta\": 1 miss(es) (partial-hit)");
    assert!(fy.is_some() && alpha.is_some(), "{out}");
    assert!(fy < alpha, "most frequent first: {out}");
    assert!(!out.contains("run only"), "{out}");
    let since = ok(d, &["retro", "--since", "2026-11-02"]);
    assert!(since.contains("\"fy shift\": 1 miss(es)"), "{since}");
    assert!(!since.contains("alpha beta"), "{since}");
}

// 2. Accept writes the alias into an open record; the proposal is settled.
#[test]
fn accept_adds_the_alias_to_an_open_record() {
    let dir = fixture();
    let d = dir.path();
    missed_trace(d, "docs/decisions/D002-use-bm25.md");
    let id = proposal_id(&ok(d, &["retro"]), "fy shift");
    let out = ok(d, &["retro", "accept", &id[..5]]);
    assert!(
        out.contains("Added alias \"fy shift\" to the front matter"),
        "{out}"
    );
    assert!(
        read(d, "docs/decisions/D002-use-bm25.md")
            .contains("aliases: [lexical baseline, \"fy shift\"]")
    );
    let hit = ok(d, &["search", "fy shift"]);
    assert!(hit.starts_with("1. D002"), "{hit}");
    let retro = ok(d, &["retro"]);
    assert!(!retro.contains("\"fy shift\""), "accepted: {retro}");
    let decisions: Value = serde_json::from_str(&read(d, ".research/lab/retro.json")).unwrap();
    assert_eq!(decisions["schema"], "rf.retro/1");
    assert_eq!(decisions["accepted"][0]["wrote"], "front matter");
    // Settled: there is nothing to accept again.
    assert_eq!(lab(d, &["retro", "accept", &id]).status.code(), Some(2));
}

// 2. A finished record is never edited: its alias goes to the thesaurus.
#[test]
fn a_finished_record_gets_its_alias_in_the_thesaurus() {
    let dir = fixture();
    let d = dir.path();
    let path = "docs/decisions/D001-use-tfidf.md";
    let before = read(d, path);
    missed_trace(d, path);
    let id = proposal_id(&ok(d, &["retro"]), "fy shift");
    let out = ok(d, &["retro", "accept", &id]);
    assert!(out.contains("is finished, so it is not edited"), "{out}");
    assert_eq!(read(d, path), before);
    assert!(read(d, THESAURUS).lines().any(|l| l == "fy shift = D001"));
    let hit = ok(d, &["search", "fy shift"]);
    assert!(hit.contains("Also searched: d001"), "{hit}");
    assert!(hit.contains("1. D001"), "{hit}");
}

// 2. A proposal without a target takes --record; no thesaurus means an error.
#[test]
fn accept_takes_a_record_id_when_the_trace_names_none() {
    let dir = fixture();
    let d = dir.path();
    write(
        d,
        TRACE,
        &(query_line("2026-11-01T10:00:00Z", "s1", "ranker", "zero", &[]) + "\n"),
    );
    let id = proposal_id(&ok(d, &["retro"]), "ranker");
    let o = lab(d, &["retro", "accept", &id]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("--record"), "{}", stderr(&o));
    ok(d, &["retro", "accept", &id, "--record", "D002"]);
    assert!(read(d, "docs/decisions/D002-use-bm25.md").contains("\"ranker\""));
}

// 2. A rejected proposal persists and is never proposed again.
#[test]
fn a_rejected_proposal_never_returns() {
    let dir = fixture();
    let d = dir.path();
    missed_trace(d, "docs/decisions/D002-use-bm25.md");
    let id = proposal_id(&ok(d, &["retro"]), "fy shift");
    let out = ok(d, &["retro", "reject", &id, "--note", "not a synonym"]);
    assert!(out.contains("Rejected"), "{out}");
    // More misses of the same query to the same record.
    let mut text = read(d, TRACE);
    text.push_str(&query_line(
        "2026-11-05T10:00:00Z",
        "s5",
        "fy shift",
        "zero",
        &[],
    ));
    text.push('\n');
    text.push_str(&open_line(
        "2026-11-05T10:01:00Z",
        "s5",
        "docs/decisions/D002-use-bm25.md",
    ));
    write(d, TRACE, &(text + "\n"));
    let retro = ok(d, &["retro"]);
    assert!(!retro.contains("fy shift"), "{retro}");
    let decisions: Value = serde_json::from_str(&read(d, ".research/lab/retro.json")).unwrap();
    assert_eq!(decisions["rejected"][0]["note"], "not a synonym");
}

// 2. An unused thesaurus entry is listed; retire keeps it but stops applying it.
#[test]
fn an_unused_entry_is_listed_and_retire_stops_it() {
    let dir = fixture();
    let d = dir.path();
    write(d, THESAURUS, "wombat = marsupial\ntfidf = vector space\n");
    // A trace older than the review window, in which one entry was used.
    let used = query_line(
        "2025-01-01T00:00:00Z",
        "s",
        "marsupial",
        "hits",
        &["a.md#L1-L1"],
    );
    write(d, TRACE, &(used + "\n"));
    ok(d, &["search", "marsupial"]);
    let retro = ok(d, &["retro"]);
    assert!(retro.contains("- line 2: tfidf = vector space"), "{retro}");
    assert!(!retro.contains("- line 1:"), "{retro}");
    let out = ok(d, &["retro", "retire", "2"]);
    assert!(out.contains("tfidf = vector space"), "{out}");
    assert_eq!(
        read(d, THESAURUS),
        "wombat = marsupial\nretired: tfidf = vector space\n"
    );
    assert!(!ok(d, &["retro"]).contains("line 2"));
    assert_eq!(lab(d, &["retro", "retire", "2"]).status.code(), Some(2));
}

// 2. Seed writes an entry for each record alias, once.
#[test]
fn seed_adds_record_aliases_once() {
    let dir = fixture();
    let d = dir.path();
    let out = ok(d, &["retro", "seed"]);
    assert!(out.contains("Added 1 entry(ies)"), "{out}");
    assert!(
        read(d, THESAURUS)
            .lines()
            .any(|l| l == "lexical baseline = D002")
    );
    let again = ok(d, &["retro", "seed"]);
    assert!(
        again.contains("holds every record alias already"),
        "{again}"
    );
}

// 3. A dismissed candidate is stored and does not return.
#[test]
fn a_dismissed_candidate_does_not_return() {
    let dir = fixture();
    let d = dir.path();
    let finding = |id: &str| {
        format!(
            r#"{{"schema":"rf.finding/1","tool":"agentprof","rule":"reread-file","severity":"warn","subject":"claude:{id}","message":"reread"}}"#
        )
    };
    write(
        d,
        "agent/findings.jsonl",
        &["a", "b", "c"].map(finding).join("\n"),
    );
    let args = ["retro", "--agent-findings", "agent/findings.jsonl"];
    let before = ok(d, &args);
    assert!(before.contains("agentprof:reread-file"), "{before}");
    let out = ok(
        d,
        &[
            "retro",
            "dismiss",
            "agentprof:reread-file",
            "--note",
            "harmless",
        ],
    );
    assert!(out.contains("Dismissed agentprof:reread-file"), "{out}");
    let after = ok(d, &args);
    assert!(!after.contains("agentprof:reread-file"), "{after}");
    assert!(
        after.contains("0 recurring rule(s) without a lesson"),
        "{after}"
    );
    let decisions: Value = serde_json::from_str(&read(d, ".research/lab/retro.json")).unwrap();
    assert_eq!(decisions["dismissed"][0]["rule"], "reread-file");
    assert_eq!(
        lab(d, &["retro", "dismiss", "no-colon"]).status.code(),
        Some(2)
    );
}

const D003: &str =
    "---\ntitle: BM25 lexical baseline again\nstatus: proposed\n---\n# D003\n\nA second look.\n";

// 4. The duplicate check lists a record on the same topic and omits unrelated ones.
#[test]
fn similar_lists_a_same_topic_record_and_omits_unrelated_ones() {
    let dir = fixture();
    let d = dir.path();
    write(d, "docs/decisions/D003-bm25-again.md", D003);
    assert!(!d.join(".research/lab/search/index.db").exists());
    let out = ok(
        d,
        &["archive", "similar", "docs/decisions/D003-bm25-again.md"],
    );
    // The absent index was built.
    assert!(d.join(".research/lab/search/index.db").exists());
    assert!(
        out.contains("D002 docs/decisions/D002-use-bm25.md · Use BM25"),
        "{out}"
    );
    assert!(!out.contains("D003"), "the file itself is left out: {out}");
    assert!(!out.contains("haiku") && !out.contains("D001"), "{out}");
    let words = ok(d, &["archive", "similar", "Haiku", "Opus", "model"]);
    assert!(words.contains("02-2026-10-03-haiku-opus.md"), "{words}");
    assert!(!words.contains("D002"), "{words}");
    let none = ok(d, &["archive", "similar", "quantum", "gravity"]);
    assert_eq!(none.trim(), "similar: none");
    let line = trace_lines(d).pop().unwrap();
    assert_eq!(line["source"], "similar");

    // `lab archive next --title` prints the list on stderr; stdout stays the id.
    let o = lab(
        d,
        &["archive", "next", "D", "--title", "BM25 lexical baseline"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o).trim(), "D004");
    assert!(stderr(&o).contains("D002"), "{}", stderr(&o));
}

// 4. A failed search never fails the check: it is skipped.
#[test]
fn similar_is_skipped_when_search_fails() {
    let dir = fixture();
    let d = dir.path();
    let broken = CONFIG.replace(
        "thesaurus = \"docs/thesaurus.txt\"",
        "thesaurus = \"docs/thesaurus.txt\"\nnotes = [\"docs/[\"]",
    );
    write(d, "research.toml", &broken);
    let out = ok(d, &["archive", "similar", "BM25", "baseline"]);
    assert!(out.starts_with("skipped: "), "{out}");
}

// 4. `lab archive check` warns about a new record on a known topic, and
// never fails because search failed.
#[test]
fn archive_check_warns_about_a_new_record_on_a_known_topic() {
    let dir = fixture();
    let d = dir.path();
    write(d, ".gitignore", ".research/\n");
    let git = |args: &[&str]| {
        let o = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(d)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", stderr(&o));
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "fixture"]);
    let clean = stdout(&lab(d, &["archive", "check"]));
    assert!(!clean.contains("similar-"), "{clean}");
    write(d, "docs/decisions/D003-bm25-again.md", D003);
    let o = lab(d, &["archive", "check", "--format", "jsonl"]);
    let findings: Vec<Value> = stdout(&o)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let similar: Vec<&Value> = findings
        .iter()
        .filter(|f| f["rule"] == "similar-record")
        .collect();
    assert_eq!(similar.len(), 1, "{findings:?}");
    assert_eq!(similar[0]["severity"], "warn");
    assert!(similar[0]["message"].as_str().unwrap().contains("D002"));

    let broken = CONFIG.replace(
        "thesaurus = \"docs/thesaurus.txt\"",
        "thesaurus = \"docs/thesaurus.txt\"\nnotes = [\"docs/[\"]",
    );
    write(d, "research.toml", &broken);
    let o = lab(d, &["archive", "check", "--format", "jsonl"]);
    let text = stdout(&o);
    assert!(text.contains("similar-skipped"), "{text}");
    assert!(!text.contains("\"rule\":\"similar-skipped\",\"severity\":\"error\""));
}
