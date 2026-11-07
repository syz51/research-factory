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
                                lab loop ──► measure, evaluate, signal (from hooks)
```

## The tools

Each tool is a separate binary with its own repository, README and tests. Every tool:

- finds the project by walking up to `research.toml`, and reads only its own table;
- reports problems as `rf.finding/1` JSON Lines when run with `--format jsonl`, and exits 0 when there are no errors, 1 when there are, and 2 when it failed;
- runs offline, except in commands whose job is network access, which space their requests per host and back off on errors;
- reports a value it cannot establish as unknown rather than guessing.

These shared rules let `lab` run any checker without knowing anything about it. A project can add its own checker, such as a script that validates a dataset, by making it print findings in the same format.

## A session

1. **Start:** `lab brief` prints the adopted rules (those learned from failures first, then practices that worked; every rule is an instruction to follow), the results of each check's latest run (a `lab check --only` run replaces only the checks it ran), each measured lesson's verdict from the last `lab loop` (or, without `[lab.loop]`, the lessons due for evaluation). The agent skill tells an agent to run it first and follow it.
2. **Work:** the agent registers each new source with `papers`, saves the extracted text, and records its reading depth. Experiments run under `prov run` and are scored with `ireval`.
3. **Check:** `lab check` runs every checker. With `--new-only` it fails only on errors outside the baseline, the findings accepted with `lab baseline set --note`, so an old backlog does not block new work, while `lab brief` still shows the backlog. Re-running the check never moves the baseline, so the gate clears only when the new errors are fixed or someone deliberately accepts them with a recorded note.
4. **Fix:** errors are fixed or, if the finding is wrong, the checker or its configuration is corrected. Findings are not deleted by hand.

## The learning loop

The factory improves from what goes wrong and from what works.

- **Bad lessons** record a failure to avoid. They usually come from recurring findings: `lab retro` lists every warn or error rule with at least `recurrence` distinct findings (default 3) that no lesson yet measures. One finding left unfixed over many runs counts once. `agentprof` findings join the same pool, so a waste pattern in agent sessions, such as rereading the same large file, becomes a candidate too.
- **Good lessons** record a practice to keep. They come from people and agents noticing what worked, such as a prompt pattern or a checking order. Without a metric, they rely on the evidence cited in the lesson.
- **Status** moves `observed → proposed → adopted → verified`, or to `retired` from any status. Each change needs a note, and the note is appended to the lesson's history, so the file shows why the rule exists.
- **Adoption** is a decision, not a side effect. `lab retro --write` only drafts `observed` lessons; a person or agent adopts one after reading its evidence. Only adopted and verified lessons reach `lab brief`.
- **Evaluation:** `lab evaluate` tests whether a lesson's metric changed after its adoption date (below). `verified` requires the verdict `improved`, or `--force` with a note citing other evidence; a lesson without a metric always needs `--force`.
- **Retirement:** a lesson that did not help, or no longer applies, is retired with a note rather than deleted, so the same idea is not adopted again without knowing it failed before.

The loop is deliberately conservative. Rules reach every session, so a wrong rule costs more than a missing one. That is why adoption needs a person or agent to decide, and why `verified` needs evidence.

### How a lesson is evaluated

A lesson's metric is either a finding rule (`tool:rule`) or a numeric field of the agent-session record (`agentprof:session.max_prompt_tokens`, `session.total.tokens.output`, `session.compactions.count`, `session.active_ms`; lower is better, and a session where the field is missing or null is skipped). Observations are dated in the project's time zone: those before the adoption date and within `[lab.evaluate] baseline_days` (default 90; 0 for all) form the baseline, those on or after it the comparison.

- **Agent-session rules** are measured per session as an indicator: did the session have at least one finding of the rule? Counting sessions rather than findings keeps one runaway session, with dozens of findings, from deciding the result (the counts are overdispersed; the indicator is not). Sessions from different sources (Claude Code, Codex, T3) and roles (main, subagent) have very different base rates, so they are compared only within their stratum `(source, role)`; a stratum without sessions on both sides is left out and counted as `excluded`. The test is exact and conditional: in each stratum, given its group sizes and its total positives, the number of positive after-sessions is hypergeometric if adoption changed nothing, and the distribution of their sum over strata is the convolution of those. `p_lower` is the chance of as few positives as observed or fewer, `p_upper` of as many or more.
- **Session fields** use the van Elteren test: a Wilcoxon rank-sum test within each stratum, combined with weights `1/(N+1)` and a normal approximation without continuity correction, with ties ranked by their average.
- **Check runs** (a metric whose tool a configured check reports) count the rule's findings per complete run: a run in which every configured check that reports the tool ran to completion. Runs repeat the same backlog, so they are not independent observations, and no test is applied: method `descriptive` compares the means. A `descriptive` verdict is weaker evidence than a tested one.

The verdict is `collecting` until both sides have `min_sessions` observations (default 10) in comparable strata, the (source, role) strata with sessions on both sides; `improved` when `p_lower < alpha/2`, `worse` when `p_upper < alpha/2` (default `alpha` 0.05, two-sided); `inconclusive` when there is enough data and neither holds; and `unmeasurable` when the metric cannot show an effect: no baseline, fewer than `min_sessions` baseline observations (the baseline cannot grow), a rule never observed on either side, a field with no numeric value, or a rule so rare that even a million clean sessions would not show a decrease. For a rule that is collecting or inconclusive, `sessions_needed` estimates how many more sessions without the finding, in the baseline's mix of sources and roles, would make the unstratified test show a decrease; it is left out when that test already shows one, since the stratified verdict is what counts.

The limits are real. The comparison is observational: anything else that changed after the adoption date, such as a new model, a different kind of task or another lesson, can explain a difference, and the test cannot tell them apart. Every lesson is tested separately at `alpha/2` per direction, and with many lessons measured repeatedly some will cross the threshold by chance, so read an `improved` verdict with its numbers before verifying, and prefer metrics that the lesson's rule acts on directly. Sessions are treated as independent within a stratum, which a long-running project's correlated sessions only approximate.

### The loop

`lab loop` runs the measuring unattended. It runs each `[lab.loop] measure` command (such as `agentprof scan`) from the project root, evaluates every active lesson, finds the recurring rules without a lesson, and turns what needs a decision into signals (`rf.signal/1`, section 7 of [interfaces.md](interfaces.md)): a lesson to verify, a lesson whose metric got worse, a lesson that cannot be measured, the uncovered rules, and a failing measure command. `collecting` and `inconclusive` produce none; the brief shows them. Each new or changed signal goes once to the `notify` command, such as a script that files a task on a work board, and a changed one is sent again at most every `renotify_hours`; a signal that disappears is resolved, and one that comes back is sent as reopened unless it was last sent less than `renotify_hours` ago, so a verdict that flips back and forth does not file again on every loop.

Claude Code hooks start it on every session start and turn end with `lab loop --background --if-older 15`: the command returns at once, and the detached copy does nothing if the last loop finished less than 15 minutes ago or another loop holds the lock. The latest results are in `<state_dir>/lab/loop/last.json`, which `lab brief` reads; if it is over a day old, the brief says the hooks may not be running.

## The archive

A long research project accumulates notes faster than anyone can reread them. When the entry files grow to hundreds of kilobytes, every session spends most of its context reading them, decisions are buried in dated notes, and the same fact ends up in several files that drift apart. `lab archive` keeps the archive in a shape an agent can use cheaply, and its checks keep it that way:

- **One short entry page.** `START.md` says what the project is, where it stands and where to look next. A size budget keeps it short, and `lab brief` tells every session to read it first.
- **Lookup by id.** Session reports (`history/NNN-YYYY-MM-DD-slug.md`), decisions (`decisions/D###-slug.md`) and follow-up folders (`followups/YYYY-MM-DD-slug/README.md`) each have an id. `lab archive find H158` or `lab archive find bm25 baseline` prints one line per matching record, so an agent opens only the file it needs, and `lab archive next D` gives the next free id.
- **Each fact written once.** A record's id, title, date, status, summary and supersession links live in its own YAML front matter. The indexes are generated from it by `lab archive update`, never edited by hand, so they cannot drift from the records.
- **History is append-only.** The research log is split by month and appended to, oldest entry first. Sealed files (the log and the session reports) may grow but not change: the seal manifest holds the length and hash of each body, and an edit fails `lab archive check` until it is reverted, or accepted with a note that the manifest keeps. A correction is a new dated entry, not an edit. Front matter is outside the seal, so a report can still be marked `superseded`.
- **Decisions are superseded, not rewritten.** A new decision lists the one it replaces in `supersedes`, the old one gets `superseded_by` and an inactive status, and the check reports a side that disagrees. A current page that links an inactive record without saying so is flagged.
- **Links stay valid.** Every relative link and anchor in the configured files must resolve, so moving or renaming a file shows up at once.

