# E4: how archive search reaches the agent

Experiment E4 of [docs/archive-design.md](../../archive-design.md) (section 5). Run on 2026-11-12 for board task research-factory-204.

## Question

The agent has `lab search`. Which delivery makes it use the search, and answer correctly, with the fewest tool calls and the least context?

## Conditions

| Condition | What the session has |
| --- | --- |
| `cli` | The research-factory skill and the `lab brief` at session start, which teach `lab search` (the baseline). |
| `mcp` | `cli`, and `lab mcp` as an MCP server: `archive_search`, `archive_open`, `archive_related`. |
| `hint` | `cli`, and the grep-hint `PostToolUse` hook (`skill/research-factory/scripts/grep-hint.sh`). |
| `agent` | `cli`, and the archivist subagent (`skill/research-factory/agents/archivist.md`). |

## Method

- Model: Claude Opus 5.5 (`claude-opus-5-5`), through the `claude` CLI 2.1.322 in headless mode (`claude -p`). Every session used the same model.
- Archive: a scratch copy of the masters project at commit 362f2f1, with the skill of this branch and only the `lab brief` SessionStart hook. Nothing was written in masters.
- Topics: 24 topics of the masters archive search test set (`docs/process/archive-search-testset-2026-11-12`). [select_topics.py](select_topics.py) takes the known-item and topical topics where the grep baseline does not rank a grade-2 record first (53 of 82 are eligible), the present-state topics first, then known-item and topical in turn: K31 K32 K33 K34 K35 T14 K01 T01 K02 T02 K03 T03 K04 T04 K05 T05 K06 T06 K07 T07 K08 T08 K09 T09.
- Sessions: 24 topics × 4 conditions = 96 sessions, 4 at a time, one of each condition per topic. [run_session.sh](run_session.sh) records each command. The prompt is the topic's `need`, with a request for a short answer and the `path:start-end` anchor of each passage. The session may read and run commands; it may not write files or use the web. User settings and user agents are not loaded (`--setting-sources project`).
- Measures ([measure.py](measure.py)), per session:
  - `correct`: the answer names a file that the topic grades 2 in `qrels-records.txt`.
  - `strict`: correct, and the answer names no file that the topic grades 0.
  - main tool calls: the main agent's tool calls; all tool calls add the subagent's.
  - main context tokens: input, cache-read and cache-creation tokens of the main agent's last model call.
  - search share: searches (`lab search`, `archive_search`) / (searches + greps); a grep is a Grep or Glob call, or a Bash command that runs grep, rg, ag or find.
- Tests ([analyze.py](analyze.py)): each condition against `cli`, paired by topic. Exact McNemar test for `correct` and `strict`; Wilcoxon signed-rank test for the other measures. Holm correction over all 21 tests.

## Result

| Metric | cli | mcp | hint | agent |
| --- | ---: | ---: | ---: | ---: |
| correct | 22/24 | 22/24 (p=1) | 22/24 (p=1) | 21/24 (p=1) |
| strict | 14/24 | 16/24 (p=1) | 18/24 (p=1) | 16/24 (p=1) |
| main tool calls (mean / median) | 3.0 / 3.0 | 5.0 / 5.0 (p=0.0031) | 3.2 / 3.0 (p=1) | 2.6 / 2.5 (p=1) |
| all tool calls (mean / median) | 3.0 / 3.0 | 5.0 / 5.0 (p=0.0031) | 3.2 / 3.0 (p=1) | 3.5 / 3.0 (p=1) |
| main context tokens (mean / median) | 46,319 / 45,883 | 46,056 / 45,658 (p=1) | 47,348 / 46,837 (p=0.97) | 45,105 / 44,747 (p=0.47) |
| cost in USD (mean / median) | 0.210 / 0.207 | 0.163 / 0.159 (p=0.0002) | 0.171 / 0.168 (p=0.0028) | 0.178 / 0.153 (p=1) |
| search share (mean / median) | 0.02 / 0.00 | 0.75 / 1.00 (p=0.0013) | 0.02 / 0.00 (p=1) | 0.41 / 0.50 (p=0.0047) |
| sessions with a search | 1/24 | 20/24 | 2/24 | 17/24 |
| archivist calls | 0 | 0 | 0 | 4 |

