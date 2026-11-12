//! End-to-end: `lab search` builds and refreshes its index, ranks sections of
//! a small synthetic archive and ends in one of its four states.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn lab(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lab"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run lab")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The output of a search that must end with exit 0.
fn search(dir: &Path, args: &[&str]) -> String {
    let o = lab(dir, &[&["search"], args].concat());
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    stdout(&o)
}

fn json(dir: &Path, args: &[&str]) -> Value {
    serde_json::from_str(&search(dir, &[&["--json"], args].concat())).expect("one JSON object")
}

/// The paths of the hits, best first.
fn paths(report: &Value) -> Vec<&str> {
    report["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["path"].as_str().unwrap())
        .collect()
}

const CONFIG: &str = r#"[project]
name = "demo"

[lab.archive]
links = ["docs/**/*.md"]

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

[lab.search.classes]
copied = ["docs/inputs/**"]
historical = ["docs/old/**"]
"#;

const INDEX_DB: &str = ".research/lab/search/index.db";

/// Two decisions (one superseding the other), three history reports, a copied
/// page, an old page and a generated index.
fn archive(d: &Path) {
    write(d, "research.toml", CONFIG);
    write(
        d,
        "docs/decisions/D001-use-tfidf.md",
        "---\ntitle: Use TF-IDF\nstatus: superseded\nsuperseded_by: D002\n---\n# D001\n\nThe first-stage retriever is TF-IDF. Retriever, retriever, retriever.\n",
    );
    write(
        d,
        "docs/decisions/D002-use-bm25.md",
        "---\ntitle: Use BM25\nstatus: accepted\nsupersedes: [D001]\naliases: [lexical baseline]\nsummary: BM25 is the first-stage retriever\n---\n# D002\n\n## Why\n\nIt is strong on the pilot.\n\n## Cost\n\nThe build needs a quarter of an hour.\n",
    );
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
    write(
        d,
        "docs/history/03-2026-10-04-mentions.md",
        "# A later report\n\nD002 was applied to the pilot again.\n",
    );
    write(
        d,
        "docs/inputs/copied.md",
        "# A copied page\n\nThe pilot of another project.\n",
    );
    write(
        d,
        "docs/old/plan.md",
        "# An old plan\n\nThe pilot was planned for spring.\n",
    );
    write(
        d,
        "docs/decisions/INDEX.md",
        "# Decisions\n\nGenerated: the pilot of an index.\n",
    );
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    archive(dir.path());
    dir
}

#[test]
fn a_body_word_returns_its_section_with_lines_and_a_snippet() {
    let dir = fixture();
    let out = search(dir.path(), &["wombat"]);
    assert!(
        out.contains("docs/history/01-2026-10-02-start.md#L5-L7 · Start of the project > Plan"),
        "{out}"
    );
    assert!(
        out.contains("Measure the retriever on the wombat questions."),
        "{out}"
    );
    assert!(
        out.starts_with("1. H1 · 2026-10-02 · Start of the project\n"),
        "{out}"
    );

    let report = json(dir.path(), &["wombat"]);
    assert_eq!(report["schema"], "rf.search/1");
    assert_eq!(report["state"], "hits");
    let hit = &report["hits"][0];
    assert_eq!(hit["anchor"], "docs/history/01-2026-10-02-start.md#L5-L7");
    assert_eq!(
        (hit["start"].as_i64(), hit["end"].as_i64()),
        (Some(5), Some(7))
    );
    assert_eq!(hit["hash"].as_str().unwrap().len(), 32);
    assert!(hit.get("explain").is_none());
    // The index is this checkout's own and git ignores it.
    assert!(dir.path().join(INDEX_DB).is_file());
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".research/lab/search/.gitignore")).unwrap(),
        "*\n"
    );
}

#[test]
fn words_match_a_title_written_with_a_slash_and_phrases_match_in_order() {
    let dir = fixture();
    let d = dir.path();
    let report = json(d, &["haiku opus"]);
    assert_eq!(
        paths(&report)[0],
        "docs/history/02-2026-10-03-haiku-opus.md"
    );
    assert_eq!(
        report["hits"][0]["record"]["title"],
        "The Haiku/Opus model rule"
    );

    assert_eq!(
        paths(&json(d, &["\"quarter of an hour\""])),
        ["docs/decisions/D002-use-bm25.md"]
    );
    assert_eq!(json(d, &["\"hour of a quarter\""])["state"], "zero");
    // Words of one query rank the sections with all of them first.
    let both = json(d, &["pilot", "strong"]);
    assert_eq!(
        both["hits"][0]["anchor"],
        "docs/decisions/D002-use-bm25.md#L10-L12"
    );
    assert!(both["hits"][0].get("missing").is_none());
    assert!(paths(&both).len() > 1);
    assert_eq!(both["hits"][1]["missing"][0], "strong");
}

