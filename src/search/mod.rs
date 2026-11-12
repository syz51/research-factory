//! `lab search`: ranked full-text search over the project's notes
//! (docs/archive-design.md section 3). This is the lexical core: the `notes`
//! scope and the `lexical` stage. Later scopes join `SCOPES`; later stages
//! change their entry in `Stage::state`.
//!
//! Retrieved text is data: nothing here runs or follows what a note says.

mod index;
pub(crate) mod learn;
mod missed;
mod query;
pub(crate) mod similar;
mod thesaurus;
mod topics;
pub(crate) mod trace;
mod units;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::archive::Spec;
use crate::archive::files::FileSet;
use crate::config::Project;
use query::{Expansion, Hit, Outcome, Unknown};

/// The scopes `--in` accepts.
const SCOPES: &[&str] = &["notes"];
const DEFAULT_LIMIT: usize = 8;
/// The text output holds about this many bytes per hit of the limit: 2 KB at 8 hits.
const BYTES_PER_HIT: usize = 256;

/// `[lab.search]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SearchConfig {
    /// Globs of the files in the `notes` scope (default: every `.md` file
    /// below the directories that the globs of `[lab.archive]` name).
    pub notes: Vec<String>,
    pub notes_exclude: Vec<String>,
    /// Source classes by path; a file in none is `own`.
    pub classes: ClassGlobs,
    /// The stages a query should run. One that is not built makes the query `partial`.
    pub stages: Vec<Stage>,
    /// A refresh of changed files that takes longer stops, and the query
    /// answers from a stale index.
    pub refresh_budget_ms: u64,
    /// How long a query waits for another process that builds the index.
    pub lock_timeout_ms: u64,
    /// The tracked thesaurus file of equivalent terms, relative to the root.
    pub thesaurus: Option<String>,
    /// `lab retro` lists a thesaurus entry that no query used in this many days.
    pub review_days: u32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            notes: Vec::new(),
            notes_exclude: Vec::new(),
            classes: ClassGlobs::default(),
            stages: vec![Stage::Lexical],
            refresh_budget_ms: 5_000,
            lock_timeout_ms: 30_000,
            thesaurus: None,
            review_days: 90,
        }
    }
}

/// `[lab.search.classes]`: globs per source class.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClassGlobs {
    pub paper: Vec<String>,
    pub copied: Vec<String>,
    pub generated: Vec<String>,
    pub chat: Vec<String>,
    pub historical: Vec<String>,
}

/// What kind of text a unit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Own,
    Paper,
    Copied,
    Generated,
    Chat,
    Historical,
}

impl Class {
    fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Paper => "paper",
            Self::Copied => "copied",
            Self::Generated => "generated",
            Self::Chat => "chat",
            Self::Historical => "historical",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        [
            Self::Own,
            Self::Paper,
            Self::Copied,
            Self::Generated,
            Self::Chat,
            Self::Historical,
        ]
        .into_iter()
        .find(|c| c.as_str() == s)
    }

    /// The share of its score a unit of this class keeps: the project's own
    /// notes rank above copied pages, and historical pages rank last.
    fn prior(self) -> f64 {
        match self {
            Self::Own => 1.0,
            Self::Paper => 0.8,
            Self::Chat => 0.6,
            Self::Copied => 0.5,
            Self::Historical => 0.25,
            // Never indexed.
            Self::Generated => 0.0,
        }
    }
}

/// A stage of the retrieval pipeline (docs/state-machines.md 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Lexical,
    Dense,
    Rerank,
    Graph,
}

impl Stage {
    const ALL: [Self; 4] = [Self::Lexical, Self::Dense, Self::Rerank, Self::Graph];

    fn name(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Dense => "dense",
            Self::Rerank => "rerank",
            Self::Graph => "graph",
        }
    }

    /// `ready`, `missing` or `failed`.
    fn state(self) -> &'static str {
        match self {
            Self::Lexical => "ready",
            Self::Dense | Self::Rerank | Self::Graph => "missing",
        }
    }
}

/// What a query needs besides the index: the project, its archive and the
/// compiled `[lab.search]`.
pub struct Context<'p> {
    project: &'p Project,
    cfg: &'p SearchConfig,
    spec: Spec<'p>,
    notes: FileSet,
    classes: Vec<(Class, FileSet)>,
    inactive_statuses: &'p [String],
    /// Where this checkout keeps its index. A linked worktree has its own.
    dir: PathBuf,
    /// Of the configuration the index depends on; a change rebuilds it.
    fingerprint: String,
    /// false: a refresh indexes every changed file, past `refresh_budget_ms`.
    budgeted: bool,
}

