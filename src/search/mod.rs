//! `lab search`: ranked full-text search over the project's notes, the
//! reference registry, extracted paper texts, run outputs, agent sessions and
//! the work board (docs/archive-design.md section 3). This is the lexical
//! core: every scope of `Scope` and the `lexical` stage. Later stages change
//! their entry in `Stage::state`.
//!
//! Retrieved text is data: nothing here runs or follows what a note says.

mod board;
mod chat;
mod graph;
mod index;
pub(crate) mod learn;
mod mcp;
mod missed;
mod open;
mod passages;
mod query;
mod refs;
pub(crate) mod similar;
mod sources;
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

/// A part of the corpus (docs/archive-design.md 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Notes,
    Refs,
    Texts,
    Files,
    Sessions,
    Board,
}

impl Scope {
    /// In the order the index builds them: `files` names the records of `notes`.
    pub const ALL: [Self; 6] = [
        Self::Notes,
        Self::Refs,
        Self::Texts,
        Self::Files,
        Self::Sessions,
        Self::Board,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::Refs => "refs",
            Self::Texts => "texts",
            Self::Files => "files",
            Self::Sessions => "sessions",
            Self::Board => "board",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

fn default_scopes() -> Vec<Scope> {
    vec![Scope::Notes, Scope::Refs, Scope::Texts]
}

/// The scopes a query searches: those named, `all`, or else the configured default.
fn requested(named: &[String], default: &[Scope]) -> Result<Vec<Scope>, String> {
    let mut scopes = Vec::new();
    for name in named.iter().flat_map(|n| n.split(',')).map(str::trim) {
        match name {
            "" => {}
            "all" => scopes.extend(Scope::ALL),
            _ => scopes.push(Scope::parse(name).ok_or_else(|| {
                format!(
                    "unknown scope `{name}`; the scopes are: {}, all",
                    Scope::ALL.map(Scope::as_str).join(", ")
                )
            })?),
        }
    }
    if scopes.is_empty() {
        scopes = default.to_vec();
    }
    scopes.sort();
    scopes.dedup();
    Ok(scopes)
}
const DEFAULT_LIMIT: usize = 8;
/// Without a given limit, the text output holds about this many bytes: 8 hits
/// and about 2 KB. A given limit prints that many hits.
const DEFAULT_BUDGET: usize = 2048;

/// The byte budget of the text output: only when the caller gave no limit.
fn budget(limit: Option<usize>) -> Option<usize> {
    limit.is_none().then_some(DEFAULT_BUDGET)
}

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
    /// Regular expressions of a citation, each with a group named `key`
    /// (default: `ref:<key>` and a link to `#reference-<key>`).
    pub cites: Vec<String>,
    /// The tracked thesaurus file of equivalent terms, relative to the root.
    pub thesaurus: Option<String>,
    /// `lab retro` lists a thesaurus entry that no query used in this many days.
    pub review_days: u32,
    /// The scopes a query without `--in` searches.
    pub default_scopes: Vec<Scope>,
    /// Globs of the paper cards (`rf.paper-card/1`) of the `refs` scope
    /// (default: `sources/cards/*.json` beside the registry).
    pub cards: Option<Vec<String>>,
    /// Globs of the files of the `files` scope (default: the files git
    /// tracks that are not Markdown and not notes).
    pub files: Option<Vec<String>>,
    pub files_exclude: Vec<String>,
    /// The command whose JSON feeds the `board` scope (default:
    /// `[lab.archive] board_export`).
    pub board_export: Option<Vec<String>>,
    /// A query refreshes a cached board export older than this many minutes.
    pub board_max_age: u64,
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
            cites: Vec::new(),
            thesaurus: None,
            review_days: 90,
            default_scopes: default_scopes(),
            cards: None,
            files: None,
            files_exclude: Vec::new(),
            board_export: None,
            board_max_age: 15,
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
    /// notes rank above copied pages, and historical pages rank last. Papers
    /// and paper cards keep their whole score: a prior of 0.8 on papers cost
    /// the masters judged set 0.04 nDCG@10, mostly on reference and passage
    /// topics, and 0.5 on paper cards cost it 0.01 (research-factory-295).
    fn prior(self) -> f64 {
        match self {
            // Generated text is only paper cards, which route to a paper;
            // generated notes are never indexed.
            Self::Own | Self::Paper | Self::Generated => 1.0,
            Self::Chat => 0.6,
            Self::Copied => 0.5,
            Self::Historical => 0.25,
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

    /// `ready`, `missing` or `failed`. An index of this version always holds
    /// the edge table of `graph`; an older one is rebuilt before a query.
    fn state(self) -> &'static str {
        match self {
            Self::Lexical | Self::Graph => "ready",
            Self::Dense | Self::Rerank => "missing",
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
    /// The citation patterns of `[lab.search] cites`.
    cites: Vec<regex::Regex>,
    /// Where this checkout keeps its index. A linked worktree has its own.
    dir: PathBuf,
    /// Of the configuration the index depends on; a change rebuilds it.
    fingerprint: String,
    /// false: a refresh indexes every changed file, past `refresh_budget_ms`.
    budgeted: bool,
    /// The scopes this query searches, in build order; `files` brings `notes`.
    scopes: Vec<Scope>,
    /// The scopes the answer comes from.
    searched: Vec<Scope>,
    /// The registry, relative to the root.
    registry: String,
    cards: FileSet,
    files: Option<FileSet>,
    /// The `rf.session/1` list, relative to the root.
    sessions: Option<String>,
    board_export: Option<Vec<String>>,
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
    fn new(project: &'p Project, searched: Vec<Scope>) -> Result<Self, String> {
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
            "{archive:?}\n{notes:?}\n{:?}\n{:?}\n{:?}",
            cfg.notes_exclude, cfg.classes, cfg.cites
        ));
        let registry = project.registry.clone();
        let cards = cfg.cards.clone().unwrap_or_else(|| {
            let dir = registry.rsplit_once('/').map_or("", |(d, _)| d);
            let dir = if dir.is_empty() {
                String::new()
            } else {
                format!("{dir}/")
            };
            vec![format!("{dir}sources/cards/*.json")]
        });
        let files = match &cfg.files {
            Some(globs) => Some(set("files", globs, &cfg.files_exclude)?),
            None => None,
        };
        // The session list beside the findings of `agentprof scan`.
        let sessions = project.lab.agent_sessions.clone().or_else(|| {
            let findings = project.lab.agent_findings.as_ref()?;
            Some(match findings.rsplit_once('/') {
                Some((dir, _)) => format!("{dir}/sessions.jsonl"),
                None => "sessions.jsonl".into(),
            })
        });
        let mut scopes = searched.clone();
        if scopes.contains(&Scope::Files) && !scopes.contains(&Scope::Notes) {
            scopes.insert(0, Scope::Notes);
        }
        Ok(Self {
            scopes,
            searched,
            cards: set("cards", &cards, &[])?,
            files,
            sessions,
            board_export: board::command(project),
            registry,
            project,
            cfg,
            spec,
            notes: set("notes", &notes, &cfg.notes_exclude)?,
            classes: classes
                .into_iter()
                .map(|(class, globs)| Ok((class, set(class.as_str(), globs, &[])?)))
                .collect::<Result<_, String>>()?,
            inactive_statuses: &archive.inactive_statuses,
            cites: graph::cite_patterns(&cfg.cites)?,
            dir: board::dir(project),
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

/// The graph and date filters of `lab search`.
#[derive(Debug, Default, Clone, clap::Args)]
pub struct GraphArgs {
    /// Only units that cite this reference key (`232` or `ref:232`); without words, list them all.
    #[arg(long, value_name = "KEY")]
    pub cites: Option<String>,
    /// Only units that link this record id or path; without words, list them all.
    #[arg(long, value_name = "ID_OR_PATH")]
    pub links_to: Option<String>,
    /// Rank by the records in force on this date (YYYY-MM-DD); later records are left out.
    #[arg(long, value_name = "DATE")]
    pub as_of: Option<String>,
    /// Only records dated on or after this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub since: Option<String>,
    /// Only records dated on or before this date (YYYY-MM-DD).
    #[arg(long, value_name = "DATE")]
    pub until: Option<String>,
    /// Fold a unit that links a better hit's record under that hit, as an
    /// "also in" line, instead of showing it as a hit of its own.
    #[arg(long)]
    pub fold: bool,
    /// No effect since 0.19.0, when each unit became a hit of its own by default.
    #[arg(long, hide = true)]
    pub all: bool,
}

impl GraphArgs {
    fn filters(&self) -> graph::Filters {
        graph::Filters {
            as_of: self.as_of.clone(),
            since: self.since.clone(),
            until: self.until.clone(),
            cites: self.cites.clone(),
            links_to: self.links_to.clone(),
        }
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
    pub graph: GraphArgs,
    /// A topics file to answer, one query per topic, as a TREC run.
    pub run: Option<PathBuf>,
    /// Where `--run` writes the run; default stdout.
    pub out: Option<PathBuf>,
    /// `--run` also searches each topic's `phrasings`.
    pub phrasings: bool,
    /// Run the board export now and cache it.
    pub refresh_board: bool,
    /// This process is the detached refresh a query started.
    pub board_child: bool,
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
    /// The scopes searched.
    scopes: Vec<&'static str>,
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
    /// The graph filters (`--cites`, `--links-to`, dates) and folding.
    pub graph: &'a GraphArgs,
    /// What a refresh earlier in this batch of queries said. The query then
    /// uses the index as it is and does not list the sources again.
    pub listed: Option<&'a index::Listed>,
}

/// The answer to one query.
pub(crate) struct Answer {
    pub state: State,
    pub hits: Vec<Hit>,
    pub more: usize,
    pub unknown: Vec<Unknown>,
    pub partial: Vec<String>,
    /// What the listings said that leaves the answer complete: `Listing::info`.
    pub info: Vec<String>,
    pub expansions: Vec<Expansion>,
    /// The best units of the lexical stage before grouping.
    pub candidates: Vec<(String, f64)>,
    pub missing_stages: Vec<&'static str>,
    /// With `weigh`: each term of the first phrasing and its weight.
    pub weights: Vec<(String, f64)>,
    /// The scopes searched.
    pub searched: Vec<Scope>,
}

impl Answer {
    /// One line: how the query ended and the stages it ran without.
    pub fn state_line(&self) -> String {
        let missing: Vec<&str> = Stage::ALL
            .into_iter()
            .filter(|s| s.state() != "ready")
            .map(|s| s.name())
            .collect();
        let mut line = format!("state: {}", self.state.name());
        if !missing.is_empty() {
            let _ = write!(line, "; missing stages: {}", missing.join(", "));
        }
        for notice in &self.partial {
            let _ = write!(line, "; {notice}");
        }
        line
    }
}

/// Several arguments are several phrasings when one of them holds a space;
/// otherwise they are the words of one.
pub fn phrasings(query: &[String]) -> Vec<String> {
    if query.len() > 1 && query.iter().any(|q| q.contains(char::is_whitespace)) {
        query.to_vec()
    } else if query.is_empty() {
        Vec::new()
    } else {
        vec![query.join(" ")]
    }
}

/// `lab related`.
pub fn related(
    project: &Project,
    name: &str,
    limit: usize,
    json: bool,
) -> Result<ExitCode, String> {
    graph::run(project, name, limit, json)
}

/// Lines of text `lab open` prints by default.
pub const OPEN_LINES: usize = open::DEFAULT_LINES;

/// `lab open`.
pub fn open(project: &Project, anchor: &str, lines: usize, json: bool) -> Result<ExitCode, String> {
    open::run(project, anchor, lines, json)
}

/// `lab mcp`.
pub fn mcp(project: &Project) -> Result<ExitCode, String> {
    mcp::run(project)
}

/// The `rf.search/1` report of a query that failed with `error`.
fn print_error(
    project: &Project,
    args: &Args,
    phrasings: &[String],
    error: &str,
) -> Result<(), String> {
    let scopes = requested(&args.scopes, &project.lab.search.default_scopes).unwrap_or_default();
    let report = Report {
        schema: "rf.search/1",
        state: State::Error,
        error: Some(error),
        query: phrasings,
        scopes: scopes.iter().map(|s| s.as_str()).collect(),
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
    Ok(())
}

/// `lab search`. Exit 0 for hits, a partial answer or no hit; `Err` (exit 2)
/// for a bad query, an unreadable index or a lock timeout.
pub fn run(project: &Project, args: &Args) -> Result<ExitCode, String> {
    if args.board_child {
        // Its output goes to board.log.
        if let Some(r) = board::refresh(project, true)? {
            println!("{r}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    let phrasings = phrasings(&args.query);
    if args.refresh_board {
        let r = match board::refresh(project, false) {
            Ok(r) => r.expect("a foreground refresh runs the export"),
            Err(e) => {
                if args.json {
                    print_error(project, args, &phrasings, &e)?;
                }
                return Err(e);
            }
        };
        if phrasings.is_empty()
            && args.run.is_none()
            && !args.rebuild
            && !args.graph.filters().lists()
        {
            println!("{r}");
            return Ok(ExitCode::SUCCESS);
        }
        eprintln!("lab: {r}");
    }
    if let Some(topics) = &args.run {
        return topics::run(project, args, topics);
    }
    if phrasings.is_empty() && args.rebuild && !args.graph.filters().lists() {
        let scopes = requested(&args.scopes, &project.lab.search.default_scopes)?;
        let ctx = Context::new(project, scopes)?;
        let opened = index::open(&ctx, true)?;
        for line in &opened.info {
            eprintln!("lab: {line}");
        }
        for notice in &opened.notices {
            eprintln!("lab: partial: {notice}");
        }
        let (files, units) = index::counts(&opened.conn).map_err(String::from)?;
        let names: Vec<&str> = ctx.scopes.iter().map(|s| s.as_str()).collect();
        println!(
            "Search index rebuilt: {units} unit(s) of {files} source(s) in {}.",
            names.join(", ")
        );
        if ctx.scopes.contains(&Scope::Sessions) {
            let left: Vec<String> = index::left_out(&opened.conn)
                .map_err(String::from)?
                .iter()
                .map(|(rule, n)| format!("{rule} {n}"))
                .collect();
            println!(
                "Session messages left out by rule: {}.",
                if left.is_empty() {
                    "none".into()
                } else {
                    left.join(", ")
                }
            );
        }
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
        graph: &args.graph,
        listed: None,
    };
    let mut answer = match traced(project, "cli", &request) {
        Ok(answer) => answer,
        Err(e) => {
            if args.json {
                print_error(project, args, &phrasings, &e)?;
            }
            return Err(e);
        }
    };
    if !args.explain {
        for hit in &mut answer.hits {
            hit.explain = None;
        }
    }
    for line in &answer.info {
        eprintln!("lab: {line}");
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
            scopes: answer.searched.iter().map(|s| s.as_str()).collect(),
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
                budget(args.limit),
                &answer.searched
            )
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Builds the search index of the default scopes, or brings it up to date
/// with their sources, without a query. Gives the sources and units it holds.
pub fn refresh(project: &Project) -> Result<(i64, i64), String> {
    // A gate must leave the index complete, so it is not held to the budget
    // of a query's refresh.
    let ctx = Context {
        budgeted: false,
        ..Context::new(project, project.lab.search.default_scopes.clone())?
    };
    let opened = index::open(&ctx, false)?;
    if let Some(why) = opened.stale {
        return Err(format!("the search index is stale: {why}"));
    }
    index::counts(&opened.conn).map_err(String::from)
}

/// Builds the index of `scopes`, or brings it up to date with their sources,
/// for a batch of queries, and gives what the refresh said. The queries pass
/// it as `Request::listed`, so the batch lists the sources once. The refresh
/// is not held to a query's budget: the batch answers from a complete index.
pub(crate) fn refresh_batch(
    project: &Project,
    scopes: &[String],
    rebuild: bool,
) -> Result<index::Listed, String> {
    let ctx = Context {
        budgeted: false,
        ..Context::new(
            project,
            requested(scopes, &project.lab.search.default_scopes)?,
        )?
    };
    Ok(index::open(&ctx, rebuild)?.into())
}

fn stages() -> BTreeMap<&'static str, &'static str> {
    Stage::ALL
        .into_iter()
        .map(|s| (s.name(), s.state()))
        .collect()
}

/// Every structure file and paper card that no longer fits its text, with
/// why: `lab archive check` reports them as `stale-derivative`.
pub fn stale_derivatives(project: &Project) -> Result<Vec<(String, String)>, String> {
    refs::stale(&Context::new(project, vec![Scope::Refs, Scope::Texts])?)
}

/// Answers the request and appends its line to the query trace, also when
/// it fails. `source` says who asked: `cli`, `mcp`, `run` or `similar`.
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
    let scopes = requested(req.scopes, &project.lab.search.default_scopes)?;
    let filters = req.graph.filters();
    if req.phrasings.is_empty() && !filters.lists() {
        return Err("give the words to search for, --cites, --links-to or --rebuild".into());
    }
    if req.limit == 0 {
        return Err("--limit must be at least 1".into());
    }
    let ctx = Context::new(project, scopes)?;
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
    let mut rebuild = req.rebuild;
    let (outcome, stale, notices, info, weights) = loop {
        let opened = match req.listed {
            Some(listed) if !rebuild => index::open_listed(&ctx, listed)?,
            _ => index::open(&ctx, rebuild)?,
        };
        let filter = filters.units_sql(&opened.conn)?;
        let opts = query::Options {
            limit: req.limit,
            records: req.records,
            statuses: req.statuses,
            filter: filter.as_deref(),
            as_of: filters.as_of.as_deref(),
            fold: req.graph.fold,
        };
        let searched = query::search(&opened.conn, &ctx, &parsed, &opts).and_then(|outcome| {
            let weights = if req.weigh {
                query::weights(&opened.conn, &parsed[0])?
            } else {
                Vec::new()
            };
            Ok((outcome, weights))
        });
        match searched {
            Ok((outcome, weights)) => {
                break (outcome, opened.stale, opened.notices, opened.info, weights);
            }
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
        .chain(notices)
        .chain(parsed.iter().filter(|p| p.dropped > 0).map(|p| {
            format!(
                "a phrasing has more than {} terms; its last {} were left out",
                query::MAX_TERMS,
                p.dropped
            )
        }))
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
        info,
        expansions,
        candidates,
        missing_stages,
        weights,
        searched: ctx.searched.clone(),
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

/// The text output of the hits, which are at most the limit. With a byte
/// `budget`, it stops before the hit that would pass it, but prints at least
/// one.
fn render(
    hits: &[Hit],
    more: usize,
    unknown: &[Unknown],
    expansions: &[Expansion],
    explain: bool,
    budget: Option<usize>,
    searched: &[Scope],
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
        let names = |want: bool| -> Vec<&str> {
            Scope::ALL
                .into_iter()
                .filter(|s| searched.contains(s) == want)
                .map(Scope::as_str)
                .collect()
        };
        let _ = writeln!(out, "Searched: {}", names(true).join(", "));
        let not = names(false);
        if !not.is_empty() {
            let _ = writeln!(
                out,
                "Not searched: {} (pass --in <scope> or --in all)",
                not.join(", ")
            );
        }
        return out;
    }
    let mut shown = 0;
    let mut record = None;
    for hit in hits {
        let mut text = String::new();
        let r = &hit.record;
        if record == Some(&hit.path) {
            let _ = writeln!(text, "{}. (same record)", hit.rank);
        } else if let Some(line) = query::ref_line(hit) {
            let _ = writeln!(text, "{}. {line}", hit.rank);
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
            if hit.linked_by > 0 {
                parts.push(format!("linked by {}", hit.linked_by));
            }
            parts.extend(r.title.clone());
            if let Some(key) = &hit.key {
                parts.push(format!("ref:{key}"));
            }
            let _ = writeln!(text, "{}. {}", hit.rank, parts.join(" · "));
        }
        let _ = write!(text, "   {}", hit.anchor);
        if !hit.heading.is_empty() {
            let _ = write!(text, " · {}", hit.heading);
        }
        let _ = writeln!(text, "\n   {}", hit.snippet);
        if !hit.also_in.is_empty() {
            let places: Vec<String> = hit
                .also_in
                .iter()
                .map(|a| match a.id.as_ref().or(a.title.as_ref()) {
                    Some(name) => format!("{name} {}", a.anchor),
                    None => a.anchor.clone(),
                })
                .collect();
            let _ = writeln!(text, "   also in: {}", places.join("; "));
        }
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
        if shown > 0 && budget.is_some_and(|b| out.len() + text.len() > b) {
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
