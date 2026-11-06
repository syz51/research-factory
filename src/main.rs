//! `lab`: the research factory's quality gate, lessons ledger and retrospectives.
//!
//! `lab check` runs the project's checkers and keeps their findings;
//! `lab retro` turns recurring findings into lesson candidates; lessons are
//! adopted by a person or agent with a note; `lab brief` hands the adopted
//! rules to every new session; `lab evaluate` measures whether they helped.

mod brief;
mod check;
mod config;
mod finding;
mod lessons;
mod retro;

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
    /// Compare each measured lesson's finding rate before and after adoption.
    Evaluate,
    /// Manage lessons.
    #[command(subcommand)]
    Lesson(LessonCommand),
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
        /// Finding rule that measures the lesson, as `tool:rule`.
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
        /// Allow `verified` without an improved metric (the note must cite the evidence).
        #[arg(long)]
        force: bool,
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
            let rec = retro::recurrences(&runs, &agent.findings);
            let n = retro::candidates(&rec, &lessons, project.lab.recurrence).len();
            if let Some(note) = &agent.note {
                eprintln!("lab: {note}");
            }
            print!(
                "{}",
                brief::render(
                    &project.name,
                    &project.today().to_string(),
                    &lessons,
                    runs.last(),
                    &tags,
                    n
                )
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Retro {
            agent_findings: path,
            since,
            write,
        } => retro_cmd(&project, path.as_deref(), since.as_deref(), write),
        Command::Evaluate => evaluate(&project),
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
        });
    }
    Ok(AgentFiles {
        findings: retro::read_findings(&path)?,
        path: Some(path),
        note: None,
    })
}

/// The agent sessions that measure agent-rule lessons, or why they are unavailable.
fn agent_sessions(project: &Project, agent: &AgentFiles) -> Result<Vec<retro::Session>, String> {
    let path = match (&project.lab.agent_sessions, &agent.path) {
        (Some(p), _) => project.path(p),
        (None, Some(findings)) => findings.with_file_name("sessions.jsonl"),
        (None, None) => return Err("no [lab] agent_findings or agent_sessions configured".into()),
    };
    retro::read_sessions(&path)
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
    if let Some(note) = &agent.note {
        println!("{note}");
    }
    let rec = retro::recurrences(&runs, &agent.findings);
    let cands = retro::candidates(&rec, &lessons, project.lab.recurrence);
    println!(
        "{} check run(s), {} agent finding(s), {} rule(s) seen; {} recurring rule(s) without a lesson (threshold: {} distinct findings).",
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
            "- {sev} {tool}:{rule}: {} distinct finding(s), {} run(s), {} subject(s); e.g. {}",
            r.findings(),
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

    fn measure(&self, lesson: &lessons::Lesson) -> Option<retro::Evaluation> {
        let configured: Vec<&str> = self
            .project
            .lab
            .checks
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        let agent = retro::Agent {
            findings: &self.agent.findings,
            sessions: self.sessions.as_deref().map_err(String::as_str),
        };
        retro::evaluate(lesson, &self.runs, &configured, &agent, |t| {
            self.project.date_of(t).ok()
        })
    }
}

fn evaluate(project: &Project) -> Result<ExitCode, String> {
    let lessons = lessons::load(&project.lessons_dir())?;
    let evidence = Evidence::load(project)?;
    let mut any = false;
    for l in lessons.iter().filter(|l| l.meta.status.active()) {
        let Some(e) = evidence.measure(l) else {
            continue;
        };
        any = true;
        let fmt = |m: Option<f64>| m.map_or_else(|| "-".into(), |v| format!("{v:.2}"));
        let metric = l
            .meta
            .metric
            .as_ref()
            .map(|m| format!("{}:{}", m.tool, m.rule))
            .unwrap_or_default();
        println!(
            "{} ({metric}): {} per {unit} before ({} {unit}s), {} after ({} {unit}s): {}",
            l.meta.id,
            fmt(e.before_mean),
            e.before_runs,
            fmt(e.after_mean),
            e.after_runs,
            e.verdict,
            unit = e.unit
        );
    }
    if any {
        println!(
            "These counts are observational: other changes in the same period can explain a difference."
        );
    } else {
        println!("No adopted lesson has both a metric and an adoption date.");
    }
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
                let verdict = Evidence::load(project)?
                    .measure(lesson)
                    .map_or_else(|| "not measurable".to_string(), |e| e.verdict);
                if verdict != retro::IMPROVED {
                    return Err(format!(
                        "`{id}` cannot be verified: its metric shows {verdict}; \
                         pass --force with a note citing other evidence"
                    ));
                }
            }
            lesson.transition(status, &note, &project.today().to_string())?;
            lessons::save(lesson)?;
            println!("{} is now {}", id, status.as_str());
        }
    }
    Ok(ExitCode::SUCCESS)
}