/// The directory a glob names before its first pattern character.
fn literal_dir(glob: &str) -> Option<String> {
    let parts: Vec<&str> = glob.split('/').collect();
    let pattern = parts
        .iter()
        .position(|p| p.contains(['*', '?', '[', '{']))?;
    Some(parts[..pattern].join("/"))
}

/// Every Markdown file below the directories that `[lab.archive]` names. A
/// glob for files in the project root, and a path without a pattern, stay as
/// they are.
fn default_notes(archive: &crate::config::ArchiveConfig) -> Vec<String> {
    let globs = archive
        .index
        .iter()
        .flat_map(|i| &i.include)
        .chain(&archive.sealed)
        .chain(&archive.links)
        .chain(&archive.current_pages)
        .chain(&archive.start);
    let mut notes: Vec<String> = globs
        .map(|g| match literal_dir(g) {
            Some(dir) if !dir.is_empty() => format!("{dir}/**/*.md"),
            _ => g.clone(),
        })
        .collect();
    notes.sort();
    notes.dedup();
    notes
}

impl<'p> Context<'p> {
    fn new(project: &'p Project) -> Result<Self, String> {
        let spec = Spec::new(project)?;
        let archive = project
            .lab
            .archive
            .as_ref()
            .ok_or("no [lab.archive] table in research.toml")?;
        let cfg = &project.lab.search;
        let notes = if cfg.notes.is_empty() {
            default_notes(archive)
        } else {
            cfg.notes.clone()
        };
        let set = |what: &str, include: &[String], exclude: &[String]| {
            FileSet::new(include, exclude).map_err(|e| format!("[lab.search] {what}: {e}"))
        };
        // An index output is generated text whatever the classes say.
        let outputs: Vec<String> = archive.index.iter().map(|i| i.output.clone()).collect();
        let generated: Vec<String> = [outputs, cfg.classes.generated.clone()].concat();
        let classes = [
            (Class::Generated, &generated),
            (Class::Copied, &cfg.classes.copied),
            (Class::Paper, &cfg.classes.paper),
            (Class::Chat, &cfg.classes.chat),
            (Class::Historical, &cfg.classes.historical),
        ];
        let fingerprint = Sha256::digest(format!(
            "{archive:?}\n{notes:?}\n{:?}\n{:?}",
            cfg.notes_exclude, cfg.classes
        ));
        Ok(Self {
            project,
            cfg,
            spec,
            notes: set("notes", &notes, &cfg.notes_exclude)?,
            classes: classes
                .into_iter()
                .map(|(class, globs)| Ok((class, set(class.as_str(), globs, &[])?)))
                .collect::<Result<_, String>>()?,
            inactive_statuses: &archive.inactive_statuses,
            dir: project.state_dir.join("lab").join("search"),
            fingerprint: fingerprint.iter().map(|b| format!("{b:02x}")).collect(),
            budgeted: true,
        })
    }

    fn class_of(&self, path: &str) -> Class {
        self.classes
            .iter()
            .find(|(_, set)| set.matches(path))
            .map_or(Class::Own, |(class, _)| *class)
    }
}

/// The arguments of `lab search`.
pub struct Args {
    pub query: Vec<String>,
    pub limit: Option<usize>,
    pub statuses: Vec<String>,
    pub records: bool,
    pub json: bool,
    pub scopes: Vec<String>,
    pub explain: bool,
    pub rebuild: bool,
    /// A topics file to answer, one query per topic, as a TREC run.
    pub run: Option<PathBuf>,
    /// Where `--run` writes the run; default stdout.
    pub out: Option<PathBuf>,
    /// `--run` also searches each topic's `phrasings`.
    pub phrasings: bool,
}

/// How a query ended (docs/state-machines.md 3.3). `error` is the `Err` of `run`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum State {
    Hits,
    Partial,
    Zero,
    Error,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            Self::Hits => "hits",
            Self::Partial => "partial",
            Self::Zero => "zero",
            Self::Error => "error",
        }
    }
}

