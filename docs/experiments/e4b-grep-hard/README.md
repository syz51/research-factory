# E4b: does MCP delivery raise correct answers where grep fails?

A follow-up of [E4](../e4-delivery/README.md), for board task research-factory-264. Run on 2026-11-12.

## Question

E4 found no difference in correct answers between `cli` and `mcp`, because every condition was near ceiling (21–22 of 24). Grep reached a judged record in about 3 calls. E4b asks: on topics where a grep session is likely to fail, does the `mcp` delivery answer correctly more often than `cli`?

## Selection rule

The rule was written down before any topic was scored or any session was run. [select_topics.py](select_topics.py) applies it.

1. **Pool.** Topics of the masters archive search test set (`docs/process/archive-search-testset-2026-11-12`, masters commit 12f8ae8) of type known-item or topical with at least one grade-2 record in `qrels-records.txt`. Passage and reference topics are left out, as in E4: their paper texts are git-ignored, and reference topics ask for registry rows.
2. **Terms.** The session prompt holds the topic's `need`. Its terms are the lowercase alphanumeric tokens of the `need`, of 3 or more characters, without the fixed stop list in the script.
3. **Corpus.** The git-tracked Markdown files of the scratch copy, outside `private/` (2,363 files). The document frequency of a term is the number of corpus files that contain it as a whole word, case-insensitive.
4. **Grep handle.** A topic's handle is the smallest document frequency among the need's terms that occur in at least one of its grade-2 records. A small handle means that a grep for one word of the question lists few files, and a grade-2 record is among them. A topic whose grade-2 records contain no need term has no handle.
5. **E4's topics.** E4's `cli` sessions answered 22 of its 24 topics correctly, so those topics are at ceiling. T09 failed in all four E4 conditions and stays in the pool. The other 23 E4 topics leave it: the second E4 `cli` failure cannot be named, because E4's transcripts were not kept.
6. **Order and cut.** T09 comes first. Then come topics without a handle, then topics by handle in descending order; ties keep test-set order. The first 24 topics are the set. 24 leaves room above the minimum of 20, should a session fail to run.

Amendment, before any session ran: the first draft of this rule (commit fd6e57a) had no step 5. Its 24 topics held 11 E4 topics, at least 10 of them answered correctly by E4's `cli`. Step 5 removes them, because a set of known-ceiling topics cannot answer the question.

This is a proxy for "the answer has no distinctive words": paraphrase questions, whose words differ from the record's, and cross-record questions, whose words are common across the archive. Whether the `cli` baseline is below ceiling on these topics is a result, reported below.

## Topics

[topics.txt](topics.txt), from `select_topics.py <masters copy> <testset> 24`: T09 K18 T12 K26 K30 T17 T11 K14 T20 K17 K12 K41 K24 T18 K20 K45 K25 K40 K39 K27 K46 T10 K16 K38. The pool held 43 topics, and every one had a handle. The selected handles run from 698 files (K18) to 84 (K38); T09's is 179.

## Conditions and method

| Condition | What the session has |
| --- | --- |
| `cli` | The research-factory skill and the `lab brief` at session start. The scratch copy has no `.mcp.json`, so the brief teaches `lab search` and says how to add `lab mcp`. |
| `mcp` | The same skill, `lab mcp` as an MCP server (`archive_search`, `archive_open`, `archive_related`), and the scratch copy's `.mcp.json`, so the brief points to `archive_search`. This is the delivery that E4 made the default. |