p-values are Holm-adjusted, against `cli`.

- **Search use.** Only `mcp` and `agent` changed it. With the MCP tools in its tool list, the agent searched in 20 of 24 sessions; with the skill alone, in 1. In `agent`, the main agent called the archivist in only 4 sessions, but ran `lab search` itself in most others: the subagent's description, which names `lab search`, worked as a reminder.
- **Correct answers.** No difference. Every condition is near ceiling (21–22 of 24); grep finds the record in a few calls on this archive. T09 failed in all four conditions.
- **Tool calls.** `mcp` takes 2 more calls per session (search, then open), with no gain in correctness here.
- **Context.** No difference: the system prompt, skill and brief (about 40,000 tokens) dominate the last call's context in every condition.
- **Hint.** No effect. The hook speaks after a grep, and on these topics the grep had usually found the answer by then.

## Decision

The default delivery is `lab mcp`: it is the only delivery that made the agent search, at equal correctness. The skill teaches the `archive_search` tool first and `lab search` where it is not configured. `lab brief` points to the tool when the project's `.mcp.json` runs `lab mcp`, and otherwise says how to add it. The grep hint and the archivist stay in the skill, off by default. The decision row is in [docs/archive-design.md](../../archive-design.md) section 4. To reverse it, drop the `.mcp.json` entry; the brief then teaches `lab search` again.

## Search-share target for research-factory-192

In real masters sessions with `lab mcp` configured, at least 0.6 of archive lookups should be searches (`lab search` or `archive_search`) rather than greps, measured as above over the logged sessions of a week. E4 gave 0.75 with MCP and 0.02 without. The target is below 0.75 because real sessions also grep for exact strings, which is right.

## Cost

96 sessions cost USD 17.31 (`total_cost_usd` of each session): `cli` 5.03, `mcp` 3.92, `hint` 4.09, `agent` 4.27. The pilot (4 sessions on K31) and the MCP client check cost about USD 0.88 more.

## Limits

- Ceiling: correctness was near ceiling in every condition, so the experiment cannot show a gain in correct answers. A harder topic set, or an archive where grep fails more often, is needed for that.
- One session per topic and condition, on one model; the 24 topics come from one archive.
- `correct` is graded by file, not by passage, from the paths that the answer names. A path counts when it is the unique suffix of a judged path. Answers that name the right record only in prose are graded wrong.
- `strict` counts a grade-0 file that the answer cites as context as an error, so it is a lower bound.
- Cost is confounded: each batch started the `cli` session first, so `cli` sessions wrote more of the prompt cache (19,320 cache-creation tokens per session against 11,426–13,669). The lower cost of the other conditions is not an effect of the delivery.
- The main-context measure is the last call's context; it does not count the archivist's own context.
- The transcripts hold archive text and stay out of this repository; only topic ids and numbers are recorded here.

## Reproduce

```sh
export E4_MASTERS=<scratch copy of masters> E4_TESTSET=<masters>/docs/process/archive-search-testset-2026-11-12 \
  E4_OUT=<out dir> E4_LAB=<dir of the lab binary> E4_SKILL=<this repo>/skill/research-factory
uv run --no-project python select_topics.py "$E4_TESTSET" 24 > topics.txt
for t in $(cat topics.txt); do for c in cli mcp hint agent; do echo "$c $t"; done; done | xargs -P4 -L1 sh run_session.sh
uv run --no-project python measure.py "$E4_TESTSET" "$E4_OUT" > e4.jsonl
uv run --no-project --with scipy python analyze.py e4.jsonl
```