/// The `--json` output, schema `rf.search/1`.
#[derive(Serialize)]
struct Report<'a> {
    schema: &'static str,
    state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
    query: &'a [String],
    scopes: &'a [String],
    stages: BTreeMap<&'static str, &'static str>,
    /// Why the answer may be incomplete: a stale index, a stage that is not built.
    partial: &'a [String],
    unknown: &'a [Unknown],
    /// The thesaurus entries the query used.
    #[serde(skip_serializing_if = "<[Expansion]>::is_empty")]
    expansions: &'a [Expansion],
    hits: &'a [Hit],
    more: usize,
}

/// One query, whoever asks it.
pub(crate) struct Request<'a> {
    pub phrasings: &'a [String],
    pub scopes: &'a [String],
    pub limit: usize,
    pub records: bool,
    pub statuses: &'a [String],
    pub rebuild: bool,
    /// Also weigh the terms of the first phrasing, for `similar`.
    pub weigh: bool,
}

/// The answer to one query.
pub(crate) struct Answer {
    pub state: State,
    pub hits: Vec<Hit>,
    pub more: usize,
    pub unknown: Vec<Unknown>,
    pub partial: Vec<String>,
    pub expansions: Vec<Expansion>,
    /// The best units of the lexical stage before grouping.
    pub candidates: Vec<(String, f64)>,
    pub missing_stages: Vec<&'static str>,
    /// With `weigh`: each term of the first phrasing and its weight.
    pub weights: Vec<(String, f64)>,
}