- Model: Claude Opus 5.5 (`claude-opus-5-5`) through the `claude` CLI 2.1.322 in headless mode (`claude -p`), on the subscription. The cost figures are the notional `total_cost_usd`; no API credit was spent.
- Archive: two scratch copies (`git clone`) of masters at 12f8ae8, one per condition, without `private/`. Each has the skill of research-factory main (a38a227) and only the `lab brief` SessionStart hook. `lab` 0.21.5. In masters, `check_qrels.py` exits 0 at 12f8ae8. Nothing was written in masters.
- Sessions: [run_session.sh](run_session.sh) uses E4's prompt and flags. 24 topics × 2 conditions × 2 replicates = 96 sessions, 4 at a time. [order.py](order.py) writes the queue: odd topics run `cli mcp mcp cli`, even topics `mcp cli cli mcp`. So each condition starts first in half of the topics, and the sessions of one topic run together. `order.log` records each start.
- Answer key: `qrels-records.txt` of the test set, as in E4. All 24 topics have grade-2 records there.
- Measures: [measure.py](measure.py) calls E4's `session()` (`../e4-delivery/measure.py`), so `correct`, `strict`, tool calls, main context tokens and search share are graded exactly as in E4. It adds the token totals of the result message and each session's start position.
- Tests: [analyze.py](analyze.py). A pair is the `cli` and `mcp` session of one topic with the same replicate number, so 48 pairs. The primary test is the exact McNemar test on `correct`. The secondary tests are Holm-adjusted together: McNemar on `strict`, a topic-level sign test on correct counts, and Wilcoxon signed-rank tests on calls, tokens and cost.
- Pilot: one session per condition on K13, outside the set, checked the setup. It is not in the results.

## Round 1: the topics' `need` as the prompt

96 sessions, all ended in success. Raw rows: [raw/round1-need.jsonl](raw/round1-need.jsonl) (one per session, numbers and topic ids only). Start order: [raw/round1-order.log](raw/round1-order.log). Full output of `analyze.py`: [raw/round1-analysis.md](raw/round1-analysis.md).

| Metric | cli | mcp | p |
| --- | ---: | ---: | ---: |
| correct | 45/48 | 43/48 | 0.5 (exact McNemar) |
| strict | 39/48 | 36/48 | 1 |
| topics with more correct answers | 2 | 0 | 1 (sign test) |
| main tool calls (mean / median) | 3.1 / 2.5 | 5.6 / 5.0 | 2.7e-07 |
| main context tokens (mean / median) | 48,355 / 46,512 | 47,490 / 46,846 | 1 |
| all tokens, all calls (mean / median) | 179,893 / 142,217 | 236,980 / 219,434 | 3.8e-05 |
| output tokens (mean / median) | 1,266 / 1,166 | 1,280 / 1,160 | 1 |
| notional cost in USD (mean / median) | 0.203 / 0.199 | 0.209 / 0.205 | 0.80 |
| search share (mean / median) | 0.29 / 0.00 | 0.89 / 1.00 | 2.9e-06 |
| sessions with a search | 21/48 | 48/48 | |

Paired correct answers, 48 pairs: both 43, `cli` only 2, `mcp` only 0, neither 3. Secondary p-values are Holm-adjusted over 10 tests. All tool calls equal main tool calls: no session started a subagent.

- **Ceiling again.** The selection rule did not find topics where grep fails. `cli` answered 45 of 48 correctly. Only three topics had a wrong session: K12 (0/2 in both conditions), T09 (cli 1/2, mcp 0/2) and T12 (cli 2/2, mcp 1/2).
- **The failures.** In K12, all four sessions cited the same record, which the key does not judge. That may be a gap in the key, not a miss. In T09, three sessions cited a 2026-11-12 record that the key grades 0.
- **Correct answers.** No difference (p=0.5). The 2 discordant pairs both favour `cli`.
- **Cost of `mcp`.** 2.5 more tool calls per session and 32% more tokens over all calls. The context at the last call and the notional cost do not differ. The run order no longer favours a condition: each started first in 12 sessions, and the first session of a topic wrote about 21,000 to 22,000 cache tokens in both conditions, against 16,000 to 17,000 for the others (`analyze.py`, run-order table).
- **Search use.** `mcp` searched in every session (search share 0.89). `cli` ran `lab search` in 21 of 48 sessions (share 0.29), against 1 of 24 in E4: the current skill and brief teach it more strongly.

Conclusion of round 1: the `need` sentences carry words that grep finds. Across E4 and round 1, 47 of the 66 pool topics have run under `cli`, and only K12, T09, T12 and one unnamed E4 topic had a wrong answer. With `need` prompts, the test set cannot supply 20 topics below ceiling.

