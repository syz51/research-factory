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
| `lab check [--only NAME] [--new-only] [--verbose]` | Runs the checks, stores the findings under `<state_dir>/lab/checks/`, and prints each check's counts with new and resolved findings since the last run. Exits 1 on any error finding, or with `--new-only` only on errors that are new, and on any check that failed to run. |
| `lab brief [--tag TAG]` | The session-start brief: the rules of adopted lessons (failures to avoid first), the last check's results, lessons due for evaluation, and the number of lesson candidates. |
| `lab retro [--agent-findings FILE] [--since DATE] [--write]` | Finds warn and error rules that recur in at least `recurrence` check runs or subjects and that no lesson covers. `--write` drafts each as an `observed` lesson. |
| `lab lesson add / list / set` | Records lessons and moves them through `observed → proposed → adopted → verified` or `retired`. Every status change needs a note and is appended to the lesson's history. |
| `lab evaluate` | For each adopted lesson with a metric, compares that rule's findings per check run before and after adoption. |

A lesson can be marked `verified` only when `lab evaluate` reports fewer findings since adoption, with at least two runs on each side, unless `--force` is given with a note that cites other evidence. The comparison is observational: other changes in the same period can explain a difference.

`<state_dir>` defaults to `.research`. The lessons directory should be committed. Commit `.research/lab/checks/` as well if check history should travel with the project, which `lab evaluate` needs on every machine; otherwise ignore `.research/`.

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
