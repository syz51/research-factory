# research-factory

A set of small command-line tools for running a research project with AI agents, where accuracy matters more than speed, and where the workflow learns from its own failures.

This repository holds the shared contracts, the `lab` CLI that ties the tools together, an agent skill, and the method behind them. Each other tool lives in its own repository and works on its own:

| Tool | Job |
| --- | --- |
| [`papers`](https://github.com/syz51/papers) | Reference registry: build it from a bibliography, check it, verify metadata against arXiv, Crossref, OpenAlex, DBLP and others, fetch open-access full text, and export BibTeX or CSL-JSON |
| [`claimcheck`](https://github.com/syz51/claimcheck) | Checks a document's claims against its sources: quotations that are not in the source text, numbers the source does not contain, citations to entries read only at abstract depth, uncited claims, and broken reference lists |
| [`prov`](https://github.com/syz51/prov) | Provenance: content-hash manifests of sources and results, duplicate files, run records, reruns compared with the original, and lineage from a result back to its inputs |
| [`ireval`](https://github.com/syz51/ireval) | Retrieval evaluation with trec_eval-compatible metrics and paired significance tests corrected for multiple comparisons |
| [`agentprof`](https://github.com/syz51/agentprof) | Profiles Claude Code, Codex and T3 agent sessions: tokens, estimated cost, time, failures, and wasteful or risky patterns |
| `lab` (here) | Runs every checker, keeps the findings, turns recurring problems into lessons, hands the adopted lessons to each new session, and keeps the research archive indexed, sealed and checked |

The tools agree on three formats, described in [docs/interfaces.md](docs/interfaces.md): one `research.toml` per project, findings as `rf.finding/1` JSON Lines, and the reference registry as `rf.reference/1` JSON Lines. None of them links another's code.

- [docs/factory.md](docs/factory.md) explains how the parts fit together and how the learning loop works.
- [docs/accuracy.md](docs/accuracy.md) sets out the accuracy method and the published evidence behind it.
- [skill/research-factory/SKILL.md](skill/research-factory/SKILL.md) is an agent skill that applies the workflow in Claude Code or Codex.

## Install `lab`

Install a Rust toolchain of version 1.99 or later, then:

```sh
cargo install --git https://github.com/syz51/research-factory --locked
```

## Use `lab`

```sh
lab init --name my-thesis     # writes research.toml and lessons/README.md; never overwrites
```

Then list the project's checkers in `research.toml`. Each check is a command that prints `rf.finding/1` JSON Lines and exits with 0 (no errors), 1 (errors) or 2 (the checker itself failed):

```toml
[[lab.checks]]
name = "claims"
run = ["claimcheck", "check", "docs", "--format", "jsonl"]
cwd = "."                      # optional, relative to research.toml
# version = ["claimcheck", "--version"]   # optional; see below
# uncommitted_inputs = ["sources/text"]   # optional; for `lab check --at`
```

| Command | What it does |
| --- | --- |
| `lab check [--only NAME] [--new-only] [--verbose]` | Runs the checks, stores the findings under `<state_dir>/lab/checks/` with the git commit and whether the work tree was dirty (since 0.7.0), stores the `rf.measure/1` records of checkers run with `--measure` (since 0.8.0) and each checker's version (since 0.9.0), prints each check's counts with the findings new and resolved relative to its baseline, and lists the errors that fail the gate. Exits 1 on any error finding, or with `--new-only` only on errors not in the baseline, and on any check that failed to run or printed output that is neither `rf.finding/1` nor a valid `rf.measure/1` record. |
| `lab check --at COMMIT [--at A..B] [--only NAME] [--verbose]` | Replays past commits (since 0.9.0): checks out each commit, or each commit of a range that changed the project (git's `A..B` leaves out `A`, so write `A^..B` to include it), into a temporary git worktree, runs today's checks there, and stores the run with that `commit`, `dirty` false, `at` set to the commit's committer time and `replayed` true. It removes the worktree afterwards, also when a step fails, and removes worktrees that a replay stopped by a signal, such as Ctrl-C, left behind. A commit without the project root is skipped with a note. Replayed runs only give `lab evaluate` interval ends, such as a baseline before a lesson's adoption; the brief, `lab retro`, `lab baseline set` and the introduced findings ignore them. A check's `uncommitted_inputs` are linked from the current work tree into the commit, and the run lists them. Exits 1 when a check failed to run in a replay; findings do not fail it. |
| `lab baseline set --note WHY [--only NAME]` / `lab baseline show` | Accepts each check's findings from its latest completed run as the baseline that `--new-only` compares against, and records the note in the baseline's history. `lab check` never moves the baseline, so re-running it cannot clear the gate. Without a baseline, every error counts as new. |
| `lab brief [--tag TAG]` | The session-start brief: the rules of adopted lessons, each an instruction to follow (those learned from failures first), the results of each configured check's latest run, each measured lesson's verdict from the last `lab loop` and the loop's health (or, without `[lab.loop]`, the lessons due for evaluation; in a linked git worktree, since 0.9.9, the main checkout's last loop), and the number of lesson candidates (project-wide, even with `--tag`). |
| `lab retro [--agent-findings FILE] [--since DATE] [--write]` | Finds warn and error rules with at least `recurrence` recurrences across stored check runs and agent sessions that no lesson covers. A check-run finding counts once per distinct finding (by fingerprint), so a finding left unfixed across many runs counts once. Agent-session findings count once per session group: a root session and the subagents below it count once. `--agent-findings FILE` reads the `sessions.jsonl` next to FILE. Without a readable sessions file, agent findings count per distinct finding, and `lab retro` says why. `--write` drafts each as an `observed` lesson. |
| `lab lesson add / list / set / review` | Records lessons and moves them through `observed → proposed → adopted → verified` or `retired`. Every status change needs a note and is appended to the lesson's history. `review ID --count N --note WHY` marks a check-run lesson's introduced findings as reviewed, for those that cannot be fixed: it checks that `lab evaluate` still lists N of them, records the latest complete run's file name as `reviewed:` in the lesson's frontmatter and appends the note to its history. `lab evaluate` then counts only findings introduced after that run. Lesson files with `reviewed:` need lab 0.6.0 or later: older versions refuse to load the lessons directory, so update lab on every machine that shares the project before the first review. |
| `lab evaluate [--format text\|jsonl]` | For each adopted or verified lesson, tests whether its metric changed after adoption and gives a verdict: `collecting`, `improved`, `worse`, `inconclusive` or `unmeasurable`. An agent-session rule is measured per session (did it have the finding?) within each source and role, counting the subagents below one root session as one group: an exact stratified test when every group is one session, otherwise a stratified permutation test that moves whole groups; a `session.<field>` metric with the van Elteren rank test; a checker's rule is described by the findings introduced since the first complete check run after adoption, or since the run of its last `lab lesson review`, and tested by the exact conditional binomial test on findings introduced per new unit of text, over intervals between clean runs on consecutive commits whose checks report `rf.measure/1` (since 0.8.0), in the unit the lesson's rule counts when a checker reports several, such as claimcheck 0.2.0 (since 0.9.2). `--format jsonl` prints `rf.evaluation/2` records. |
| `lab loop [--if-older MINUTES] [--background] [--format text\|jsonl]` | Runs the `[lab.loop] measure` commands, evaluates every lesson, and sends each new or changed signal (a lesson to verify, one that got worse or cannot be measured, findings a check-run lesson has seen introduced, uncovered recurring rules, a failed measure command) to the `notify` command once. `--background` returns at once and leaves a detached copy running; `--if-older` skips the run when the last one finished recently. Hooks run `lab loop --background --if-older 15`. Started in a linked git worktree, it runs for the main checkout's project instead, so worktree sessions share its measures and signal state. Exits 1 when a measure or notify command failed. |
| `lab archive update` | The archive's maintenance step, run before `lab check`: `lab archive index`, then `lab archive seal`. Exits 1 if either reports a problem. |
| `lab archive index` | Regenerates each `[[lab.archive.index]]` file from its records' front matter, one line per record, and writes only the files that changed. Writes nothing and exits 1 when a record's front matter is invalid. |
| `lab archive seal [--accept PATH --note WHY]` | Records the length and SHA-256 of each `sealed` (append-only) file's body in the seal manifest, and accepts growth that keeps the sealed bytes. Reports an edited or deleted sealed file and exits 1. `--accept` re-seals one edited file and keeps the note in the manifest's history. |
| `lab archive check [--format text\|jsonl]` | Read-only check of the archive: invalid front matter, missing required fields, duplicate or unknown ids, supersession links that disagree, stale or missing indexes, size budgets, broken relative links and anchors (a link to a file in an `[[lab.archive.offloaded]]` list is counted in one `archived-link` note instead, and a broken link in the sealed text of an append-only file that a `[[lab.archive.kept_link]]` entry records with a reason, since 0.9.11, is a `kept-link` note), current pages linking inactive records, and unsealed, edited or missing sealed files. With `--format jsonl` it is a `[[lab.checks]]` checker. |
| `lab search WORDS... [--limit N] [--status S] [--records] [--in notes] [--json] [--explain] [--rebuild]` | Ranked full-text search over the project's notes (since 0.11.0). A hit is one Markdown section: the record's id, date, status and title, then `path#Lstart-Lend` with the heading path and a snippet. Sections with every word rank first, then sections with some, each naming the words it lacks; an exact record id ranks first; a part of a word finds the word; a superseded record ranks below its successor and names it. A phrase goes in double quotes inside one argument, and several arguments of which one holds a space are several phrasings of one question, fused into one list. The default is 8 hits and about 2 KB; `--records` gives one hit for each record. The index (SQLite FTS5, under `<state_dir>/lab/search/`, one for each checkout, ignored by git) is built on the first query and refreshed on each later one for changed and deleted files. Exits 0 with hits, with a partial answer (the reason on stderr) and with `0 results` (with the nearest indexed words); exits 2 on an error. `--json` prints one `rf.search/1` object whose `state` is `hits`, `partial`, `zero` or `error`. Needs `[lab.archive]`; `[lab.search]` sets the files and their source classes. |
| `lab archive find TERMS... [--limit N]` | The same as `lab search --in notes --records TERMS...` (since 0.11.0; before, a match of every term in a record's id, title, summary, aliases or path, with exit 1 when nothing matched). |
| `lab archive next PREFIX [--width N]` | Prints the next free id with that prefix, keeping the existing zero padding (`D007` → `D008`); `--width` starts a new series. |

A lesson can be marked `verified` only when `lab evaluate` says `improved`: at least `min_sessions` (default 10) session groups on each side of the adoption date and a one-sided p-value below `alpha/2` (default 0.025). Otherwise `--force` is needed, with a note that cites other evidence; a lesson without a metric always needs it. For a check-run metric, `lab evaluate` lists the findings introduced since the first complete run after adoption, keyed by message and data. Fix them, and mark the ones that cannot be fixed, such as findings in append-only files, with `lab lesson review`. Its verdict needs `min_sessions` intervals on each side: intervals between clean runs (no uncommitted changes) on consecutive commits whose checks report a denominator (`rf.measure/1`, from a checker run with `--measure`). Until then it stays `collecting`, and with too few intervals before adoption the reason names the missing baseline; `lab check --at` can replay commits before adoption to supply it, unless no commit predates adoption in the before-window, which the reason then says (since 0.9.5). Intervals across a change of a required checker's recorded version are left out, and since 0.9.6 so are intervals measured with another version than the current one (that of the run stored last); since 0.9.7 a run without a recorded version, such as one stored before 0.9.0, takes for that comparison the first version recorded by a run stored after it. The reason counts both, and `lab check --at <first>^..HEAD` measures every commit again with the current checker. The comparison is observational: other changes in the same period can explain a difference, and with many lessons measured some will pass by chance. Adopting a lesson again after retiring it restarts the measured period. [docs/factory.md](docs/factory.md#how-a-lesson-is-evaluated) describes the method and its limits, and [docs/interfaces.md](docs/interfaces.md) the `[lab.evaluate]` and `[lab.loop]` settings, the records and the notify protocol.

A lesson's metric is a finding rule, `tool:rule`, or a numeric agent-session field, such as `agentprof:session.max_prompt_tokens` or `agentprof:session.compactions.count`, where lower is better.

A finding's fingerprint covers its tool, rule, severity, message and subject, without the line number of a `path:line` subject. So a finding keeps its fingerprint when lines are added above it, and becomes new when its severity or message changes. Identical findings are numbered in order.

`<state_dir>` defaults to `.research`; `lab loop` keeps its lock, last results (`last.json`), signal state and log in `<state_dir>/lab/loop/`. The lessons directory should be committed. Commit `.research/lab/checks/` and `.research/lab/baseline.json` as well if check history and the accepted baseline should travel with the project, which `lab evaluate` and `--new-only` need on every machine; otherwise ignore `.research/`.

Check names must be unique. A check's program with a directory part, such as `scripts/check.sh`, resolves against the folder holding `research.toml`; a bare name is looked up on `PATH`.

Each run records each checker's version: the first line that the check's `version` command prints, run where the check runs. Without `version`, lab runs `<program> --version` when the program is a bare name and not an interpreter or wrapper, such as `sh`, `python3.12`, `uv`, `npm` or `make`. A script's version is unknown unless `version` names a command, and `version = []` records none. An unknown version is `null`, and lab prints a warning when a command gives none.

`lab check --at` runs each check, and its `version` command, in the commit's copy of the project root, with its `cwd` resolved there. Only the program, `run[0]`, resolves against the current project root. So a checker installed on `PATH` is today's, but a script named as an argument, such as `run = ["sh", "scripts/check.sh"]`, or a tool that `uv run` or `bunx` finds from the working directory, is the commit's; its version, when `version` reads it, is the commit's too. Checkers read that commit's files, including its `research.toml`. Files that no commit holds, such as git-ignored source texts, are missing from a replay. List them as `uncommitted_inputs` (paths relative to the project root): lab links each one, or each entry of a directory that the commit holds only a placeholder of (such as its `.gitignore`), from the current work tree, and the run's check lists it, since the replay then reads today's copy and is not faithful for it. The links point into the work tree, so list only inputs the check reads: a check that writes to them changes the work tree. Hooks do not run in the worktree. lab 0.8.0 and older read a replayed run as a live one, and refuse a `research.toml` whose `[[lab.checks]]` has `version` or `uncommitted_inputs`, so update lab on every machine that shares the project before using either.

In a linked git worktree (since 0.9.10), lab finds the git-ignored files that only the main checkout has, such as agent-session scans, downloaded sources and data, without copying them. It looks a path up in the main checkout only when the worktree lacks it and git ignores it there, so a tracked file deleted in the worktree stays missing. It never lists, copies or changes anything in the main checkout. Three places use it, and each says so on stderr:

- `lab evaluate`, `lab retro` and `lab brief` read the main checkout's `[lab] agent_findings` and `agent_sessions` files, such as `.research/agentprof/`. A scan in the worktree itself takes precedence.
- `lab archive check` only checks that a link target exists in the main checkout; it does not check the target's anchors.
- `lab check` passes a check argument that has a directory part, such as `docs/sources/source-manifest.json`, as the main checkout's absolute path. Findings that name that path get the argument as written again, so their fingerprints match the main checkout's baseline.

A replay's worktree (`lab check --at`) gets none of this: it reads only the inputs it links itself.

`lab archive` reads `[lab.archive]`; `lab init` writes a commented example, and [docs/factory.md](docs/factory.md#the-archive) explains the layout it maintains. Its globs are relative to the folder holding `research.toml`; `*` does not cross `/` and `**` matches any depth. The walk enters only directories a glob can match below and never follows a symbolic link to a directory. With `[lab.archive]` configured, `lab brief` ends with the start page to read first and the indexes to search.

Lesson files are Markdown with YAML frontmatter, so they read well on GitHub and in Obsidian:

```markdown
---
id: quote-from-text
title: Quote from extracted text
kind: bad
status: adopted
scope: writing
tags: [citations]
rule: Copy quotations from the source's extracted text, never from memory.
metric: claimcheck:quote-not-found
created: 2026-11-07
updated: 2026-11-07
adopted: 2026-11-07
evidence: [docs/survey.md:42]
---

What happened and why it matters.

## History

- 2026-11-07 adopted: three quotations in the survey did not appear in their sources.
```

## Develop

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## Licence

Licensed under either the [MIT licence](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option.