## Round 2: paraphrased prompts, screened on cli failures

This rule was written down before any round-2 session ran.

1. **Prompts.** Each test-set topic has two `phrasings`: other ways to ask for the same thing, so the topic's grades still apply. A round-2 prompt is E4's prompt with a phrasing in place of the `need`. Phrasings are shorter and lack the `need`'s explanation, as a user's question often does.
2. **Pool.** The 66 known-item and topical topics with a grade-2 record (round 1's pool with E4's topics back in, since the prompt differs). 2 phrasings each: 132 prompts.
3. **Screening.** One `cli` session per prompt, graded as above. The screening sessions are not part of the comparison.
4. **Selection.** A topic is selected when a screening session of one of its phrasings is wrong. Its prompt is its first wrong phrasing. Topics with both phrasings wrong come first, then test-set order; the first 24 are the set. With fewer than 20, all selected topics run and the shortfall is reported.
5. **Comparison.** Fresh sessions, as in round 1: 2 per condition per topic, the first condition alternating per topic, 4 at a time. Selection used only screening sessions, so the fresh `cli` and `mcp` sessions are both independent of it. Fresh `cli` sessions may succeed more often than in screening (regression to the mean); that affects the ceiling, not the comparison.

### Screening

132 `cli` sessions, all ended in success: 115 correct, 17 wrong. Rows: [raw/round2-screen.jsonl](raw/round2-screen.jsonl), start order: [raw/round2-screen-order.log](raw/round2-screen-order.log). The wrong prompts (topic-phrasing): K03-1 K08-2 K11-1 K12-1 K12-2 K17-1 K19-1 K20-1 K24-2 K25-1 K28-1 K28-2 K35-1 T02-2 T09-2 T18-1 T19-2. `screen.py select` gives 15 topics, 2 of them with both phrasings wrong: [topics-round2.txt](topics-round2.txt) (K12 1, K28 1, K03 1, K08 2, K11 1, K17 1, K19 1, K20 1, K24 2, K25 1, K35 1, T02 2, T09 2, T18 1, T19 2). That is 5 short of the 20 the task asked for; by the rule, all 15 ran. `cli` ran `lab search` in 84 of the 132 screening sessions: short prompts made it search more than the `need` did.

### Comparison

60 sessions (15 topics × 2 conditions × 2 replicates), all ended in success. Rows: [raw/round2-compare.jsonl](raw/round2-compare.jsonl), start order: [raw/round2-compare-order.log](raw/round2-compare-order.log), full output: [raw/round2-analysis.md](raw/round2-analysis.md).

| Metric | cli | mcp | p |
| --- | ---: | ---: | ---: |
| correct | 6/30 | 9/30 | 0.51 (exact McNemar) |
| strict | 4/30 | 6/30 | 1 |
| topics with more correct answers | 3 | 5 | 1 (sign test) |
| main tool calls (mean / median) | 3.0 / 3.0 | 5.1 / 5.0 | 7.9e-05 |
| main context tokens (mean / median) | 47,303 / 46,679 | 47,250 / 46,902 | 1 |
| all tokens, all calls (mean / median) | 181,118 / 174,714 | 223,328 / 217,587 | 0.050 |
| output tokens (mean / median) | 1,211 / 1,152 | 1,132 / 1,045 | 0.79 |
| cache-creation tokens (mean / median) | 12,272 / 11,651 | 17,214 / 18,741 | 0.0007 |
| notional cost in USD (mean / median) | 0.156 / 0.148 | 0.202 / 0.207 | 0.0037 |
| search share (mean / median) | 0.48 / 0.33 | 0.96 / 1.00 | 0.00065 |
| sessions with a search | 24/30 | 30/30 | |

Paired correct answers, 30 pairs: both 3, `cli` only 3, `mcp` only 6, neither 18. Secondary p-values are Holm-adjusted over 10 tests.

