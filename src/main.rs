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
mod check;
mod config;
mod evaluate;
mod finding;
mod lesson_loop;
mod lessons;
mod retro;
mod signals;
mod stats;

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
        /// Fail only on error findings that are not in the baseline (`lab baseline set`).
        #[arg(long)]
        new_only: bool,
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
    /// Find recurring finding rules that no lesson covers yet.
    Retro {
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
}

#[derive(Subcommand)]
enum ArchiveCommand {
    /// Regenerate every configured index from its records' front matter.
    Index,
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
    /// Find records whose id, title, summary, aliases or path contain every term.
    Find {
        #[arg(required = true)]
        terms: Vec<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Print the next free id with this prefix, such as `H159` for `H`.
    Next {
        prefix: String,
        /// Zero-pad the number to this width; also starts a series no record has yet.
        #[arg(long)]
        width: Option<usize>,
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
            new_only,
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
                    Some(brief::Archive {
                        start,
                        indexes: a.index.iter().map(|i| i.output.clone()).collect(),
                    })
                }
            };
            // With [lab.loop], the loop's last results replace the reminder to evaluate.
            let measurements = match &project.lab.lesson_loop {
                None => None,
                Some(_) => Some(brief::measurements(
                    lesson_loop::read_record(&project),
                    jiff::Timestamp::now(),
                    &project.tz,
                    &lessons,
                    &tags,
                )),
            };
            print!(
                "{}{}",
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
                archive.as_ref().map(brief::archive).unwrap_or_default()
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Archive(cmd) => match cmd {
            ArchiveCommand::Index => archive::index(&project),
            ArchiveCommand::Seal { accept, note } => {
                archive::seal(&project, accept.as_deref().zip(note.as_deref()))
            }
            ArchiveCommand::Update => archive::update(&project),
            ArchiveCommand::Check { format } => archive::check(&project, format == Format::Jsonl),
            ArchiveCommand::Find { terms, limit } => archive::find(&project, &terms, limit),
            ArchiveCommand::Next { prefix, width } => archive::next(&project, &prefix, width),
        },
        Command::Retro {
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
            if background {
                return lesson_loop::background(&project);
            }
            lesson_loop::run(&project, if_older, format == Format::Jsonl, || {
                analyse(&project)
            })
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
# indexes and seals; check the archive with the other checks:
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
    println!("stored {}", run.path.display());
    Ok(if failing {
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

/// Agent findings from `explicit` (must exist) or `[lab] agent_findings` (may
/// be missing, for example before the first `agentprof scan`, with a note).
fn agent_findings(project: &Project, explicit: Option<&Path>) -> Result<AgentFiles, String> {
    let (path, required) = match (explicit, &project.lab.agent_findings) {
        (Some(p), _) => (p.to_path_buf(), true),
        (None, Some(p)) => (project.path(p), false),
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
        (Some(p), _) => project.path(p),
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
    let cands = retro::candidates(&rec, &lessons, project.lab.recurrence);
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
    Ok(ExitCode::SUCCESS)
}

/// Everything a lesson is measured against, loaded once.
struct Evidence<'p> {
    project: &'p Project,
    runs: Vec<check::Run>,
    agent: AgentFiles,
    sessions: Result<Vec<retro::Session>, String>,
}

impl<'p> Evidence<'p> {
    fn load(project: &'p Project) -> Result<Self, String> {
        let agent = agent_findings(project, None)?;
        Ok(Self {
            project,
            runs: check::history(project)?,
            sessions: agent_sessions(project, &agent),
            agent,
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
        };
        evaluate::evaluate(lesson, &inputs, |t| self.project.date_of(t).ok())
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
