# How the factory works

The factory is a research workflow run by people and AI agents together. It has three layers: tools that check the work, a record of what they found, and a loop that turns recurring findings into rules for the next session.

```text
            sources ──► papers ──► references.jsonl ◄── claimcheck ◄── documents
                                        ▲                    │
     experiments ──► prov (manifests, runs, lineage)         │
             │                                               │
             └──► ireval (metrics, paired tests)             │
                                                             ▼
   agent sessions ──► agentprof ──► findings ──► lab check / lab retro
                                                     │
                       lessons/ (observed → proposed → adopted → verified | retired)
                                                     │
                                lab brief ──► every new session
                                lab evaluate ──► did the lesson help?
```

## The tools

Each tool is a separate binary with its own repository, README and tests. Every tool:

- finds the project by walking up to `research.toml`, and reads only its own table;
- reports problems as `rf.finding/1` JSON Lines when run with `--format jsonl`, and exits 0 when there are no errors, 1 when there are, and 2 when it failed;
- runs offline, except in commands whose job is network access, which space their requests per host and back off on errors;
- reports a value it cannot establish as unknown rather than guessing.

These shared rules let `lab` run any checker without knowing anything about it. A project can add its own checker, such as a script that validates a dataset, by making it print findings in the same format.

## A session

1. **Start:** `lab brief` prints the adopted rules (those learned from failures first, then practices that worked; every rule is an instruction to follow), the results of each check's latest run (a `lab check --only` run replaces only the checks it ran), and any lessons due for evaluation. The agent skill tells an agent to run it first and follow it.
2. **Work:** the agent registers each new source with `papers`, saves the extracted text, and records its reading depth. Experiments run under `prov run` and are scored with `ireval`.
3. **Check:** `lab check` runs every checker. With `--new-only` it fails only on errors outside the baseline, the findings accepted with `lab baseline set --note`, so an old backlog does not block new work, while `lab brief` still shows the backlog. Re-running the check never moves the baseline, so the gate clears only when the new errors are fixed or someone deliberately accepts them with a recorded note.
4. **Fix:** errors are fixed or, if the finding is wrong, the checker or its configuration is corrected. Findings are not deleted by hand.

## The learning loop

The factory improves from what goes wrong and from what works.

- **Bad lessons** record a failure to avoid. They usually come from recurring findings: `lab retro` lists every warn or error rule with at least `recurrence` distinct findings (default 3) that no lesson yet measures. One finding left unfixed over many runs counts once. `agentprof` findings join the same pool, so a waste pattern in agent sessions, such as rereading the same large file, becomes a candidate too.
- **Good lessons** record a practice to keep. They come from people and agents noticing what worked, such as a prompt pattern or a checking order. Without a metric, they rely on the evidence cited in the lesson.
- **Status** moves `observed → proposed → adopted → verified`, or to `retired` from any status. Each change needs a note, and the note is appended to the lesson's history, so the file shows why the rule exists.
- **Adoption** is a decision, not a side effect. `lab retro --write` only drafts `observed` lessons; a person or agent adopts one after reading its evidence. Only adopted and verified lessons reach `lab brief`.
- **Evaluation:** a lesson with a metric (`tool:rule`) is measured by `lab evaluate`, which compares that rule's findings per check run before and after the adoption date, using only runs in which every configured checker that reports the tool ran successfully. A metric of `agentprof` is measured per agent session, from the `sessions.jsonl` next to the agent findings, so sessions without the finding count as zero. `verified` requires a decrease with at least two runs or sessions on each side, or `--force` with a note citing other evidence; a lesson without a metric always needs `--force`.
- **Retirement:** a lesson that did not help, or no longer applies, is retired with a note rather than deleted, so the same idea is not adopted again without knowing it failed before.

The loop is deliberately conservative. Rules reach every session, so a wrong rule costs more than a missing one. That is why adoption needs a person or agent to decide, and why `verified` needs evidence.

## Where things live

| Path | Contents | Commit? |
| --- | --- | --- |
| `research.toml` | Project config; one table per tool | Yes |
| `references.jsonl` | Reference registry (`papers`) | Yes |
| `lessons/` | One Markdown file per lesson | Yes |
| `.research/lab/checks/` | One JSON Lines file per `lab check` run | Commit it so `lab evaluate` works on every machine; otherwise ignore |
| `.research/` (other) | Caches and logs owned by each tool | No |

## Extending the factory

- **A new checker:** any command that prints `rf.finding/1` lines and follows the exit codes. Add it under `[[lab.checks]]`.
- **A new rule in an existing tool:** give it a stable kebab-case id and document it in the tool's README. Lessons and history refer to rules by id, so renaming one breaks their link.
- **A new record type:** version it with a `schema` field (`rf.<name>/1`), document it in the owning tool's README, and list it in [interfaces.md](interfaces.md).