| Topic | cli correct | mcp correct |
| --- | ---: | ---: |
| K03 | 0/2 | 0/2 |
| K08 | 0/2 | 0/2 |
| K11 | 1/2 | 2/2 |
| K12 | 0/2 | 1/2 |
| K17 | 0/2 | 0/2 |
| K19 | 1/2 | 2/2 |
| K20 | 0/2 | 1/2 |
| K24 | 1/2 | 0/2 |
| K25 | 0/2 | 2/2 |
| K28 | 0/2 | 0/2 |
| K35 | 1/2 | 0/2 |
| T02 | 1/2 | 0/2 |
| T09 | 0/2 | 0/2 |
| T18 | 0/2 | 0/2 |
| T19 | 1/2 | 1/2 |

- **Below ceiling.** Fresh `cli` sessions answered 6 of 30 correctly (20%), so these topics are not at ceiling. Screening did not just catch chance failures.
- **Correct answers.** `mcp` answered 9 of 30, 3 more than `cli`. The difference is not significant: 6 discordant pairs favour `mcp` and 3 favour `cli`, exact McNemar p=0.51. With 9 discordant pairs, only a split of 8-1 (p=0.039) or 9-0 (p=0.004) would reach p<0.05. By topic, `mcp` did better on 5 topics and worse on 3.
- **Not search against grep.** `cli` also searched, with `lab search` in 24 of 30 sessions. So round 2 compares the MCP tools with the same search run from the shell, more than it compares search with grep.
- **Neither correct in 18 pairs.** Six topics failed in all four sessions: K03, K08, K17, K28, T09 and T18. A phrasing can be broader than its `need`: the key grades the record that states the present status, and a phrasing such as a short query may also match older records. Part of these failures may be prompts that the key does not fit, not misses of either delivery.
- **Cost.** `mcp` took 2.1 more tool calls and 23% more tokens over all calls. Its notional cost was 29% higher, but that figure is confounded: the `cli` comparison sessions started right after the 132 `cli` screening sessions, with the same prompt-cache prefix, so they wrote fewer cache tokens (12,272 against 17,214 per session).

## Regrade against the test set at masters 410b5e4 (research-factory-342)

The sections above grade against the test set at masters 12f8ae8. There, some wrong answers named only files that the key had never graded. research-factory-342 graded those files blind in masters: 109 cited (topic, file) pairs on 43 topics, 27 at grade 2, 45 at grade 1 and 37 at grade 0. The test set's README, section "Addition, 13 November 2026", gives the method. This section regrades the same 290 transcripts with the same `measure.py` and `analyze.py` against the test set at masters 410b5e4. No session ran again. Rows and full output: [raw/regrade-342/](raw/regrade-342/).

| Part | Correct at 12f8ae8 | Correct at 410b5e4 | Strict at 12f8ae8 | Strict at 410b5e4 |
| --- | ---: | ---: | ---: | ---: |
| Round 1, `cli` | 45/48 | 45/48 | 39/48 | 37/48 |
| Round 1, `mcp` | 43/48 | 44/48 | 36/48 | 33/48 |
| Screening, `cli` | 115/132 | 118/132 | 99/132 | 91/132 |
| Round 2, `cli` | 6/30 | 8/30 | 4/30 | 5/30 |
| Round 2, `mcp` | 9/30 | 14/30 | 6/30 | 6/30 |

Correct answers only rose: 11 sessions changed from wrong to correct and none the other way. `strict` fell in 21 sessions and rose in 9: a file that the key now grades 0 makes an answer that names it not strict.

Paired tests after the regrade:

| Round | Pairs | Both | `cli` only | `mcp` only | Neither | Exact McNemar p | Topics better for `cli` / `mcp` (sign test, Holm) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Round 1 | 48 | 44 | 1 | 0 | 3 | 1 | 1 / 0 (p=1) |
| Round 2 | 30 | 6 | 2 | 8 | 14 | 0.109 | 2 / 6 (p=0.87) |