#[test]
fn an_unknown_word_is_named_and_the_other_words_still_hit() {
    let dir = fixture();
    let d = dir.path();
    let out = search(d, &["wombat", "retreiver", "zzqq"]);
    assert!(
        out.contains("docs/history/01-2026-10-02-start.md#L5-L7"),
        "{out}"
    );
    assert!(out.contains("missing: retreiver, zzqq"), "{out}");
    assert!(
        out.contains("Not in the index: retreiver (nearest: retriever); zzqq\n"),
        "{out}"
    );
    let report = json(d, &["wombat", "retreiver"]);
    assert_eq!(report["state"], "hits");
    assert_eq!(report["unknown"][0]["word"], "retreiver");
    assert_eq!(report["unknown"][0]["nearest"][0], "retriever");

    // A part of a word finds the word.
    let part = json(d, &["womb"]);
    assert_eq!(paths(&part), ["docs/history/01-2026-10-02-start.md"]);
}

#[test]
fn no_hit_exits_0_with_0_results_and_the_nearest_words() {
    let dir = fixture();
    let o = lab(dir.path(), &["search", "retreiver"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "0 results\nNot in the index: retreiver (nearest: retriever)\nSearched: notes\n"
    );
    let report = json(dir.path(), &["zzqq"]);
    assert_eq!(report["state"], "zero");
    assert_eq!(report["hits"].as_array().unwrap().len(), 0);
}

#[test]
fn an_exact_id_ranks_first_and_a_superseded_record_below_its_successor() {
    let dir = fixture();
    let d = dir.path();
    // H3 names D002 in its text; the record D002 still comes first.
    let by_id = json(d, &["d002"]);
    assert_eq!(paths(&by_id)[0], "docs/decisions/D002-use-bm25.md");
    assert!(paths(&by_id).contains(&"docs/history/03-2026-10-04-mentions.md"));

    // D001 says `retriever` more often than D002 does.
    let report = json(d, &["--records", "retriever"]);
    let found = paths(&report);
    let at = |p: &str| found.iter().position(|f| *f == p).unwrap();
    assert!(
        at("docs/decisions/D002-use-bm25.md") < at("docs/decisions/D001-use-tfidf.md"),
        "{found:?}"
    );
    let old = &report["hits"][at("docs/decisions/D001-use-tfidf.md")]["record"];
    assert_eq!(old["status"], "superseded");
    assert_eq!(old["inactive"], true);
    assert_eq!(old["superseded_by"][0], "D002");
    let text = search(d, &["--records", "retriever"]);
    assert!(
        text.contains("D001 · superseded · superseded by D002 · Use TF-IDF"),
        "{text}"
    );

    assert_eq!(
        paths(&json(d, &["--status", "superseded", "retriever"])),
        ["docs/decisions/D001-use-tfidf.md"]
    );
}

#[test]
fn source_classes_label_and_rank_and_generated_files_are_not_indexed() {
    let dir = fixture();
    let report = json(dir.path(), &["--records", "pilot"]);
    let found = paths(&report);
    assert!(!found.contains(&"docs/decisions/INDEX.md"), "{found:?}");
    let class = |p: &str| {
        let at = found.iter().position(|f| *f == p).unwrap();
        (at, report["hits"][at]["record"]["class"].as_str().unwrap())
    };
    let (own, own_class) = class("docs/history/03-2026-10-04-mentions.md");
    let (copied, copied_class) = class("docs/inputs/copied.md");
    let (old, old_class) = class("docs/old/plan.md");
    assert_eq!(
        (own_class, copied_class, old_class),
        ("own", "copied", "historical")
    );
    assert!(own < copied && copied < old, "{found:?}");
    let text = search(dir.path(), &["--records", "pilot"]);
    assert!(text.contains(" · historical · An old plan"), "{text}");
}

#[test]
fn a_changed_and_a_deleted_file_show_in_the_next_query() {
    let dir = fixture();
    let d = dir.path();
    assert_eq!(json(d, &["wombat"])["state"], "hits");
    assert_eq!(json(d, &["numbat"])["state"], "zero");

    write(
        d,
        "docs/history/01-2026-10-02-start.md",
        "# Start of the project\n\nWe began.\n\n## Plan\n\nMeasure the retriever on the numbat questions instead.\n",
    );
    write(
        d,
        "docs/history/04-2026-10-05-new.md",
        "# New\n\nA quokka.\n",
    );
    assert_eq!(json(d, &["wombat"])["state"], "zero");
    let changed = json(d, &["numbat"]);
    assert_eq!(
        changed["hits"][0]["anchor"],
        "docs/history/01-2026-10-02-start.md#L5-L7"
    );
    assert_eq!(
        paths(&json(d, &["quokka"])),
        ["docs/history/04-2026-10-05-new.md"]
    );

    std::fs::remove_file(d.join("docs/history/01-2026-10-02-start.md")).unwrap();
    assert_eq!(json(d, &["numbat"])["state"], "zero");
    assert_eq!(json(d, &["quokka"])["state"], "hits");
}

#[test]
fn archive_find_is_search_in_notes_by_record() {
    let dir = fixture();
    let d = dir.path();
    for words in [&["pilot"][..], &["first-stage", "retriever"], &["zzqq"]] {
        let find = lab(d, &[&["archive", "find"], words].concat());
        let search = lab(
            d,
            &[&["search", "--in", "notes", "--records"], words].concat(),
        );
        assert_eq!(find.status.code(), Some(0), "{}", stderr(&find));
        assert_eq!(stdout(&find), stdout(&search));
        assert!(!stdout(&find).is_empty());
    }
    // One hit for each record.
    let report = json(d, &["--records", "pilot"]);
    let mut found = paths(&report);
    let n = found.len();
    found.sort_unstable();
    found.dedup();
    assert_eq!(found.len(), n);
}

#[test]
fn a_broad_query_shows_at_most_8_hits_and_limit_raises_it() {
    let dir = fixture();
    let d = dir.path();
    for i in 0..30 {
        write(
            d,
            &format!("docs/notes/n{i}.md"),
            &format!("# Note {i}\n\nThe marmot count is {i}.\n"),
        );
    }
    let report = json(d, &["marmot"]);
    assert_eq!(report["hits"].as_array().unwrap().len(), 8);
    assert_eq!(report["more"], 22);
    let text = search(d, &["marmot"]);
    let shown = text.lines().filter(|l| l.contains("#L")).count();
    assert!((1..=8).contains(&shown), "{text}");
    assert!(text.len() <= 8 * 256 + 100, "{} bytes", text.len());
    assert!(text.contains("more hit(s)"), "{text}");
    assert_eq!(
        json(d, &["--limit", "12", "marmot"])["hits"]
            .as_array()
            .unwrap()
            .len(),
        12
    );
    let explained = search(d, &["--explain", "--limit", "1", "marmot"]);
    assert!(
        explained.contains("explain: exact_id=false bm25="),
        "{explained}"
    );
    assert!(json(d, &["--explain", "marmot"])["hits"][0]["explain"]["bm25"].is_number());
}

#[test]
fn another_schema_version_and_a_corrupt_file_rebuild_the_index() {
    let dir = fixture();
    let d = dir.path();
    assert_eq!(json(d, &["wombat"])["state"], "hits");
    let version = |d: &Path| -> String {
        rusqlite::Connection::open(d.join(INDEX_DB))
            .unwrap()
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    };
    let current = version(d);
    {
        let conn = rusqlite::Connection::open(d.join(INDEX_DB)).unwrap();
        conn.execute(
            "UPDATE meta SET value = '0' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();
        // A rebuild drops what an index of another version held.
        conn.execute("DELETE FROM units_fts", []).unwrap();
    }
    assert_eq!(json(d, &["wombat"])["state"], "hits");
    assert_eq!(version(d), current);

    std::fs::write(d.join(INDEX_DB), b"not a database, not a database").unwrap();
    assert_eq!(json(d, &["wombat"])["state"], "hits");
    assert_eq!(version(d), current);

    // A change of what is indexed also rebuilds.
    write(
        d,
        "research.toml",
        &CONFIG.replace(
            "copied = [\"docs/inputs/**\"]",
            "generated = [\"docs/inputs/**\"]",
        ),
    );
    let found = json(d, &["--records", "pilot"]);
    assert!(!paths(&found).contains(&"docs/inputs/copied.md"));

    let rebuilt = lab(d, &["search", "--rebuild"]);
    assert_eq!(rebuilt.status.code(), Some(0), "{}", stderr(&rebuilt));
    assert!(
        stdout(&rebuilt).starts_with("Search index rebuilt: "),
        "{}",
        stdout(&rebuilt)
    );
    // No temporary file stays behind.
    let left: Vec<String> = std::fs::read_dir(d.join(".research/lab/search"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("tmp") || n.contains("journal"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn errors_exit_2_and_json_carries_the_state() {
    let dir = fixture();
    let d = dir.path();
    let scope = lab(d, &["search", "--in", "refs", "wombat"]);
    assert_eq!(scope.status.code(), Some(2));
    assert!(
        stderr(&scope).contains("unknown scope `refs`; the scopes are: notes"),
        "{}",
        stderr(&scope)
    );
    let status = lab(d, &["search", "--status", "maybe", "wombat"]);
    assert_eq!(status.status.code(), Some(2));
    assert!(
        stderr(&status).contains("unknown status `maybe`"),
        "{}",
        stderr(&status)
    );
    for bad in [
        &["--json", "--", "-- !!"][..],
        &["--json"],
        &["--json", "--limit", "0", "x"],
    ] {
        let o = lab(d, &[&["search"], bad].concat());
        assert_eq!(o.status.code(), Some(2), "{bad:?}");
        let report: Value = serde_json::from_str(&stdout(&o)).expect("one JSON object");
        assert_eq!(report["state"], "error", "{bad:?}");
        assert!(!report["error"].as_str().unwrap().is_empty());
        assert!(stderr(&o).starts_with("lab: "), "{}", stderr(&o));
    }

    let none = tempfile::tempdir().unwrap();
    write(none.path(), "research.toml", "[project]\nname = \"demo\"\n");
    let o = lab(none.path(), &["search", "wombat"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("[lab.archive]"), "{}", stderr(&o));
}

#[test]
fn a_configured_stage_that_is_missing_makes_the_answer_partial() {
    let dir = fixture();
    let d = dir.path();
    // No configuration asks for the later stages: their absence is not partial.
    let plain = json(d, &["wombat"]);
    assert_eq!(plain["state"], "hits");
    assert_eq!(plain["stages"]["lexical"], "ready");
    assert_eq!(plain["stages"]["dense"], "missing");
    assert_eq!(plain["partial"].as_array().unwrap().len(), 0);

    write(
        d,
        "research.toml",
        &format!("{CONFIG}\n[lab.search]\nstages = [\"lexical\", \"dense\"]\n"),
    );
    let o = lab(d, &["search", "--json", "wombat"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(
        stderr(&o).contains("lab: partial: stage `dense` is missing"),
        "{}",
        stderr(&o)
    );
    let report: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(report["state"], "partial");
    assert!(report["partial"][0].as_str().unwrap().contains("`dense`"));
    assert_eq!(paths(&report), ["docs/history/01-2026-10-02-start.md"]);

    write(
        d,
        "research.toml",
        &format!("{CONFIG}\n[lab.search]\nstages = [\"lexical\", \"psychic\"]\n"),
    );
    let bad = lab(d, &["search", "wombat"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(stderr(&bad).contains("psychic"), "{}", stderr(&bad));
}

#[test]
fn a_stale_index_answers_from_its_last_state_and_says_so() {
    let dir = fixture();
    let d = dir.path();
    assert_eq!(json(d, &["wombat"])["state"], "hits");
    write(
        d,
        "docs/history/04-2026-10-05-new.md",
        "# New\n\nA quokka.\n",
    );

    // Another process holds the write lock: the reader does not wait.
    let lock = std::fs::File::open(d.join(".research/lab/search/lock")).unwrap();
    lock.lock().unwrap();
    let o = lab(d, &["search", "--json", "wombat"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(
        stderr(&o).contains("lab: partial: the index is stale"),
        "{}",
        stderr(&o)
    );
    let report: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(report["state"], "partial");
    assert!(
        report["partial"][0]
            .as_str()
            .unwrap()
            .contains("1 changed file(s)")
    );
    // The new file is not in the index yet: no hit, and the notice stays.
    let o = lab(d, &["search", "--json", "quokka"]);
    assert_eq!(o.status.code(), Some(0));
    let report: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(report["state"], "zero");
    assert_eq!(report["partial"].as_array().unwrap().len(), 1);
    lock.unlock().unwrap();
    drop(lock);
    assert_eq!(json(d, &["quokka"])["state"], "hits");

    // A refresh that reaches its budget stops, and the next one continues.
    write(
        d,
        "docs/history/05-2026-10-06-more.md",
        "# More\n\nA bilby.\n",
    );
    write(
        d,
        "research.toml",
        &format!("{CONFIG}\n[lab.search]\nrefresh_budget_ms = 0\n"),
    );
    let report = json(d, &["bilby"]);
    assert_eq!(report["state"], "zero");
    assert!(report["partial"][0].as_str().unwrap().contains("budget"));
    write(d, "research.toml", CONFIG);
    assert_eq!(json(d, &["bilby"])["state"], "hits");
}