/// `lab search`. Exit 0 for hits, a partial answer or no hit; `Err` (exit 2)
/// for a bad query, an unreadable index or a lock timeout.
pub fn run(project: &Project, args: &Args) -> Result<ExitCode, String> {
    if let Some(topics) = &args.run {
        return topics::run(project, args, topics);
    }
    // Several arguments are several phrasings when one of them holds a
    // space; otherwise they are the words of one.
    let phrasings: Vec<String> =
        if args.query.len() > 1 && args.query.iter().any(|q| q.contains(char::is_whitespace)) {
            args.query.clone()
        } else if args.query.is_empty() {
            Vec::new()
        } else {
            vec![args.query.join(" ")]
        };
    if phrasings.is_empty() && args.rebuild {
        check_scopes(&args.scopes)?;
        let ctx = Context::new(project)?;
        let opened = index::open(&ctx, true)?;
        let (files, units) = index::counts(&opened.conn).map_err(String::from)?;
        println!("Search index rebuilt: {units} unit(s) of {files} file(s).");
        return Ok(ExitCode::SUCCESS);
    }
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
    let request = Request {
        phrasings: &phrasings,
        scopes: &args.scopes,
        limit,
        records: args.records,
        statuses: &args.statuses,
        rebuild: args.rebuild,
        weigh: false,
    };
    let mut answer = match traced(project, "cli", &request) {
        Ok(answer) => answer,
        Err(e) => {
            if args.json {
                let report = Report {
                    schema: "rf.search/1",
                    state: State::Error,
                    error: Some(&e),
                    query: &phrasings,
                    scopes: &args.scopes,
                    stages: stages(),
                    partial: &[],
                    unknown: &[],
                    expansions: &[],
                    hits: &[],
                    more: 0,
                };
                println!(
                    "{}",
                    serde_json::to_string(&report).map_err(|e| e.to_string())?
                );
            }
            return Err(e);
        }
    };
    if !args.explain {
        for hit in &mut answer.hits {
            hit.explain = None;
        }
    }
    for notice in &answer.partial {
        eprintln!("lab: partial: {notice}");
    }
    if args.json {
        let report = Report {
            schema: "rf.search/1",
            state: answer.state,
            error: None,
            query: &phrasings,
            scopes: &args.scopes,
            stages: stages(),
            partial: &answer.partial,
            unknown: &answer.unknown,
            expansions: &answer.expansions,
            hits: &answer.hits,
            more: answer.more,
        };
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|e| e.to_string())?
        );
    } else {
        print!(
            "{}",
            render(
                &answer.hits,
                answer.more,
                &answer.unknown,
                &answer.expansions,
                args.explain,
                limit
            )
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Builds the search index, or brings it up to date with the files, without
/// a query. Gives the files and units it holds.
pub fn refresh(project: &Project) -> Result<(i64, i64), String> {
    // A gate must leave the index complete, so it is not held to the budget
    // of a query's refresh.
    let ctx = Context {
        budgeted: false,
        ..Context::new(project)?
    };
    let opened = index::open(&ctx, false)?;
    if let Some(why) = opened.stale {
        return Err(format!("the search index is stale: {why}"));
    }
    index::counts(&opened.conn).map_err(String::from)
}

fn stages() -> BTreeMap<&'static str, &'static str> {
    Stage::ALL
        .into_iter()
        .map(|s| (s.name(), s.state()))
        .collect()
}

fn check_scopes(scopes: &[String]) -> Result<(), String> {
    match scopes.iter().find(|s| !SCOPES.contains(&s.as_str())) {
        Some(unknown) => Err(format!(
            "unknown scope `{unknown}`; the scopes are: {}",
            SCOPES.join(", ")
        )),
        None => Ok(()),
    }
}

/// Answers the request and appends its line to the query trace, also when
/// it fails. `source` says who asked: `cli`, `run` or `similar`.
pub(crate) fn traced(project: &Project, source: &str, req: &Request) -> Result<Answer, String> {
    let started = Instant::now();
    let answered = execute(project, req);
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut line = trace::QueryLine {
        schema: trace::QUERY_SCHEMA.into(),
        at: crate::finding::now(),
        session: trace::session(),
        source: source.into(),
        query: req.phrasings.to_vec(),
        scopes: req.scopes.to_vec(),
        state: State::Error.name().into(),
        stages: Vec::new(),
        missing_stages: Vec::new(),
        expansions: Vec::new(),
        candidates: BTreeMap::new(),
        hit_count: 0,
        complete: 0,
        hits: Vec::new(),
        unknown: Vec::new(),
        notices: Vec::new(),
        error: None,
        latency_ms,
    };
    match &answered {
        Ok(a) => {
            line.state = a.state.name().into();
            line.stages = vec![Stage::Lexical.name().into()];
            line.missing_stages = a.missing_stages.iter().map(|s| s.to_string()).collect();
            line.expansions = a.expansions.clone();
            line.candidates.insert(
                Stage::Lexical.name().into(),
                a.candidates
                    .iter()
                    .map(|(id, score)| trace::Scored {
                        id: id.clone(),
                        score: *score,
                    })
                    .collect(),
            );
            line.hit_count = a.hits.len() + a.more;
            line.complete = a.hits.iter().filter(|h| h.missing.is_empty()).count();
            line.hits = a.hits.iter().map(|h| h.anchor.clone()).collect();
            line.unknown = a.unknown.iter().map(|u| u.word.clone()).collect();
            line.notices = a.partial.clone();
        }
        Err(e) => line.error = Some(e.clone()),
    }
    trace::record(project, &line);
    answered
}

fn execute(project: &Project, req: &Request) -> Result<Answer, String> {
    check_scopes(req.scopes)?;
    if req.phrasings.is_empty() {
        return Err("give the words to search for, or --rebuild".into());
    }
    if req.limit == 0 {
        return Err("--limit must be at least 1".into());
    }
    let ctx = Context::new(project)?;
    let archive_statuses = project
        .lab
        .archive
        .as_ref()
        .map_or(&[][..], |a| &a.statuses[..]);
    if let Some(unknown) = req.statuses.iter().find(|s| !archive_statuses.contains(s)) {
        return Err(format!(
            "unknown status `{unknown}`; the statuses are: {}",
            archive_statuses.join(", ")
        ));
    }
    let mut parsed: Vec<query::Phrasing> = req
        .phrasings
        .iter()
        .map(|p| query::parse(p))
        .collect::<Result<_, _>>()?;
    let entries = thesaurus::load(project)?;
    let mut expansions: Vec<Expansion> = Vec::new();
    for phrasing in &mut parsed {
        for e in phrasing.expand(&entries) {
            if !expansions.contains(&e) {
                expansions.push(e);
            }
        }
    }
    let opts = query::Options {
        limit: req.limit,
        records: req.records,
        statuses: req.statuses,
    };
    let mut rebuild = req.rebuild;
    let (outcome, stale, weights) = loop {
        let opened = index::open(&ctx, rebuild)?;
        let searched = query::search(&opened.conn, &ctx, &parsed, &opts).and_then(|outcome| {
            let weights = if req.weigh {
                query::weights(&opened.conn, &parsed[0])?
            } else {
                Vec::new()
            };
            Ok((outcome, weights))
        });
        match searched {
            Ok((outcome, weights)) => break (outcome, opened.stale, weights),
            // Corruption that only a query met: build anew, once.
            Err(index::Fault::Corrupt(_)) if !rebuild => rebuild = true,
            Err(e) => return Err(e.into()),
        }
    };
    let Outcome {
        hits,
        more,
        unknown,
        candidates,
    } = outcome;
    let mut partial: Vec<String> = stale
        .into_iter()
        .map(|why| format!("the index is stale: {why}"))
        .collect();
    let mut missing_stages = Vec::new();
    for stage in ctx.cfg.stages.iter().filter(|s| s.state() != "ready") {
        missing_stages.push(stage.name());
        partial.push(format!(
            "stage `{}` is {}; the results come without it",
            stage.name(),
            stage.state()
        ));
    }
    let state = if hits.is_empty() {
        State::Zero
    } else if partial.is_empty() {
        State::Hits
    } else {
        State::Partial
    };
    Ok(Answer {
        state,
        hits,
        more,
        unknown,
        partial,
        expansions,
        candidates,
        missing_stages,
        weights,
    })
}

fn unknown_line(unknown: &[Unknown]) -> String {
    let words: Vec<String> = unknown
        .iter()
        .map(|u| {
            if u.nearest.is_empty() {
                u.word.clone()
            } else {
                format!("{} (nearest: {})", u.word, u.nearest.join(", "))
            }
        })
        .collect();
    format!("Not in the index: {}\n", words.join("; "))
}

/// The text output: at most `limit` hits and about `limit * BYTES_PER_HIT` bytes.
fn render(
    hits: &[Hit],
    more: usize,
    unknown: &[Unknown],
    expansions: &[Expansion],
    explain: bool,
    limit: usize,
) -> String {
    let mut out = String::new();
    // The thesaurus terms a query added are always named.
    for e in expansions {
        let _ = write!(
            out,
            "Also searched: {} (for {})",
            e.added.join(", "),
            e.term
        );
        if explain {
            let _ = write!(out, " · thesaurus: {}", e.entry);
        }
        out.push('\n');
    }
    if hits.is_empty() {
        out.push_str("0 results\n");
        if !unknown.is_empty() {
            out.push_str(&unknown_line(unknown));
        }
        let _ = writeln!(out, "Searched: {}", SCOPES.join(", "));
        return out;
    }
    let budget = limit * BYTES_PER_HIT;
    let mut shown = 0;
    let mut record = None;
    for hit in hits {
        let mut text = String::new();
        let r = &hit.record;
        if record == Some(&hit.path) {
            let _ = writeln!(text, "{}. (same record)", hit.rank);
        } else {
            let mut parts = vec![r.id.clone().unwrap_or_else(|| "-".into())];
            parts.extend(r.date.clone());
            parts.extend(r.status.clone());
            if r.class != Class::Own.as_str() {
                parts.push(r.class.clone());
            }
            if !r.superseded_by.is_empty() {
                parts.push(format!("superseded by {}", r.superseded_by.join(", ")));
            }
            parts.extend(r.title.clone());
            let _ = writeln!(text, "{}. {}", hit.rank, parts.join(" · "));
        }
        let _ = write!(text, "   {}", hit.anchor);
        if !hit.heading.is_empty() {
            let _ = write!(text, " · {}", hit.heading);
        }
        let _ = writeln!(text, "\n   {}", hit.snippet);
        if !hit.missing.is_empty() {
            let _ = writeln!(text, "   missing: {}", hit.missing.join(", "));
        }
        if let Some(e) = &hit.explain {
            let _ = writeln!(
                text,
                "   explain: exact_id={} bm25={:.3} trigram={:.3} prior={} score={:.3} fused={:.5} matched={}",
                e.exact_id,
                e.bm25,
                e.trigram,
                e.prior,
                e.score,
                e.fused,
                e.matched.join(",")
            );
        }
        if shown > 0 && out.len() + text.len() > budget {
            break;
        }
        out.push_str(&text);
        shown += 1;
        record = Some(&hit.path);
    }
    if !unknown.is_empty() {
        out.push_str(&unknown_line(unknown));
    }
    let more = more + hits.len() - shown;
    if more > 0 {
        let _ = writeln!(
            out,
            "{more} more hit(s): add a word, or pass --limit {}.",
            shown + more
        );
    }
    out
}