Round 2 by topic after the regrade (`cli` / `mcp` correct of 2): K03 0/1, K08 0/0, K11 2/2, K12 0/1, K17 0/0, K19 1/2, K20 0/1, K24 1/0, K25 0/2, K28 0/0, K35 1/0, T02 2/2, T09 0/0, T18 0/2, T19 1/1. The tool calls, tokens and cost do not depend on the key and do not change.

- **Round 1.** One `mcp` session (T12) is now correct. `cli` 45 and `mcp` 44 of 48, p=1. No change in conclusion.
- **Round 2.** Seven sessions are now correct: 2 `cli` (K11, T02) and 5 `mcp` (K03, T02 twice, T18 twice). `mcp` answered 14 of 30 against 8 of 30. The discordant pairs split 8 to 2 for `mcp`, exact McNemar p=0.109. The direction favours `mcp` more clearly, but the difference is still not significant at 0.05.
- **Selection under the new key.** Three screening failures are now correct: K11 phrasing 1, K12 phrasing 1 and T02 phrasing 2. Under the new key, the round-2 rule would not have selected these three topics. Without them (12 topics, 24 pairs): `cli` 4 and `mcp` 9 of 24, discordant 2 to 7, p=0.18. The primary result stays the 15 topics as selected, because the rule was fixed before the sessions.
- **K12 in round 1.** The record that all four round-1 K12 sessions cited (`brief-path4.md`) is now graded 1, not 2. Those four sessions stay wrong.
- **Neither correct.** Four topics now fail in all four round-2 sessions: K08, K17, K28 and T09.

**Files outside the corpus.** Four graded pairs are on files outside the test set's pool corpus, so the qrels leave them out: `lab-eval/README.md` of the test set (K41 grade 2, K37 grade 1) and `finrate-source-audit-inputs.md` (grade 0 for K41 and for K37). The tables above count them as the qrels do: not judged. This follows the test set's rule that the corpus defines the test set. As a check, the four grades were added to a copy of `qrels-records.txt` and the three parts regraded. No `correct` value changed: every session that cites one of them was already correct. `strict` fell in 7 sessions (round 1: K41, all 4 sessions; screening: K37 phrasing 1, K41 phrasings 1 and 2), because they also name the grade-0 file. Round 2 did not change.

## Conclusion

The bullets use the regraded results (test set at masters 410b5e4). The results at 12f8ae8 are in the sections above.

- On topics where `cli` is at ceiling (round 1), `mcp` does not raise correct answers (44 against 45 of 48, p=1). At 12f8ae8: 43 against 45, p=0.5.
- On topics where a `cli` session failed (round 2, 15 topics), `mcp` answered 14 of 30 against 8 of 30. The direction favours `mcp`, but the test is not significant (p=0.109; at 12f8ae8, 9 against 6, p=0.51). E4b does not show that MCP delivery raises correct answers. The regrade did not change this conclusion; it made the direction clearer.
- In both rounds, `mcp` costs about 2 more tool calls and 23–32% more tokens per session, at equal context in the last call. E4's decision (`lab mcp` as default, because it makes agents search) does not change: E4b gives no reason to reverse it, and no correctness gain to add to it.
- To detect a gain of the size seen, a run needs more below-ceiling topics than this test set has. With `need` prompts the test set has about 4; with phrasings, 15 (12 under the new key). With the regraded split (a third of pairs discordant, 8 of 10 for `mcp`), an exact test at alpha 0.05 needs about 20–26 discordant pairs for power 0.80–0.87, about 60–80 pairs. With the split at 12f8ae8 (6 of 9), it needs about 250 pairs. research-factory-343 plans that study.

## Cost and usage

All sessions ran on the subscription; the dollar figures are the notional `total_cost_usd`.

| Part | Sessions | Notional USD |
| --- | ---: | ---: |
| Pilot (K13) | 2 | 0.45 |
| Round 1 | 96 | 19.79 |
| Round 2 screening | 132 | 28.25 |
| Round 2 comparison | 60 | 10.73 |
| Total | 290 | 59.22 |

The board's weekly usage reading went from 88% before the pilot to 90% after the last session.

## Limits

