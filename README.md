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
```

| Command | What it does |
| --- | --- |
| `lab check [--only NAME] [--new-only] [--verbose]` | Runs the checks, stores the findings under `<state_dir>/lab/checks/`, prints each check's counts with the findings new and resolved relative to its baseline, and lists the errors that fail the gate. Exits 1 on any error finding, or with `--new-only` only on errors not in the baseline, and on any check that failed to run or printed output that is not `rf.finding/1`. |
| `lab baseline set --note WHY [--only NAME]` / `lab baseline show` | Accepts each check's findings from its latest completed run as the baseline that `--new-only` compares against, and records the note in the baseline's history. `lab check` never moves the baseline, so re-running it cannot clear the gate. Without a baseline, every error counts as new. |
| `lab brief [--tag TAG]` | The session-start brief: the rules of adopted lessons, each an instruction to follow (those learned from failures first), the results of each configured check's latest run, each measured lesson's verdict from the last `lab loop` and the loop's health (or, without `[lab.loop]`, the lessons due for evaluation), and the number of lesson candidates (project-wide, even with `--tag`). |
| `lab retro [--agent-findings FILE] [--since DATE] [--write]` | Finds warn and error rules with at least `recurrence` recurrences across stored check runs and agent sessions that no lesson covers. A check-run finding counts once per distinct finding (by fingerprint), so a finding left unfixed across many runs counts once. Agent-session findings count once per session group: a root session and the subagents below it count once. Without a readable `sessions.jsonl`, agent findings count per distinct finding, and `lab retro` says why. `--write` drafts each as an `observed` lesson. |
| `lab lesson add / list / set / review` | Records lessons and moves them through `observed → proposed → adopted → verified` or `retired`. Every status change needs a note and is appended to the lesson's history. `review ID --count N --note WHY` marks a check-run lesson's introduced findings as reviewed, for those that cannot be fixed: it checks that `lab evaluate` still lists N of them, records the latest complete run's file name as `reviewed:` in the lesson's frontmatter and appends the note to its history. `lab evaluate` then counts only findings introduced after that run. Lesson files with `reviewed:` need lab 0.6.0 or later: older versions refuse to load the lessons directory, so update lab on every machine that shares the project before the first review. |
| `lab evaluate [--format text\|jsonl]` | For each adopted or verified lesson, tests whether its metric changed after adoption and gives a verdict: `collecting`, `improved`, `worse`, `inconclusive` or `unmeasurable`. An agent-session rule is measured per session (did it have the finding?) within each source and role, counting the subagents below one root session as one group: an exact stratified test when every group is one session, otherwise a stratified permutation test that moves whole groups; a `session.<field>` metric with the van Elteren rank test; a checker's rule is described by the findings introduced since the first complete check run after adoption, or since the run of its last `lab lesson review`, without a test. `--format jsonl` prints `rf.evaluation/2` records. |
| `lab loop [--if-older MINUTES] [--background] [--format text\|jsonl]` | Runs the `[lab.loop] measure` commands, evaluates every lesson, and sends each new or changed signal (a lesson to verify, one that got worse or cannot be measured, findings a check-run lesson has seen introduced, uncovered recurring rules, a failed measure command) to the `notify` command once. `--background` returns at once and leaves a detached copy running; `--if-older` skips the run when the last one finished recently. Hooks run `lab loop --background --if-older 15`. Exits 1 when a measure or notify command failed. |
| `lab archive update` | The archive's maintenance step, run before `lab check`: `lab archive index`, then `lab archive seal`. Exits 1 if either reports a problem. |
| `lab archive index` | Regenerates each `[[lab.archive.index]]` file from its records' front matter, one line per record, and writes only the files that changed. Writes nothing and exits 1 when a record's front matter is invalid. |
| `lab archive seal [--accept PATH --note WHY]` | Records the length and SHA-256 of each `sealed` (append-only) file's body in the seal manifest, and accepts growth that keeps the sealed bytes. Reports an edited or deleted sealed file and exits 1. `--accept` re-seals one edited file and keeps the note in the manifest's history. |
| `lab archive check [--format text\|jsonl]` | Read-only check of the archive: invalid front matter, missing required fields, duplicate or unknown ids, supersession links that disagree, stale or missing indexes, size budgets, broken relative links and anchors, current pages linking inactive records, and unsealed, edited or missing sealed files. With `--format jsonl` it is a `[[lab.checks]]` checker. |
| `lab archive find TERMS... [--limit N]` | Lists the records whose id, title, summary, aliases or path contain every term, exact id first, as `id`, `date`, `status`, `path` and `title` separated by tabs. Exits 1 when nothing matches. |
| `lab archive next PREFIX [--width N]` | Prints the next free id with that prefix, keeping the existing zero padding (`D007` → `D008`); `--width` starts a new series. |

A lesson can be marked `verified` only when `lab evaluate` says `improved`: at least `min_sessions` (default 10) session groups on each side of the adoption date and a one-sided p-value below `alpha/2` (default 0.025). Otherwise `--force` is needed, with a note that cites other evidence; a lesson without a metric always needs it. A check-run metric gets no verdict: `lab evaluate` lists the findings introduced since the first complete run after adoption, keyed by message and data, and stays `collecting` for good. Fix them, and mark the ones that cannot be fixed, such as findings in append-only files, with `lab lesson review`. The comparison is observational: other changes in the same period can explain a difference, and with many lessons measured some will pass by chance. Adopting a lesson again after retiring it restarts the measured period. [docs/factory.md](docs/factory.md#how-a-lesson-is-evaluated) describes the method and its limits, and [docs/interfaces.md](docs/interfaces.md) the `[lab.evaluate]` and `[lab.loop]` settings, the records and the notify protocol.

A lesson's metric is a finding rule, `tool:rule`, or a numeric agent-session field, such as `agentprof:session.max_prompt_tokens` or `agentprof:session.compactions.count`, where lower is better.

A finding's fingerprint covers its tool, rule, severity, message and subject, without the line number of a `path:line` subject. So a finding keeps its fingerprint when lines are added above it, and becomes new when its severity or message changes. Identical findings are numbered in order.

`<state_dir>` defaults to `.research`; `lab loop` keeps its lock, last results (`last.json`), signal state and log in `<state_dir>/lab/loop/`. The lessons directory should be committed. Commit `.research/lab/checks/` and `.research/lab/baseline.json` as well if check history and the accepted baseline should travel with the project, which `lab evaluate` and `--new-only` need on every machine; otherwise ignore `.research/`.

Check names must be unique. A check's program with a directory part, such as `scripts/check.sh`, resolves against the folder holding `research.toml`; a bare name is looked up on `PATH`.

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
