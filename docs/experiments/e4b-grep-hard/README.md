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