- Round 2 has 15 topics, not 20, and 30 pairs. Its power is low; see the conclusion.
- The pairs within a topic are not independent: both replicates share the topic and the prompt. The McNemar tests treat the 48 and 30 pairs as independent. The topic-level sign test does not, and it agrees (p=1 in both rounds; after the regrade, p=1 and p=0.87).
- One model, one archive, one answer key. `correct` is graded by file, from the paths the answer names, as in E4. At masters 12f8ae8, some wrong answers cited only unjudged files: 4 of 8 in round 1, 3 of 17 in screening, 8 of 45 in the round-2 comparison. research-factory-342 graded them; see the regrade section. The new grades come from one blind judge per unit, so they can still be wrong.
- Round 2's prompts are the test set's phrasings, whose grades were made for the `need`. See "Neither correct in 18 pairs".
- Round 2's cost is confounded by the screening run before it (see "Cost" above). Round 1's is not: each condition started first in 12 sessions.
- The start position comes from `order.log`. Sessions of one topic start within the same second, so the position is the order in which `xargs` started them.
- The transcripts hold archive text and stay out of this repository. They are on the `air` storage pool at `research-factory/e4b-grep-hard/e4b-transcripts.tgz` (pilot, both rounds and the screening).

## Reproduce

```sh
# Two scratch copies of masters with only the `lab brief` hook and this repo's skill;
# masters-cli without .mcp.json, masters-mcp with it.
export E4B_MASTERS=<folder with masters-cli and masters-mcp> \
  E4B_TESTSET=<masters>/docs/process/archive-search-testset-2026-11-12 E4B_LAB=<dir of lab>
# Round 1
uv run --no-project python select_topics.py "$E4B_MASTERS/masters-cli" "$E4B_TESTSET" 24 > topics.txt
uv run --no-project python order.py topics.txt | E4B_OUT=out xargs -P4 -L1 sh run_session.sh
uv run --no-project python measure.py "$E4B_TESTSET" out > raw/round1-need.jsonl
uv run --no-project --with scipy python analyze.py raw/round1-need.jsonl
# Round 2
uv run --no-project python screen.py queue "$E4B_TESTSET" | E4B_OUT=screen xargs -P4 -L1 sh run_session.sh
uv run --no-project python measure.py "$E4B_TESTSET" screen > raw/round2-screen.jsonl
uv run --no-project python screen.py select "$E4B_TESTSET" raw/round2-screen.jsonl 24 > topics-round2.txt
uv run --no-project python order.py topics-round2.txt | E4B_OUT=out2 xargs -P4 -L1 sh run_session.sh
uv run --no-project python measure.py "$E4B_TESTSET" out2 > raw/round2-compare.jsonl
uv run --no-project --with scipy python analyze.py raw/round2-compare.jsonl
# Regrade (research-factory-342): the test set at masters 410b5e4, the transcripts from air
git -C <masters> archive 410b5e4 docs/process/archive-search-testset-2026-11-12 | tar x -C ts
storagectl get air:research-factory/e4b-grep-hard/e4b-transcripts.tgz e4b.tgz && tar xzf e4b.tgz
T=ts/docs/process/archive-search-testset-2026-11-12
uv run --no-project python measure.py "$T" out > raw/regrade-342/round1-need.jsonl
uv run --no-project python measure.py "$T" screen > raw/regrade-342/round2-screen.jsonl
uv run --no-project python measure.py "$T" out2 > raw/regrade-342/round2-compare.jsonl
uv run --no-project --with scipy python analyze.py raw/regrade-342/round1-need.jsonl
uv run --no-project --with scipy python analyze.py raw/regrade-342/round2-compare.jsonl
# Without K11, K12 and T02 (the selection under the new key)
jq -c 'select(.topic != "K11" and .topic != "K12" and .topic != "T02")' raw/regrade-342/round2-compare.jsonl > r2-12.jsonl
uv run --no-project --with scipy python analyze.py r2-12.jsonl
```

The runs went in batches of 6 topics (44 prompts for screening), each after a check that the weekly usage was below 95%.
