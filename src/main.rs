//! `lab`: the research factory's quality gate, lessons ledger and retrospectives.
//!
//! `lab check` runs the project's checkers and keeps their findings;
//! `lab retro` turns recurring findings into lesson candidates; lessons are
//! adopted by a person or agent with a note; `lab brief` hands the adopted
//! rules to every new session; `lab evaluate` measures whether they helped;
//! `lab loop` runs the measuring and evaluating unattended and reports what
//! needs a decision.

mod archive;
mod brief;
mod change;
mod check;
mod config;
mod evaluate;
mod finding;
mod lesson_loop;
mod lessons;
mod replay;
mod retro;
mod rules;
mod search;
mod signals;
mod stats;
mod worktree;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::config::Project;
use crate::finding::Severity;
use crate::lessons::{Kind, Meta, Metric, Status};

#[derive(Parser)]
#[command(
    name = "lab",
    version,
    about = "Quality gate, lessons ledger and retrospectives for research projects"
)]
struct Cli {
    /// Use this research.toml instead of searching upward from the current directory.
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create research.toml and the lessons directory in the current directory (never overwrites).
    Init {
        /// Project name (default: the directory name).
        #[arg(long)]
        name: Option<String>,
    },
    /// Run the configured checkers, store their findings, and compare with the baseline.
    Check {
        /// Run only these checks (repeatable).
        #[arg(long = "only", value_name = "NAME")]
        only: Vec<String>,
        /// Fail only on error findings that are not in the baseline (`lab baseline set`),
        /// and on errors and warnings that the change adds to the files it touches,
        /// against today's checkers run on the merge base with the default branch.
        #[arg(long, conflicts_with = "at")]
        new_only: bool,
        /// Replay a past commit, or each commit of a range `A..B`, in a temporary
        /// git worktree with today's checkers, for `lab evaluate` (repeatable).
        #[arg(long = "at", value_name = "COMMIT")]
        at: Vec<String>,
        /// Print every finding, not only the summary.
        #[arg(long)]
        verbose: bool,
    },
    /// Show or set the accepted findings that `lab check --new-only` does not fail on.
    #[command(subcommand)]
    Baseline(BaselineCommand),
    /// Print the session-start brief: adopted rules, last check, maintenance due.
    Brief {
        /// Only lessons with one of these tags (repeatable).
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// Find recurring finding rules that no lesson covers yet, and missed
    /// search queries that an alias would answer.
    #[command(args_conflicts_with_subcommands = true)]
    Retro {
        #[command(subcommand)]
        action: Option<RetroCommand>,
        /// Agent-session findings (rf.finding/1 JSONL from `agentprof scan`); default from [lab].
        #[arg(long, value_name = "FILE")]
        agent_findings: Option<PathBuf>,
        /// Only check runs on or after this date (YYYY-MM-DD).
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// Write each candidate as an `observed` lesson draft.
        #[arg(long)]
        write: bool,
    },
    /// Test whether each active lesson's metric changed after adoption.
    Evaluate {
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Run the [lab.loop] measure commands, evaluate, and send new or changed signals to notify.
    Loop {
        /// Do nothing when the last loop finished less than this many minutes ago (at most a year).
        #[arg(long, value_name = "MINUTES", value_parser = clap::value_parser!(u64).range(..=525_600))]
        if_older: Option<u64>,
        /// Start the loop detached, with its output in <state_dir>/lab/loop/loop.log, and return at once.
        #[arg(long)]
        background: bool,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Manage lessons.
    #[command(subcommand)]
    Lesson(LessonCommand),
    /// Index, seal and check the research archive configured in [lab.archive].
    #[command(subcommand)]
    Archive(ArchiveCommand),
    /// Search the project's notes, reference registry, paper texts and more: ranked units with an anchor and a snippet.
    ///
    /// Words rank sections that hold all of them first, then sections that
    /// hold some. Put a phrase in double quotes inside one argument
    /// (`'"fiscal calendar" lag'`). Several arguments of which one holds a
    /// space are several phrasings of one question. Exit 0 with hits, with a
    /// partial answer and with no hit; exit 2 on an error.
    Search {
        /// The words, or several phrasings, to search for.
        query: Vec<String>,
        /// The scopes to search, repeatable or comma-separated: notes, refs,
        /// texts, files, sessions, board, or all (default: [lab.search]
        /// default_scopes, which is notes, refs and texts).
        #[arg(long = "in", value_name = "SCOPE", value_delimiter = ',')]
        scopes: Vec<String>,
        /// At most this many hits. Without it: 8 hits, packed to about 2 KB.
        #[arg(long)]
        limit: Option<usize>,
        /// Only records with one of these statuses (repeatable).
        #[arg(long, value_name = "STATUS")]
        status: Vec<String>,
        /// One hit for each record.
        #[arg(long)]
        records: bool,
        /// Print one JSON object (schema rf.search/1) with `state`: hits, partial, zero or error.
        #[arg(long)]
        json: bool,
        /// Show the score of each stage for each hit.
        #[arg(long)]
        explain: bool,
        /// Build the index anew before the query; alone, only build it.
        #[arg(long)]
        rebuild: bool,
        #[command(flatten)]
        graph: search::GraphArgs,
        /// Answer each topic of this file (JSON lines with `id` and `query`, or
        /// `id<TAB>query`) and write a TREC run that `ireval` scores.
        #[arg(long, value_name = "TOPICS")]
        run: Option<PathBuf>,
        /// Write the run to this file instead of stdout.
        #[arg(long, value_name = "FILE", requires = "run")]
        out: Option<PathBuf>,
        /// With --run: also search each topic's `phrasings`.
        #[arg(long, requires = "run")]
        phrasings: bool,
    },
    /// List the neighbours of a record, file, anchor, reference key (`ref:232`) or board task in the search graph.
    ///
    /// The relations: superseded by, supersedes, amended by, amends, task,
    /// part of, parts, links to, linked from and cites; for a reference key,
    /// cited by and the papers it cites; for a task, its records.
    Related {
        /// A record id (`DR041`), a path or anchor (`docs/a.md#L3-L9`), `ref:<key>` or `task:<id>`.
        name: String,
        /// At most this many neighbours of each relation.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Print one JSON object (schema rf.related/1).
        #[arg(long)]
        json: bool,
    },
    /// Print one unit of the search index within a line budget, with its neighbours' anchors.
    ///
    /// When the file changed since the anchor was made, the anchor finds its
    /// text again by content hash and the output says so.
    Open {
        /// A hit's anchor (`docs/a.md#L3-L9`), `path:line`, a path, a record id or a hit's content hash.
        anchor: String,
        /// Print at most this many lines of text.
        #[arg(long, default_value_t = search::OPEN_LINES)]
        lines: usize,
        /// Print one JSON object (schema rf.open/1).
        #[arg(long)]
        json: bool,
    },
    /// Serve the archive search to an agent as an MCP server on stdio.
    ///
    /// Tools: archive_search, archive_open and archive_related, which run the
    /// same code as `lab search`, `lab open` and `lab related`.
    Mcp,
}

#[derive(Subcommand)]
enum RetroCommand {
    /// Add a missed query as an alias of its target record (in the thesaurus
    /// when the record is finished).
    Accept {
        /// The proposal id `lab retro` prints, or its first digits.
        id: String,
        /// The record, as a path or a record id, when the proposal has none
        /// or names the wrong one.
        #[arg(long, value_name = "PATH|ID")]
        record: Option<String>,
    },
    /// Never propose these missed queries again.
    Reject {
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Never propose this recurring rule as a lesson again.
    Dismiss {
        /// `tool:rule`, as `lab retro` prints it.
        candidate: String,
        #[arg(long)]
        note: Option<String>,
    },
    /// Stop applying the thesaurus entry on this line; the line stays.
    Retire { line: usize },
    /// Add an entry `alias = id` to the thesaurus for each record alias.
    Seed,
}

#[derive(Subcommand)]
enum ArchiveCommand {
    /// Regenerate every configured index from its records' front matter.
    Index {
        /// Print this index instead of writing the index files; links are
        /// relative to the project root.
        #[arg(long, value_name = "NAME")]
        print: Option<String>,
    },
    /// The completion gate of archive model 2: refresh the derived files and
    /// the search index, then run the checks; stop at the first failed step
    /// and print the fix for each new finding.
    Done,
    /// Print the records in date order with kind and summary; with a record,
    /// the commits that changed it.
    Log {
        /// Only records dated on or after this day (YYYY-MM-DD).
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// A record id or path: print its commits instead of the timeline.
        record: Option<String>,
    },
    /// Seal new append-only files and record appends; report edited or missing ones.
    Seal {
        /// Re-seal this edited file at its current content (needs --note).
        #[arg(long, value_name = "PATH", requires = "note")]
        accept: Option<String>,
        /// Why the edit is accepted; kept in the manifest's history.
        #[arg(long, requires = "accept")]
        note: Option<String>,
    },
    /// `index`, then `seal`: run it before `lab check`.
    Update,
    /// Check records, indexes, links, size budgets and seals without changing anything.
    Check {
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// List records on the same topic as a record file or title words; prints
    /// `skipped: <reason>` and exits 0 when the search fails.
    Similar {
        #[arg(required = true, value_name = "FILE|WORDS")]
        subject: Vec<String>,
    },
    /// The same as `lab search --in notes --records`.
    Find {
        #[arg(required = true)]
        terms: Vec<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Print the next free id with this prefix, such as `H159` for `H`.
    Next {
        prefix: String,
        /// The new record's title: list records on the same topic on stderr.
        #[arg(long)]
        title: Option<String>,
        /// Zero-pad the number to this width; also starts a series no record has yet.
        #[arg(long)]
        width: Option<usize>,
        /// Record the number in the main checkout's ignored state, so no other
        /// worktree gets it (the default with `[lab.archive] model = 2`).
        #[arg(long, conflicts_with = "peek")]
        reserve: bool,
        /// Only print the number; do not reserve it.
        #[arg(long)]
        peek: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Format {
    Text,
    Jsonl,
}

#[derive(Subcommand)]
enum BaselineCommand {
    /// Accept each check's findings from its latest completed run.
    Set {
        /// Only these checks (repeatable); default: every configured check.
        #[arg(long = "only", value_name = "NAME")]
        only: Vec<String>,
        /// Why the current findings are accepted, e.g. "existing backlog before the gate".
        #[arg(long)]
        note: String,
    },
    /// Print the baseline of each check and its history.
    Show,
}

#[derive(Subcommand)]
enum LessonCommand {
    /// Record a new lesson (status `observed` unless --status is given).
    Add {
        #[arg(long)]
        id: String,
        #[arg(long)]
        title: String,
        #[arg(long, value_enum)]
        kind: Kind,
        /// The practice, as one imperative sentence.
        #[arg(long)]
        rule: String,
        #[arg(long, default_value = "workflow")]
        scope: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// What measures the lesson: a finding rule as `tool:rule`, or a numeric
        /// agent-session field as `agentprof:session.<field.path>`.
        #[arg(long)]
        metric: Option<Metric>,
        /// Evidence pointer: path:line, session id, finding fingerprint or URL (repeatable).
        #[arg(long = "evidence")]
        evidence: Vec<String>,
        /// What happened and why it matters.
        #[arg(long, default_value = "")]
        body: String,
        #[arg(long, value_enum, default_value_t = Status::Observed)]
        status: Status,
    },
    /// List lessons.
    List {
        #[arg(long, value_enum)]
        status: Option<Status>,
        #[arg(long)]
        tag: Option<String>,
    },
    /// Change a lesson's status, with a note saying why.
    Set {
        id: String,
        #[arg(value_enum)]
        status: Status,
        #[arg(long)]
        note: String,
        /// Allow `verified` without an `improved` verdict (the note must cite the evidence).
        #[arg(long)]
        force: bool,
    },
    /// Mark a check-run lesson's introduced findings as reviewed, for those that
    /// cannot be fixed: `lab evaluate` then counts only findings introduced
    /// after the latest complete run.
    Review {
        id: String,
        /// How many introduced findings were reviewed, as `lab evaluate` listed
        /// them; refused when the current count differs.
        #[arg(long)]
        count: usize,
        /// What the review found, e.g. "5 in append-only log lines, 2 misread".
        #[arg(long)]
        note: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("lab: {e}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    if let Command::Init { name } = &cli.command {
        return init(&cwd, name.as_deref());
    }
    let project = Project::discover(&cwd, cli.config.as_deref())?;
    match cli.command {
        Command::Init { .. } => unreachable!("handled above"),
        Command::Check {
            only,
            new_only: _,
            at,
            verbose,
        } if !at.is_empty() => replay(&project, &only, &at, verbose),
        Command::Check {
            only,
            new_only,
            at: _,
            verbose,
        } => check(&project, &only, new_only, verbose),
        Command::Baseline(cmd) => baseline(&project, cmd),
        Command::Brief { tags } => {
            let lessons = lessons::load(&project.lessons_dir())?;
            let runs = check::history(&project)?;
            let agent = agent_findings(&project, None)?;
            let (groups, ungrouped) = session_groups(&agent, || {
                agent_sessions(&project, &agent).map(|s| retro::SessionGroups::new(&s))
            });
            let rec = retro::recurrences(&runs, &agent.findings, groups.as_ref());
            let n = retro::candidates(&rec, &lessons, project.lab.recurrence).len();
            for note in [&agent.note, &ungrouped].into_iter().flatten() {
                eprintln!("lab: {note}");
            }
            let checks: Vec<String> = project.lab.checks.iter().map(|c| c.name.clone()).collect();
            let archive = match project.lab.archive.as_ref() {
                None => None,
                Some(a) => {
                    let start = match &a.start {
                        None => None,
                        Some(s) => {
                            // A missing start page is reported in the brief; any other
                            // problem with it is an error, not an invitation to create it.
                            let bytes = match std::fs::metadata(project.path(s)) {
                                Ok(m) if m.is_file() => Some(m.len()),
                                Ok(_) => return Err(format!("{s}: the start page is not a file")),
                                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                                Err(e) => return Err(format!("{s}: {e}")),
                            };
                            Some((s.clone(), bytes))
                        }
                    };
                    // An index without a file is printed, not read.
                    let files = |i: &&config::IndexSpec| a.write_indexes && !i.output.is_empty();
                    let section = brief::Archive {
                        start,
                        indexes: a
                            .index
                            .iter()
                            .filter(files)
                            .map(|i| i.output.clone())
                            .collect(),
                        mcp: brief::lab_mcp_configured(&project.root),
                        registry: project
                            .root
                            .join(&project.registry)
                            .is_file()
                            .then(|| project.registry.clone()),
                    };
                    Some(if a.model >= 2 {
                        let printed: Vec<String> = a
                            .index
                            .iter()
                            .filter(|i| !files(i))
                            .map(|i| i.name.clone())
                            .collect();
                        brief::archive_model_2(&section, &printed)
                    } else {
                        brief::archive(&section)
                    })
                }
            };
            // With [lab.loop], the loop's last results replace the reminder to evaluate.
            // In a linked git worktree they are the main checkout's, where the loop runs.
            let measurements = project.lab.lesson_loop.as_ref().map(|_| {
                let now = jiff::Timestamp::now();
                let show = |record, main| {
                    brief::measurements(record, main, now, &project.tz, &lessons, &tags)
                };
                match lesson_loop::checkout(&project, cli.config.as_deref()) {
                    Ok(lesson_loop::Checkout::This) => {
                        show(lesson_loop::read_record(&project), None)
                    }
                    Ok(lesson_loop::Checkout::Main(main)) => {
                        show(lesson_loop::read_record(&main), Some(&main.root))
                    }
                    Ok(lesson_loop::Checkout::Skip(reason)) => brief::no_measurements(&format!(
                        "{reason}; the loop runs only in a main checkout, so there are no \
                         measurements to show."
                    )),
                    Err(e) => brief::no_measurements(&format!(
                        "Cannot find the checkout the loop runs for: {e}."
                    )),
                }
            });
            let version = project
                .lab
                .min_version
                .as_deref()
                .and_then(|min| brief::version_note(env!("CARGO_PKG_VERSION"), min));
            print!(
                "{}{}{}",
                version.unwrap_or_default(),
                brief::render(
                    &project.name,
                    &project.today().to_string(),
                    &lessons,
                    &runs,
                    &checks,
                    &tags,
                    n,
                    measurements.as_deref()
                ),
                archive.unwrap_or_default()
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Archive(cmd) => match cmd {
            ArchiveCommand::Index { print: None } => archive::index(&project),
            ArchiveCommand::Index { print: Some(name) } => archive::print_index(&project, &name),
            ArchiveCommand::Done => archive::done(&project),
            ArchiveCommand::Log { since, record } => {
                archive::log(&project, since.as_deref(), record.as_deref())
            }
            ArchiveCommand::Seal { accept, note } => {
                archive::seal(&project, accept.as_deref().zip(note.as_deref()))
            }
            ArchiveCommand::Update => archive::update(&project),
            ArchiveCommand::Check { format } => archive::check(&project, format == Format::Jsonl),
            ArchiveCommand::Find { terms, limit } => search::run(
                &project,
                &search::Args {
                    query: terms,
                    limit,
                    statuses: Vec::new(),
                    records: true,
                    json: false,
                    scopes: vec!["notes".into()],
                    explain: false,
                    rebuild: false,
                    graph: search::GraphArgs::default(),
                    run: None,
                    out: None,
                    phrasings: false,
                },
            ),
            ArchiveCommand::Similar { subject } => search::similar::run(&project, &subject),
            ArchiveCommand::Next {
                prefix,
                title,
                width,
                reserve,
                peek,
            } => {
                if let Some(title) = &title {
                    // stdout stays the id alone.
                    match search::similar::similar(&project, title, &[]) {
                        search::similar::Outcome::Found(found) if !found.is_empty() => eprint!(
                            "similar: {} record(s) on the same topic; extend one instead?\n{}",
                            found.len(),
                            search::similar::render(&found)
                        ),
                        search::similar::Outcome::Found(_) => eprintln!("similar: none"),
                        search::similar::Outcome::Skipped(why) => eprintln!("skipped: {why}"),
                    }
                }
                archive::next(
                    &project,
                    cli.config.as_deref(),
                    &prefix,
                    width,
                    reserve,
                    peek,
                )
            }
        },
        Command::Search {
            query,
            scopes,
            limit,
            status,
            records,
            json,
            explain,
            rebuild,
            graph,
            run,
            out,
            phrasings,
        } => search::run(
            &project,
            &search::Args {
                query,
                limit,
                statuses: status,
                records,
                json,
                scopes,
                explain,
                rebuild,
                graph,
                run,
                out,
                phrasings,
            },
        ),
        Command::Related { name, limit, json } => search::related(&project, &name, limit, json),
        Command::Open {
            anchor,
            lines,
            json,
        } => search::open(&project, &anchor, lines, json),
        Command::Mcp => search::mcp(&project),
        Command::Retro {
            action: Some(action),
            ..
        } => match action {
            RetroCommand::Accept { id, record } => {
                search::learn::accept(&project, &id, record.as_deref())
            }
            RetroCommand::Reject { ids, note } => {
                search::learn::reject(&project, &ids, note.as_deref())
            }
            RetroCommand::Dismiss { candidate, note } => {
                search::learn::dismiss(&project, &candidate, note.as_deref())
            }
            RetroCommand::Retire { line } => search::learn::retire(&project, line),
            RetroCommand::Seed => search::learn::seed(&project),
        },
        Command::Retro {
            action: None,
            agent_findings: path,
            since,
            write,
        } => retro_cmd(&project, path.as_deref(), since.as_deref(), write),
        Command::Evaluate { format } => evaluate(&project, format == Format::Jsonl),
        Command::Loop {
            if_older,
            background,
            format,
        } => {
            let (project, worktree) = match lesson_loop::checkout(&project, cli.config.as_deref())?
            {
                lesson_loop::Checkout::This => (project, None),
                lesson_loop::Checkout::Main(main) => (*main, Some(project.root)),
                lesson_loop::Checkout::Skip(reason) => {
                    let message = format!(
                        "{reason}; `lab loop` runs only in a main checkout, so it did nothing."
                    );
                    if format == Format::Jsonl {
                        eprintln!("{message}");
                    } else {
                        println!("{message}");
                    }
                    return Ok(ExitCode::SUCCESS);
                }
            };
            if background {
                return lesson_loop::background(&project);
            }
            lesson_loop::run(
                &project,
                worktree.as_deref(),
                if_older,
                format == Format::Jsonl,
                || analyse(&project),
            )
        }
        Command::Lesson(cmd) => lesson(&project, cmd),
    }
}

const CONFIG_TEMPLATE: &str = r#"# Research factory configuration. Each tool reads its own table; see
# https://github.com/syz51/research-factory/blob/main/docs/interfaces.md

[project]
name = "NAME"
timezone = "Asia/Tokyo"
state_dir = ".research"

[lab]
lessons_dir = "lessons"
recurrence = 3
# Agent-session findings for `lab retro`: run `agentprof scan --out .research/agentprof` first.
# Do not also list agentprof under [[lab.checks]]. `lab evaluate` measures agentprof lessons per
# session from the sessions.jsonl next to this file (override with agent_sessions).
# agent_findings = ".research/agentprof/findings.jsonl"

# How `lab evaluate` decides whether a lesson helped (these are the defaults).
# [lab.evaluate]
# min_sessions = 10     # session groups, or check-run intervals, needed on each side of the adoption date
# alpha = 0.05          # two-sided; each direction is tested at alpha/2
# baseline_days = 90    # compare with this many days before adoption; 0 = all earlier sessions

# `lab loop` runs these commands, evaluates every lesson, and sends each new or changed signal
# (rf.signal/1 JSON on stdin) to `notify`. Hooks start it with `lab loop --background --if-older 15`.
# [lab.loop]
# measure = [["agentprof", "scan", "--out", ".research/agentprof", "--timezone", "Asia/Tokyo"]]
# notify = ["sh", "scripts/notify.sh"]
# renotify_hours = 24

# Each check prints rf.finding/1 JSON Lines and exits 0 (no errors), 1 (errors) or 2 (failed).
# Names must be unique. After the first `lab check`, accept the existing backlog with
# `lab baseline set --note "<why>"` so that `lab check --new-only` fails only on new errors.
# Each run records each checker's version: the first line `version` prints (default:
# `<program> --version` for a bare program name that is not an interpreter; [] for none).
# `lab check --at <commit>` replays past commits; `uncommitted_inputs` lists files no commit
# holds, such as git-ignored source texts, which a replay then reads from the work tree.
# [[lab.checks]]
# name = "references"
# run = ["papers", "registry", "check", "--format", "jsonl"]
#
# [[lab.checks]]
# name = "claims"
# run = ["claimcheck", "check", "docs", "--format", "jsonl"]
#
# [[lab.checks]]
# name = "sources"
# run = ["prov", "manifest", "verify", "sources/manifest.jsonl", "--format", "jsonl"]

# The research archive: a short start page, records with optional YAML front matter, indexes
# generated from it, and append-only files sealed against edits. `lab archive update` writes the
# indexes and seals. Archive model 2 (`model = 2` with `seal = "git"`, `task_from`, `board_export`
# and `write_indexes = false`; docs/interfaces.md "Archive model 2") makes git the seal, has each
# record name its board task and kind, and ends a record with `lab archive done`.
# Check the archive with the other checks:
# [[lab.checks]]
# name = "archive"
# run = ["lab", "archive", "check", "--format", "jsonl"]
#
# [lab.archive]
# start = "docs/research/START.md"
# seal_manifest = "docs/research/.seal.json"     # default: <state_dir>/lab/archive-seal.json
# sealed = ["docs/research/history/[0-9]*.md", "docs/research/log/*.md"]
# links = ["docs/research/**/*.md"]
# links_exclude = ["docs/research/followups/*/inputs/**"]
# current_pages = ["docs/research/START.md"]
#
# [[lab.archive.index]]
# name = "history"
# output = "docs/research/history/INDEX.md"
# title = "History reports"
# intro = "One line per session report, newest first."
# include = ["docs/research/history/[0-9]*.md"]
# id_from = '^(\d+[a-z]?)-'
# id_prefix = "H"
# trim_leading_zeros = true
# sort = "id-desc"
# require = ["id", "title", "date"]
#
# [[lab.archive.index]]
# name = "decisions"
# output = "docs/research/decisions/INDEX.md"
# title = "Decisions"
# include = ["docs/research/decisions/D*.md"]
# id_from = '^(D\d+)-'
# group_by = "status"
# require = ["id", "title", "status"]
#
# [[lab.archive.budget]]
# path = "docs/research/START.md"
# max_bytes = 8192
# severity = "error"
"#;

const LESSONS_README: &str = "# Lessons\n\nOne file per lesson, managed with `lab lesson`. \
A lesson records a failure to avoid (`bad`) or a practice to keep (`good`), the rule that \
follows from it, and its evidence. Status: observed → proposed → adopted → verified, or retired. \
`lab brief` shows every adopted and verified rule at the start of a session.\n";

fn init(dir: &Path, name: Option<&str>) -> Result<ExitCode, String> {
    let config = dir.join(config::CONFIG_FILE);
    let name = name.map_or_else(
        || {
            dir.file_name()
                .map_or_else(|| "project".into(), |n| n.to_string_lossy().into_owned())
        },
        str::to_owned,
    );
    if config.exists() {
        println!("{} exists; left unchanged", config.display());
    } else {
        let name = name.replace('\\', "\\\\").replace('"', "\\\"");
        std::fs::write(&config, CONFIG_TEMPLATE.replace("NAME", &name))
            .map_err(|e| format!("{}: {e}", config.display()))?;
        println!("created {}", config.display());
    }
    let lessons_dir = dir.join("lessons");
    std::fs::create_dir_all(&lessons_dir).map_err(|e| format!("{}: {e}", lessons_dir.display()))?;
    let readme = lessons_dir.join("README.md");
    if !readme.exists() {
        std::fs::write(&readme, LESSONS_README)
            .map_err(|e| format!("{}: {e}", readme.display()))?;
        println!("created {}", readme.display());
    }
    println!(
        "Add .research/ to .gitignore if the project uses git, then configure [[lab.checks]]."
    );
    Ok(ExitCode::SUCCESS)
}

fn check(
    project: &Project,
    only: &[String],
    new_only: bool,
    verbose: bool,
) -> Result<ExitCode, String> {
    let report = check::run(project, only)?;
    let run = &report.run;
    println!(
        "{:<14} {:>6} {:>7} {:>6} {:>5} {:>5} {:>9} {:>8}",
        "check", "status", "errors", "warn", "info", "new", "resolved", "ms"
    );
    let mut failing = false;
    let mut failing_items = Vec::new();
    let mut no_baseline = Vec::new();
    for c in &run.header.checks {
        let count = |sev| {
            run.items
                .iter()
                .filter(|i| i.check == c.name && i.finding.severity == sev)
                .count()
        };
        let delta = report.delta.get(&c.name).and_then(Option::as_ref);
        let (new, resolved) = delta.map_or_else(
            || ("-".to_string(), "-".to_string()),
            |d| (d.new.len().to_string(), d.resolved.len().to_string()),
        );
        println!(
            "{:<14} {:>6} {:>7} {:>6} {:>5} {:>5} {:>9} {:>8}",
            c.name,
            c.status,
            count(Severity::Error),
            count(Severity::Warn),
            count(Severity::Info),
            new,
            resolved,
            c.duration_ms
        );
        if c.ran() && delta.is_none() {
            no_baseline.push(c.name.as_str());
        }
        // Without a baseline for this check, every error counts as new.
        let errors = run
            .items
            .iter()
            .filter(|i| i.check == c.name && i.finding.severity == Severity::Error)
            .filter(|i| !new_only || delta.is_none_or(|d| d.new.contains(&i.fingerprint)));
        let before = failing_items.len();
        failing_items.extend(errors);
        failing |= failing_items.len() > before || !c.ran();
    }
    for m in &run.measures {
        println!(
            "{} measured {} {}(s) for {}{}",
            m.check,
            m.measure.count,
            m.measure.unit,
            m.measure.tool,
            if m.measure.keys.is_some() {
                ", with keys"
            } else {
                ""
            }
        );
    }
    if new_only && !no_baseline.is_empty() {
        println!(
            "No baseline for {}: every error counts as new. Accept the current findings with \
             `lab baseline set --note \"<why>\"`.",
            no_baseline.join(", ")
        );
    }
    if !verbose && !failing_items.is_empty() {
        println!(
            "{} error(s){}:",
            failing_items.len(),
            if new_only { " not in the baseline" } else { "" }
        );
        for item in &failing_items {
            let f = &item.finding;
            println!(
                "{} {}:{} {}: {}",
                item.fingerprint, f.tool, f.rule, f.subject, f.message
            );
        }
    }
    if verbose {
        for item in &run.items {
            let f = &item.finding;
            println!(
                "{} {} {}:{} {}: {}",
                item.fingerprint,
                f.severity.as_str(),
                f.tool,
                f.rule,
                f.subject,
                f.message
            );
        }
    }
    if new_only {
        failing |= print_change(
            &change::compare(project, only, run, &report.delta)?,
            verbose,
        );
    }
    println!("stored {}", run.path.display());
    Ok(if failing {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// Prints what `lab check --new-only` found against the merge base; true when
/// the change adds an error or warning to a file it touches.
fn print_change(outcome: &change::Outcome, verbose: bool) -> bool {
    let line = |n: &change::New| {
        let f = &n.item.finding;
        format!(
            "{} {} {}:{}: {}",
            n.place,
            f.severity.as_str(),
            f.tool,
            f.rule,
            f.message
        )
    };
    let c = match outcome {
        change::Outcome::Skipped(why) => {
            println!("No change check: {why}. Only the baseline gate applies.");
            return false;
        }
        change::Outcome::Unchanged { branch, base } => {
            println!(
                "No file differs from {branch} at the merge base {}.",
                change::short(base)
            );
            return false;
        }
        change::Outcome::Compared(c) => c,
    };
    println!(
        "Against {} at the merge base {}, {} file(s) changed.",
        c.branch,
        change::short(&c.base),
        c.changed
    );
    for name in &c.no_base {
        println!(
            "Check `{name}` did not run to completion at the merge base: each of its findings counts as new."
        );
    }
    let new = &c.new;
    if new.gated.is_empty() {
        println!("The change adds no error or warning to the files it touches.");
    } else {
        println!(
            "{} error(s) or warning(s) the change adds to the files it touches:",
            new.gated.len()
        );
        for n in &new.gated {
            println!("{}", line(n));
        }
        println!(
            "Fix them. Only a person may accept the ones that cannot be fixed, with `lab baseline \
             set --note \"<why>\"` after this run, which accepts every finding of each check's \
             latest run."
        );
    }
    let apart = [
        ("accepted in the baseline", &new.accepted),
        ("info in the files it touches", &new.info),
        ("in text sealed at the merge base", &new.sealed),
        ("naming a missing file that git ignores", &new.missing_input),
        ("in files the change does not touch", &new.elsewhere),
    ];
    let counts: Vec<String> = apart
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(what, v)| format!("{} {what}", v.len()))
        .collect();
    if !counts.is_empty() {
        println!(
            "Also new against {}, not gated: {}{}.",
            c.branch,
            counts.join(", "),
            if verbose {
                ""
            } else {
                " (listed with --verbose)"
            }
        );
    }
    if verbose {
        for (what, v) in apart.iter().filter(|(_, v)| !v.is_empty()) {
            println!("New, {what}:");
            for n in v.iter() {
                println!("{}", line(n));
            }
        }
    }
    !new.gated.is_empty()
}

/// `lab check --at`: one summary line per replayed commit. Exits 1 when a
/// check failed in a replay; findings do not fail it, since a replay measures
/// past text rather than gating it.
fn replay(
    project: &Project,
    only: &[String],
    revs: &[String],
    verbose: bool,
) -> Result<ExitCode, String> {
    let mut failed = false;
    // Each run is printed as it is stored, so an error on a later commit
    // still shows the earlier ones.
    replay::replay(project, only, revs, |run| {
        let h = &run.header;
        let commit = h.commit.as_deref().unwrap_or("?");
        let checks: Vec<String> = h
            .checks
            .iter()
            .map(|c| {
                let count = |sev| {
                    run.items
                        .iter()
                        .filter(|i| i.check == c.name && i.finding.severity == sev)
                        .count()
                };
                let from_work_tree = if c.uncommitted_inputs.is_empty() {
                    String::new()
                } else {
                    format!(
                        ", read {} from the work tree",
                        c.uncommitted_inputs.join(", ")
                    )
                };
                format!(
                    "{} {} ({} errors, {} warnings, {} info{from_work_tree})",
                    c.name,
                    c.status,
                    count(Severity::Error),
                    count(Severity::Warn),
                    count(Severity::Info)
                )
            })
            .collect();
        failed |= h.checks.iter().any(|c| !c.ran());
        println!(
            "replayed {} ({}): {}",
            &commit[..commit.len().min(12)],
            h.at,
            checks.join("; ")
        );
        for m in &run.measures {
            println!(
                "  {} measured {} {}(s) for {}",
                m.check, m.measure.count, m.measure.unit, m.measure.tool
            );
        }
        if verbose {
            for item in &run.items {
                let f = &item.finding;
                println!(
                    "  {} {} {}:{} {}: {}",
                    item.fingerprint,
                    f.severity.as_str(),
                    f.tool,
                    f.rule,
                    f.subject,
                    f.message
                );
            }
        }
        println!("  stored {}", run.path.display());
    })?;
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

struct AgentFiles {
    findings: Vec<finding::Finding>,
    /// The findings file, when one is named.
    path: Option<PathBuf>,
    /// Why the configured findings file was not read.
    note: Option<String>,
    /// Whether the file was named on the command line, which makes its
    /// sibling `sessions.jsonl` its sessions file.
    explicit: bool,
}

/// A configured file of gitignored state, such as `agentprof scan` output. A
/// new linked git worktree has none, so when the worktree lacks it and git
/// ignores it there, it is the main checkout's file, read-only.
fn state_file(project: &Project, rel: &str) -> Result<PathBuf, String> {
    let path = project.path(rel);
    if path.exists() || !config::below_root(Path::new(rel)) {
        return Ok(path);
    }
    let Some(fallback) = worktree::Fallback::find(&project.root)? else {
        return Ok(path);
    };
    Ok(match fallback.file(rel)? {
        Some(main) => {
            eprintln!(
                "lab: this linked git worktree has no {rel}, which git ignores; reading the main checkout's {}",
                main.display()
            );
            main
        }
        None => path,
    })
}

/// Agent findings from `explicit` (must exist) or `[lab] agent_findings` (may
/// be missing, for example before the first `agentprof scan`, with a note).
fn agent_findings(project: &Project, explicit: Option<&Path>) -> Result<AgentFiles, String> {
    let (path, required) = match (explicit, &project.lab.agent_findings) {
        (Some(p), _) => (p.to_path_buf(), true),
        (None, Some(p)) => (state_file(project, p)?, false),
        (None, None) => {
            return Ok(AgentFiles {
                findings: Vec::new(),
                path: None,
                note: None,
                explicit: false,
            });
        }
    };
    if !required && !path.exists() {
        return Ok(AgentFiles {
            findings: Vec::new(),
            note: Some(format!(
                "{} does not exist yet; agent-session findings are left out (run `agentprof scan --out <dir>`)",
                path.display()
            )),
            path: Some(path),
            explicit: false,
        });
    }
    Ok(AgentFiles {
        findings: retro::read_findings(&path)?,
        path: Some(path),
        note: None,
        explicit: required,
    })
}

/// The agent sessions that measure agent-rule lessons, or why they are
/// unavailable: the sibling `sessions.jsonl` of a findings file named on the
/// command line, else `[lab] agent_sessions`, else the configured findings
/// file's sibling.
fn agent_sessions(project: &Project, agent: &AgentFiles) -> Result<Vec<retro::Session>, String> {
    let path = match (&project.lab.agent_sessions, &agent.path) {
        (_, Some(findings)) if agent.explicit => findings.with_file_name("sessions.jsonl"),
        (Some(p), _) => state_file(project, p)?,
        (None, Some(findings)) => findings.with_file_name("sessions.jsonl"),
        (None, None) => return Err("no [lab] agent_findings or agent_sessions configured".into()),
    };
    retro::read_sessions(&path)
}

/// The session groups `lab retro` counts agent findings in, read only when
/// there are agent findings; without them, a note saying why agent findings
/// count once per distinct finding.
fn session_groups(
    agent: &AgentFiles,
    sessions: impl FnOnce() -> Result<retro::SessionGroups, String>,
) -> (Option<retro::SessionGroups>, Option<String>) {
    if agent.findings.is_empty() {
        return (None, None);
    }
    match sessions() {
        Ok(groups) => (Some(groups), None),
        Err(why) => (
            None,
            Some(format!(
                "agent findings count once per distinct finding, not once per session group: sessions file unreadable: {why}"
            )),
        ),
    }
}

fn baseline(project: &Project, cmd: BaselineCommand) -> Result<ExitCode, String> {
    let mut b = check::Baseline::load(project)?;
    match cmd {
        BaselineCommand::Set { only, note } => {
            let specs = check::select(project, &only)?;
            let runs = check::history(project)?;
            let names = b.set(&runs, &specs, &note)?;
            b.save(project)?;
            for name in names {
                let e = &b.checks[&name];
                println!(
                    "{name}: {} finding(s) accepted from {}",
                    e.fingerprints.len(),
                    e.run
                );
            }
        }
        BaselineCommand::Show => {
            if b.checks.is_empty() {
                println!("No baseline set: `lab check --new-only` treats every error as new.");
            }
            for (name, e) in &b.checks {
                println!(
                    "{name}: {} finding(s) from {} ({})",
                    e.fingerprints.len(),
                    e.run,
                    e.at
                );
            }
            for h in &b.history {
                println!("- {}: {}: {}", h.at, h.checks.join(", "), h.note);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn retro_cmd(
    project: &Project,
    path: Option<&Path>,
    since: Option<&str>,
    write: bool,
) -> Result<ExitCode, String> {
    let since: Option<jiff::civil::Date> = since
        .map(|s| {
            s.parse()
                .map_err(|_| format!("--since must be YYYY-MM-DD, got `{s}`"))
        })
        .transpose()?;
    let lessons = lessons::load(&project.lessons_dir())?;
    let runs: Vec<_> = check::history(project)?
        .into_iter()
        .filter(|r| since.is_none_or(|s| project.date_of(&r.header.at).is_ok_and(|d| d >= s)))
        .collect();
    let agent = agent_findings(project, path)?;
    let (groups, ungrouped) = session_groups(&agent, || {
        agent_sessions(project, &agent).map(|s| retro::SessionGroups::new(&s))
    });
    for note in [&agent.note, &ungrouped].into_iter().flatten() {
        println!("{note}");
    }
    let rec = retro::recurrences(&runs, &agent.findings, groups.as_ref());
    let mut cands = retro::candidates(&rec, &lessons, project.lab.recurrence);
    // A dismissed candidate does not return.
    let decisions = search::learn::Decisions::load(project)?;
    cands.retain(|((tool, rule), _)| !decisions.is_dismissed(tool, rule));
    println!(
        "{} check run(s), {} agent finding(s), {} rule(s) seen; {} recurring rule(s) without a lesson (threshold: {} recurrences, each a distinct finding or the agent findings in one session group).",
        runs.len(),
        agent.findings.len(),
        rec.len(),
        cands.len(),
        project.lab.recurrence
    );
    let today = project.today().to_string();
    for ((tool, rule), r) in &cands {
        let sev = r.severity.map_or("?", Severity::as_str);
        let sample: Vec<&str> = r.subjects.iter().take(3).map(String::as_str).collect();
        println!(
            "- {sev} {tool}:{rule}: {}, {} run(s), {} subject(s); e.g. {}",
            r.describe(),
            r.runs,
            r.subjects.len(),
            sample.join(", ")
        );
        if write {
            let id = format!("{tool}-{rule}")
                .to_lowercase()
                .replace(|c: char| !c.is_ascii_alphanumeric(), "-");
            let id = id
                .split('-')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-");
            let meta = Meta {
                id: id.clone(),
                title: format!("Recurring {tool} finding: {rule}"),
                kind: Kind::Bad,
                status: Status::Observed,
                scope: "workflow".into(),
                tags: vec![tool.clone()],
                rule: format!(
                    "Find and remove the cause of recurring `{rule}` findings from {tool}."
                ),
                metric: Some(Metric {
                    tool: tool.clone(),
                    rule: rule.clone(),
                }),
                created: today.clone(),
                updated: today.clone(),
                adopted: None,
                reviewed: None,
                evidence: sample.iter().map(|s| s.to_string()).collect(),
            };
            let body = "Drafted by `lab retro` from recurring findings. Replace the rule with the concrete practice once the cause is known.";
            match lessons::create(&project.lessons_dir(), meta, body) {
                Ok(p) => println!("  wrote {}", p.display()),
                Err(e) => println!("  not written: {e}"),
            }
        }
    }
    // The search trace is optional input: a problem with it is a note.
    match search::learn::report(project, since) {
        Ok(text) => print!("{text}"),
        Err(e) => println!("Missed search queries are left out: {e}"),
    }
    Ok(ExitCode::SUCCESS)
}

/// Everything a lesson is measured against, loaded once.
struct Evidence<'p> {
    project: &'p Project,
    runs: Vec<check::Run>,
    agent: AgentFiles,
    sessions: Result<Vec<retro::Session>, String>,
    /// The dates of the commits a replay can check (`Inputs::commit_dates`),
    /// read from git when a lesson first needs them.
    commit_dates: std::cell::OnceCell<Option<Vec<jiff::civil::Date>>>,
}

impl<'p> Evidence<'p> {
    fn load(project: &'p Project) -> Result<Self, String> {
        let agent = agent_findings(project, None)?;
        Ok(Self {
            project,
            // Replayed runs are interval ends for `lab evaluate`, and nothing else.
            runs: check::history_with_replays(project)?,
            sessions: agent_sessions(project, &agent),
            agent,
            commit_dates: std::cell::OnceCell::new(),
        })
    }

    fn measure(&self, lesson: &lessons::Lesson) -> evaluate::Evaluation {
        let configured: Vec<&str> = self
            .project
            .lab
            .checks
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        let inputs = evaluate::Inputs {
            runs: &self.runs,
            configured: &configured,
            findings: &self.agent.findings,
            sessions: self.sessions.as_deref().map_err(String::as_str),
            settings: &self.project.lab.evaluate,
            commit_dates: &|| self.commit_dates().as_deref(),
        };
        evaluate::evaluate(lesson, &inputs, |t| self.project.date_of(t).ok())
    }

    fn commit_dates(&self) -> &Option<Vec<jiff::civil::Date>> {
        self.commit_dates.get_or_init(|| {
            let times = replay::commit_times(&self.project.root)?;
            let dates: std::collections::BTreeSet<_> = times
                .into_iter()
                .map(|t| t.to_zoned(self.project.tz.clone()).date())
                .collect();
            Some(dates.into_iter().collect())
        })
    }

    /// Every active lesson, measured.
    fn active(&self, lessons: &[lessons::Lesson]) -> Vec<evaluate::Evaluation> {
        lessons
            .iter()
            .filter(|l| l.meta.status.active())
            .map(|l| self.measure(l))
            .collect()
    }
}

/// What `lab loop` evaluates and signals, after its measure commands ran.
fn analyse(project: &Project) -> Result<lesson_loop::Analysis, String> {
    let lessons = lessons::load(&project.lessons_dir())?;
    let evidence = Evidence::load(project)?;
    let evaluations = evidence.active(&lessons);
    let (groups, ungrouped) = session_groups(&evidence.agent, || {
        evidence
            .sessions
            .as_deref()
            .map(retro::SessionGroups::new)
            .map_err(Clone::clone)
    });
    let rec = retro::recurrences(&evidence.runs, &evidence.agent.findings, groups.as_ref());
    let candidates = retro::candidates(&rec, &lessons, project.lab.recurrence)
        .into_iter()
        .map(|((tool, rule), r)| signals::Candidate {
            tool: tool.clone(),
            rule: rule.clone(),
            severity: r.severity.map_or("?", Severity::as_str).into(),
            count: r.describe(),
            sample: r.subjects.iter().take(3).cloned().collect(),
        })
        .collect();
    Ok(lesson_loop::Analysis {
        lessons,
        evaluations,
        uncovered: signals::Uncovered {
            candidates,
            ungrouped,
        },
    })
}

fn evaluate(project: &Project, jsonl: bool) -> Result<ExitCode, String> {
    let lessons = lessons::load(&project.lessons_dir())?;
    let evidence = Evidence::load(project)?;
    let evaluations = evidence.active(&lessons);
    if jsonl {
        for e in &evaluations {
            println!("{}", serde_json::to_string(e).map_err(|e| e.to_string())?);
        }
        return Ok(ExitCode::SUCCESS);
    }
    if evaluations.is_empty() {
        println!("No adopted or verified lesson.");
        return Ok(ExitCode::SUCCESS);
    }
    for e in &evaluations {
        println!("{}", e.line());
        for line in e.introduced_lines() {
            println!("{line}");
        }
    }
    let s = &project.lab.evaluate;
    println!(
        "\nVerdicts need {} observations (session groups, or check-run intervals) on each side and p below {} in one direction \
         (alpha {} two-sided); p↓ tests for fewer findings since adoption, p↑ for more. \
         The comparison is observational: other changes since adoption can explain a difference, \
         and with {} lessons measured some will cross the threshold by chance. \
         For check runs, \"introduced\" counts findings in the latest complete run beyond the count \
         of the same message and data in the first complete run since adoption, or in the run of the \
         last `lab lesson review`; a verdict compares findings introduced per new unit of checked text \
         over intervals between clean runs (no uncommitted changes) on consecutive commits whose checks \
         print `rf.measure/1`, each interval counting as one observation.",
        s.min_sessions,
        s.alpha / 2.0,
        s.alpha,
        evaluations.len()
    );
    Ok(ExitCode::SUCCESS)
}

fn lesson(project: &Project, cmd: LessonCommand) -> Result<ExitCode, String> {
    let dir = project.lessons_dir();
    match cmd {
        LessonCommand::Add {
            id,
            title,
            kind,
            rule,
            scope,
            tags,
            metric,
            evidence,
            body,
            status,
        } => {
            let today = project.today().to_string();
            let adopted = status.active().then(|| today.clone());
            if status == Status::Verified {
                return Err("a new lesson cannot start as verified".into());
            }
            let meta = Meta {
                id,
                title,
                kind,
                status,
                scope,
                tags,
                rule,
                metric,
                created: today.clone(),
                updated: today,
                reviewed: None,
                adopted,
                evidence,
            };
            println!("{}", lessons::create(&dir, meta, &body)?.display());
        }
        LessonCommand::List { status, tag } => {
            for l in lessons::load(&dir)? {
                if status.is_some_and(|s| s != l.meta.status)
                    || tag.as_ref().is_some_and(|t| !l.meta.tags.contains(t))
                {
                    continue;
                }
                let kind = match l.meta.kind {
                    Kind::Good => "good",
                    Kind::Bad => "bad",
                };
                println!(
                    "{:<9} {:<4} {:<32} {}",
                    l.meta.status.as_str(),
                    kind,
                    l.meta.id,
                    l.meta.title
                );
            }
        }
        LessonCommand::Set {
            id,
            status,
            note,
            force,
        } => {
            let mut all = lessons::load(&dir)?;
            let lesson = all
                .iter_mut()
                .find(|l| l.meta.id == id)
                .ok_or_else(|| format!("no lesson `{id}` in {}", dir.display()))?;
            // `verified` needs the lesson's own metric to have improved, or --force.
            if status == Status::Verified && !force {
                if lesson.meta.metric.is_none() {
                    return Err(format!(
                        "`{id}` has no metric to measure; pass --force with a note citing the evidence"
                    ));
                }
                let e = Evidence::load(project)?.measure(lesson);
                if e.verdict != evaluate::Verdict::Improved {
                    return Err(format!(
                        "`{id}` cannot be verified: `lab evaluate` says {}{}; \
                         pass --force with a note citing other evidence",
                        e.verdict.as_str(),
                        e.reason.map(|r| format!(" ({r})")).unwrap_or_default()
                    ));
                }
            }
            lesson.transition(status, &note, &project.today().to_string())?;
            lessons::save(lesson)?;
            println!("{} is now {}", id, status.as_str());
        }
        LessonCommand::Review { id, count, note } => {
            let mut all = lessons::load(&dir)?;
            let lesson = all
                .iter_mut()
                .find(|l| l.meta.id == id)
                .ok_or_else(|| format!("no lesson `{id}` in {}", dir.display()))?;
            lesson.reviewable(&note)?;
            let e = Evidence::load(project)?.measure(lesson);
            let Some(found) = e.introduced else {
                return Err(format!(
                    "`{id}` has no introduced findings to review: `lab evaluate` says {}{}",
                    e.verdict.as_str(),
                    e.reason.map(|r| format!(" ({r})")).unwrap_or_default()
                ));
            };
            let since = found.since_text(e.adopted.as_deref());
            if found.count == 0 {
                return Err(format!("`{id}` has no findings introduced since {since}"));
            }
            if found.count != count {
                return Err(format!(
                    "`{id}` has {} findings introduced since {since} in run {}, not {count}; \
                     list them with `lab evaluate` and review again",
                    found.count, found.latest_run
                ));
            }
            let summary = format!(
                "{} finding(s) introduced since {since}, through run {}",
                found.count, found.latest_run
            );
            lesson.review(
                &found.latest_run,
                &summary,
                &note,
                &project.today().to_string(),
            )?;
            lessons::save(lesson)?;
            println!("{id}: reviewed {summary}");
        }
    }
    Ok(ExitCode::SUCCESS)
}