A session reads `lab brief` and the start page, looks records up with `lab archive find` and reads large files by section. It appends to the month's log, updates records in place and adds new ones with ids from `lab archive next`. Before finishing it runs `lab archive update`, which regenerates the indexes and seals new and appended files, and then `lab check --new-only`, which runs `lab archive check` among the other checks when it is configured as one.

## Where things live

| Path | Contents | Commit? |
| --- | --- | --- |
| `research.toml` | Project config; one table per tool | Yes |
| `references.jsonl` | Reference registry (`papers`) | Yes |
| `lessons/` | One Markdown file per lesson | Yes |
| Archive indexes and the seal manifest | Generated by `lab archive update` | Yes; keep the manifest out of `.research/` (set `seal_manifest`) so the seals travel with the archive |
| `.research/lab/checks/` | One JSON Lines file per `lab check` run | Commit it so `lab evaluate` works on every machine; otherwise ignore |
| `.research/lab/loop/` | `lab loop`'s lock, last results, signal state and log | No |
| `.research/` (other) | Caches and logs owned by each tool | No |

## Extending the factory

- **A new checker:** any command that prints `rf.finding/1` lines and follows the exit codes. Add it under `[[lab.checks]]`.
- **A new rule in an existing tool:** give it a stable kebab-case id and document it in the tool's README. Lessons and history refer to rules by id, so renaming one breaks their link.
- **A new record type:** version it with a `schema` field (`rf.<name>/1`), document it in the owning tool's README, and list it in [interfaces.md](interfaces.md).
