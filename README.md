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
| `lab` (here) | Runs every checker, keeps the findings, turns recurring problems into lessons, and hands the adopted lessons to each new session |

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
| `lab brief [--tag TAG]` | The session-start brief: the rules of adopted lessons, each an instruction to follow (those learned from failures first), the results of each configured check's latest run, lessons due for evaluation, and the number of lesson candidates (project-wide, even with `--tag`). |
| `lab retro [--agent-findings FILE] [--since DATE] [--write]` | Finds warn and error rules with at least `recurrence` distinct findings (by fingerprint) across stored check runs and agent sessions that no lesson covers. A finding left unfixed across many runs counts once. `--write` drafts each as an `observed` lesson. |
| `lab lesson add / list / set` | Records lessons and moves them through `observed → proposed → adopted → verified` or `retired`. Every status change needs a note and is appended to the lesson's history. |
| `lab evaluate` | For each adopted lesson with a metric, compares that rule's findings per check run before and after adoption, counting only runs in which every configured check that reports the tool completed. A metric of an agent-session tool such as `agentprof` is measured per session instead, from `sessions.jsonl`. |

A lesson can be marked `verified` only when `lab evaluate` reports fewer findings since adoption, with at least two runs (or sessions) on each side, unless `--force` is given with a note that cites other evidence. A lesson without a metric always needs `--force`. The comparison is observational: other changes in the same period can explain a difference. Adopting a lesson again after retiring it restarts the measured period.

A finding's fingerprint covers its tool, rule, severity, message and subject, without the line number of a `path:line` subject. So a finding keeps its fingerprint when lines are added above it, and becomes new when its severity or message changes. Identical findings are numbered in order.

`<state_dir>` defaults to `.research`. The lessons directory should be committed. Commit `.research/lab/checks/` and `.research/lab/baseline.json` as well if check history and the accepted baseline should travel with the project, which `lab evaluate` and `--new-only` need on every machine; otherwise ignore `.research/`.

Check names must be unique. A check's program with a directory part, such as `scripts/check.sh`, resolves against the folder holding `research.toml`; a bare name is looked up on `PATH`.

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
