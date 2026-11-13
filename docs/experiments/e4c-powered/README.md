# E4c: a powered cli vs mcp study on topics where grep fails (pre-registration)

Task: research-factory-343. This file is the pre-registration. Its merge commit on main is the pre-registration date. No session of this study, screening included, runs before that commit. Changes after it follow "Amendments".

## Question

On archive questions that grep and BM25 cannot answer from the question's words, does `lab mcp` delivery (`mcp`) give more correct answers than `lab search` taught by `lab brief` (`cli`)?

E4b ([../e4b-grep-hard/README.md](../e4b-grep-hard/README.md)) asked this on 15 topics and 30 pairs. It found 8 vs 14 correct of 30 after the regrade (p=0.109). Its power was low because the test set had too few topics where `cli` fails. E4c adds new grep-hard topics to the masters test set and runs at least 250 pairs.

Hypothesis: `mcp` answers correctly more often than `cli`. The test is two-sided.

## New topics in the masters test set

The masters test set is `docs/process/archive-search-testset-2026-11-12` in masters. A masters thread adds the new topics under a sub-task of research-factory-343. This section is its rule.

### How a topic is made

- Type: `known-item` or `topical`, about the notes (the type scope of these types).
- Kind: one of `paraphrase`, `supersession`, `cross-record` or `table` (see the list below). The writer sets it before grading, and `gate.tsv` records it.
- `origin`: `grep-hard-343`. `as_of`: the masters commit the topic's units were judged at.
- `id`: the next free ids of the test set's scheme (`K` for known-item, `T` for topical).
- The writer, a Claude Opus agent, reads the archive and picks one to three seed records that answer a question. Then it writes the topic: `need` (what counts as relevant), `query`, and two `phrasings`.
- The phrasings avoid the answer's distinctive words: record ids, file names, people's and tool names, and rare terms of the seed records. Each topic uses one of these kinds:
  - a paraphrase: the question uses other words than the records;
  - the present state after a supersession: a later record replaced the answer;
  - a cross-record question: the answer needs two records;
  - a number or a value that is in a table or a list.
- Each phrasing must be a question that a user could ask without having read the seed records.
- The writer must not use the test set's existing topics or E4b's topics as a source.

### How a topic is graded blind

- Pool per topic, at unit level as the test set's `pool.py` pools: the seeds, and the top 10 records of the grep word-count baseline and of BM25 over all units (mapped to records), each run on phrasing 1 and on phrasing 2. A record enters as its best section for the phrasing, as in `pool.py`.
- Judges: Claude Code `worker` subagents on claude-opus-5-5, on the subscription, with `lab-eval/judge-system.md` and the test set's grading rules. A judge sees the topic (type, query, need) and the unit texts. It never sees a seed mark, a run, a rank or a source.
- Second judge: 10 of the new topics, drawn with `random.Random(343)`, are graded again by a second blind judge. Report Cohen's kappa (unweighted and linear weighted), as research-factory-234 did. The first grade is kept.
- A topic is admitted only if at least one record has grade 2. A topic without one is dropped and is not replaced by a new topic in the same batch.
- Records outside the pool corpus stay out of the qrels, as in research-factory-342.
- The new topics are kept out of the test set's existing figures: the base-82 and query-log tables do not change. `check_qrels.py` exits 0 after each batch.

### The grep-hard gate

A phrasing passes the gate if no grade-2 record of its topic is in the top 10 of the grep word-count baseline or of BM25 (records) for that phrasing. The pool covers both top-10 lists, so the gate uses judged records only. The masters thread writes a file `extension-343/gate.tsv` in the test set. It has the header line `topic phrasing grep_rank bm25_rank pass batch kind`, then one line per phrasing of each admitted topic, separated by spaces:

- `grep_rank`, `bm25_rank`: the rank of the best grade-2 record in that run's top 10, `-` when there is none;
- `pass`: `yes` if the phrasing passes the gate, else `no`;
- `batch`: the batch number, 1 to 5;
- `kind`: the topic's kind.

Each batch appends its lines; earlier lines do not change.

### How many topics

- Make the topics in batches of 40. The first two batches (80 topics) are needed in any case.
- Let s be the share of new topics that screening selects (gate passed and `cli` wrong; see "Screening"). E4b's ordinary topics gave s of about 0.2 (13–15 of 66). Gated topics are made to fail, so the plan assumes s = 0.3: about 160 topics (4 batches) for 50 selected topics.
- Each batch is screened before the next one is made. Add a batch while the selected count is below 50 and the total is below 200. Report s after each batch.
- The maximum is 200 topics (5 batches). With s ≥ 0.16, 200 topics give the minimum of 32 selected topics.

## Setup

- Model: claude-opus-5-5 through the `claude` CLI in headless mode (`claude -p`), on the subscription, with the flags of [../e4b-grep-hard/run_session.sh](../e4b-grep-hard/run_session.sh). No API credit is spent; the USD figures are notional.
- Conditions: as in E4b. `cli`: the research-factory skill and the `lab brief` hook, no `.mcp.json`. `mcp`: the same and `lab mcp` as an MCP server.
- Archive: two scratch copies (`git clone`) of masters, one per condition, without `private/` and without any `docs/process/archive-search-testset-*` folder. The sessions read the prompt from the test set outside the copies, as E4b did.
  - Screening of batch b runs in copies at C_b, the masters commit that adds batch b's topics, grades and `gate.tsv` lines.
  - The comparison runs in copies at C, the commit of the last batch screened. Every comparison session uses C.
  - The report lists each C_b and C. Archive changes between C_b and C are a limit of the study: a topic is selected on the archive at C_b and compared on the archive at C. `check_qrels.py` must exit 0 at C.
- Pins: the `lab` version, the `claude` CLI version, the model and the research-factory skill commit are recorded before the first screening session. They stay the same for every session. If a pinned part must change, the study stops, and the change is an amendment.
- Leak check: after each batch, check that no transcript read `qrels-records.txt`, `topics.jsonl` or `extension-343/` through a tool call. E4b had 0 such reads in 290 sessions.
- Measures: [../e4b-grep-hard/measure.py](../e4b-grep-hard/measure.py). `correct`: the answer names a grade-2 record of the topic. `strict`: correct and no grade-0 record named.

## Screening

- Pool: the admitted new topics. E4b's 15 compared topics and every topic of the test set that is not `grep-hard-343` are excluded.
- Queue: one `cli` session per phrasing of the batch that passes the gate (`run_session.sh cli <topic> <k> <k>`, phrasing k as the rep number, as E4b's screening). Each batch queues only its own `gate.tsv` lines, so no phrasing is screened twice.
- Selection: a topic is selected if at least one screened phrasing was wrong (`correct` false). It is compared on that phrasing; if both were wrong, on phrasing 1.
- Key completion for screening: before selection, the cited records without a grade for the topic are graded blind (see "Key completion"). Selection uses the completed key.
- Order: batch by batch, in the order the topics were added. Screening stops after the batch in which the selected count reaches 50, or after the last batch.
- Let n be the selected count. If n < 32, the comparison does not run. The study then reports "underpowered" with the screening data, and research-factory-343 asks the user what to do.

## Comparison and sample size

- Replicates per topic: r = max(2, ceil(250 / n)), at most 8. So the comparison has n × r ≥ 250 pairs. With n = 50, r = 5 and 250 pairs.
- A pair is the `cli` and the `mcp` session of one topic with the same replicate number.
- Order: [order.py](order.py). The selected topics are sorted by id and shuffled with `random.Random(343)`. Topic i runs its replicates in order. Replicate k starts with `cli` when i + k is even, else with `mcp`. The queue runs 4 sessions at a time, in order.
- At least one hour passes between the last screening session and the first comparison session.
- Size rationale: E4b at 12f8ae8 had 9 discordant pairs in 30, 6 of them for `mcp`. So the discordant share is 0.3 and the `mcp` share of discordant pairs is 2/3. An exact two-sided McNemar test at alpha 0.05 needs about 75 discordant pairs for power 0.80, so about 250 pairs. A simulation with the interim look and the futility rule below gives power 0.795 at 250 pairs (0.873 at 300). The regraded split (2 of 10 discordant pairs for `cli`) would need about 60–80 pairs. The plan uses the 6:9 split, as the decision note on research-factory-343 says.

## Key completion

The key is the test set's `qrels-records.txt`. Sessions can cite records that were not judged for the topic. Before each analysis (screening, interim and final), every record cited by a session and not judged for its topic is graded blind. A masters thread does this as research-factory-342 did: a `worker` judge sees the topic and the unit texts, never the condition, the session or the answer. The grades go to masters main before the analysis runs.

The primary analysis uses the completed key. The report also gives the results with the key before completion.

## Analysis

[analyze.py](analyze.py) runs every test below on the pairs of the look. It takes the queue that the sessions ran from, so it uses all r replicates and, at the interim, only the pairs of the first half.

- Primary test: exact McNemar on `correct` (two-sided binomial test on the discordant pairs, p = 0.5). Its alpha comes from "Interim look".
- Topic-clustered analysis: the pairs within one topic are not independent. For each topic t, d_t = (mcp correct − cli correct) / pairs of t.
  - A sign-flip randomization test on the mean of d_t: exact with up to 20 topics, else 100,000 random sign vectors with seed 343.
  - A 95% percentile bootstrap interval for the mean of d_t over topics: 10,000 resamples, seed 343.
- Claim: `mcp` gives more correct answers only if the primary test is significant at its alpha, and the clustered test gives p < 0.05 in the same direction. If the primary test is significant and the clustered test is not, the report says the gain depends on a few topics.
- Secondary tests, five, Holm-adjusted together: exact McNemar on `strict`; a topic-level sign test on correct counts over all replicates (ties dropped); Wilcoxon signed-rank tests (zero differences dropped) on main tool calls, main context tokens and notional cost.
- Descriptive, without tests:
  - the per-topic table of correct counts;
  - correct answers by which session of a pair started first, from the `order.log` positions;
  - correct answers by topic kind, from `gate.tsv`.

## Interim look

- One interim look on the first half of the pairs in queue order: the first floor(P/2) pairs, where P = n × r.
- The queue runs in two parts. Part 1 is the queue lines of the first floor(P/2) pairs. When part 1 is done and no session runs, the interim analysis runs. Part 2, the other lines, starts only after the interim decision. `analyze.py --look interim` also cuts the rows at floor(P/2) pairs.
- Alpha spending: Lan-DeMets with an O'Brien-Fleming-type function, total two-sided alpha 0.05, information fraction 0.5. Nominal alpha: 0.00557 at the interim (|z| ≥ 2.772), 0.0478 at the final look (|z| ≥ 1.979). With P ≥ 250, floor(P/2)/P is at least 0.498, and the interim alpha at that fraction differs by less than 0.0001.
- Efficacy stop: the interim primary p < 0.00557 and the clustered test agrees (as in "Claim"). The study stops and reports.
- Futility stop: non-binding. The study stops if the `mcp`-only pairs are not more than the `cli`-only pairs at the interim. The final alpha does not use this rule, so it stays valid if the study continues anyway. The simulation gave type I error 0.020 with this rule and 0.80 power without it.
- Key completion runs for the first half before the interim analysis.
- Otherwise the study continues to P pairs, and the final look uses alpha 0.0478.

## Stopping and usage rules

- Before each batch of at most 40 sessions, read the weekly usage in the board's `board_snapshot`.
  - Launch the batch only if the usage is below 95%.
  - Stop launching at 97%. Resume after the weekly reset.
  - Record each pause and resume in this README and on research-factory-343.
- Estimate: E4b's 290 sessions moved the weekly usage by 2 points. E4c has about 160–400 screening sessions and 500 comparison sessions, so about 5–7 points, over more than one week.
- A started session is not stopped for usage. A session that fails (no result message) is run again once, and the failure is reported.
- Other stops: a forced change of a pinned part (see "Setup"), a leak, or a defect in the scripts. Each is reported and handled as an amendment.

## Amendments

A change to this design is a dated commit to this file, made before the sessions it affects, with its reason. Sessions run before an amendment are analyzed by the rules they ran under. The report lists every amendment.

## Reproduce

```sh
E=../e4b-grep-hard TS=docs/process/archive-search-testset-2026-11-12 E4B_LAB=<dir of lab>
# Screening of batch b: scratch copies and the test set at C_b
export E4B_MASTERS=<folder with masters-cli and masters-mcp at C_b> E4B_TESTSET=<masters at C_b>/$TS
awk -v b=<b> 'NR > 1 && $5 == "yes" && $6 == b {print "cli", $1, $2, $2}' \
  "$E4B_TESTSET/extension-343/gate.tsv" > screen-queue-<b>.txt
E4B_OUT=screen xargs -P4 -L1 sh $E/run_session.sh < screen-queue-<b>.txt
# After key completion for screening: K = the masters commit with those grades
git -C <masters> archive <K> $TS | (mkdir -p key && tar x -C key)
uv run --no-project python $E/measure.py key/$TS screen > raw/screen.jsonl
# selected.txt: `<topic> <phrasing>` per selected topic (see "Screening")
# Comparison: scratch copies at C; part 1 is the first floor(P/2) pairs (2 lines each)
export E4B_MASTERS=<folder with masters-cli and masters-mcp at C> E4B_TESTSET=<masters at C>/$TS
uv run --no-project python order.py selected.txt <r> > queue.txt
P=$(( $(wc -l < queue.txt) / 2 ))
head -n $(( P / 2 * 2 )) queue.txt | E4B_OUT=out xargs -P4 -L1 sh $E/run_session.sh
# Interim, after key completion for part 1 (K1 = the masters commit with those grades)
git -C <masters> archive <K1> $TS | (rm -rf key && mkdir -p key && tar x -C key)
uv run --no-project python $E/measure.py key/$TS out > raw/interim.jsonl
uv run --no-project --with scipy --with numpy python analyze.py raw/interim.jsonl \
  --queue queue.txt --look interim --gate key/$TS/extension-343/gate.tsv
# Part 2, if the interim says continue
tail -n +$(( P / 2 * 2 + 1 )) queue.txt | E4B_OUT=out xargs -P4 -L1 sh $E/run_session.sh
# Final, after key completion for all sessions (K2); the key before completion is <masters at C>/$TS
git -C <masters> archive <K2> $TS | (rm -rf key && mkdir -p key && tar x -C key)
uv run --no-project python $E/measure.py key/$TS out > raw/compare.jsonl
uv run --no-project python $E/measure.py "$E4B_TESTSET" out > raw/compare-before-completion.jsonl
uv run --no-project --with scipy --with numpy python analyze.py raw/compare.jsonl \
  --queue queue.txt --gate key/$TS/extension-343/gate.tsv
```

The scripts were checked on E4b's regraded round 2, with E4b's `order.py` queue: `analyze.py ../e4b-grep-hard/raw/regrade-342/round2-compare.jsonl --queue <queue>` gives 8 vs 14 of 30, p=0.109, as E4b's README states. The clustered difference is +0.200 (95% interval 0.000 to +0.433, sign-flip p=0.188). `strict` (5 vs 6) and the topic counts (2 vs 6) match E4b. The Holm-adjusted p-values differ from E4b's because E4c has five secondary tests, not ten.

### Amendment 1 (2026-11-13): usage cap lifted by the user

- 2026-11-12 ~16:30Z: paused before any session. Weekly usage read 95%.
- 2026-11-13 03:30Z: resumed (amendment commit 03:30:45Z, first session 03:30:57Z). The user reported a usage reset and lifted the 95% and 97% caps of "Stopping and usage rules" for now. The board's usage endpoint still read 97% at 03:29Z (resets_at 2026-11-15 01:00Z), so the reading may lag the reset.
- Effect: sessions launch without the usage check until the user restores the cap. Every other rule stays the same. Usage does not enter selection or the analysis, so no measure changes.
- Pins, recorded before the first screening session: `lab` 0.22.0, `claude` CLI 2.1.323, model claude-opus-5-5, research-factory skill at 9961a5f (`git archive`).

### Amendment 2 (2026-11-13): a leak through git history

- Found after batch 2 screening, before any further session. The copies were `git clone`s, as in E4b. Removing the test-set folder from the working tree left it in git history.
- Leak: one of 65 screening sessions (batch 2, K80 phrasing 2) ran `git grep ... HEAD -- docs/process/archive-search-testset-2026-11-12`. The output showed K80's own lines of `topics.jsonl` and `extension-343/written.jsonl`, with its seed record. That session answered correctly.
- Other exposures, no answer leak: in batch 1, K77 phrasing 2 read the test-set README and `git show --stat HEAD` (file names and line counts only). The README names three other `grep-hard-343` topics with grade-0 records. T30 phrasing 1 read one README line.
- Changes, for every session from now on:
  - Copies: after `git clone --no-local` and checkout of C_b or C, `git filter-repo --path-glob 'docs/process/archive-search-testset-*' --invert-paths` (git-filter-repo through `uv run`). HEAD's tree stays the same as C_b or C without the test set. No commit in any ref keeps a test-set file. Commits that touched only the test set drop out.
  - Leak check: it also scans tool results, not only tool inputs, for `qrels`, `topics.jsonl`, `extension-*/` key files and `archive-search-testset` paths.
  - K80 phrasing 2 is invalid. It runs again once in a filtered copy at a71b91b, and the new run replaces it in screening. The invalid transcript is kept and reported.
- Batches 1 and 2 otherwise stand, screened in unfiltered copies. This is a limit: the screening copies differ from the comparison copies in git history only.

### Amendment 3 (2026-11-13): a narrower leak-check pattern

- Set before batch 3 screening, in board note #5915 on research-factory-343 (2026-11-13 07:58Z). Batch 3 started at 08:09Z. This commit came later, with the results, so it breaks the rule of "Amendments" above. See "Limits".
- Amendment 2's pattern matched any `qrels` or `topics.jsonl` text. On batches 1 and 2 it flagged 14 of 65 sessions, mostly archive prose that mentions qrels. That is too broad for a pass or fail check.
- New pattern, on tool inputs and tool results: test-set paths under `qrels`, `topics.jsonl` or `extension-<n>/`, lines of `qrels-records.txt`, and a `"seeds": [` field. On batches 1 and 2 it flags K80 phrasing 2 (the leak), K77 phrasing 2 and T30 phrasing 1, the three exposures that amendment 2 lists.
- Each flagged session is read by hand. Batches 3 to 5 ran in filtered copies, and the check flagged none of their 85 sessions.

## Results: screening, and the study is underpowered

Screening ended after batch 5, the maximum of 200 topics. At most 30 topics are selected, below the minimum of 32. By "Screening", the comparison does not run.

### Outcome and decision

- **Outcome: underpowered.** n is at most 30, below the minimum of 32. No comparison session ran, so this study has no cli vs mcp result. E4b ([../e4b-grep-hard/README.md](../e4b-grep-hard/README.md)) stays the only paired result on grep-hard topics.
- **Decision (14 November 2026).** The pre-registration asked the user what to do (ask #174 on research-factory-343). The options were: stop and report underpowered; run an exploratory comparison on the selected topics; or add batches past 200 topics. The owner chose the first: "take option 1 for ask 174". The study stops here. Batch 5's key completion and the comparison do not run.
- **What it found.** Grep-hard topics are rarer than planned: s = 0.125 over batches 1 to 4, against the planned 0.3. `cli` answered 98 of 120 screened phrasings correctly. Key completion cut the selected topics of batches 1 to 4 from 35 to 20: 20 sessions named relevant files that the key had not judged. See "What the screening shows".
- A new study needs an amendment to this pre-registration, such as a higher topic maximum or a different selection rule.

### Per batch

| Batch | C_b | Admitted of 40 | Topics with a gate-passing phrasing | Sessions | Correct (key completed) | Selected before completion | Selected (key completed) | Key K |
|---|---|---|---|---|---|---|---|---|
| 1 | 41d3057 | 33 | 28 | 37 | 30 | 9 | 7 | ae12511 |
| 2 | a71b91b | 30 | 20 | 28 | 19 | 11 | 8 | ae12511 |
| 3 | 81bad6d | 31 | 22 | 33 | 31 | 8 | 2 | 9e75b8d |
| 4 | b1cf03e | 29 | 18 | 22 | 18 | 7 | 3 | 9349b6a |
| 5 | 3da6935 | 30 | 22 | 30 | not completed | 10 | at most 10 | none |
| All | | 153 | 110 | 150 | | 45 | 20 to 30 | |

- Sessions: one `cli` session per gate-passing phrasing. All 150 ended in success; none ran again for a missing result. K80 phrasing 2 of batch 2 ran again after the leak (amendment 2). The invalid first run is left out of the rows and counts.
- Selected topics, batches 1 to 4 (topic and compared phrasing): [raw/selected-b1-b4.txt](raw/selected-b1-b4.txt). Batch 5 before completion: [raw/selected-b5-before-completion.txt](raw/selected-b5-before-completion.txt).
- Rows: `raw/screen-b<b>.jsonl` with the completed key, and `raw/screen-b<b>-before-completion.jsonl` with the key at C_b (batches 1 and 2: at a71b91b). Start order: `raw/screen-b<b>-order.log`. The batch 2 log keeps both K80 phrasing 2 starts: 03:34:59Z (invalid) and 03:41:20Z, marked `rerun`. `measure.py` reads only 5-field lines, so for batch 2 drop the first K80 2 line and the `rerun` word before measuring.
- The selected count can only fall with key completion. So n is at most 20 + 10 = 30 whatever batch 5's completion finds. Batch 5's completion did not run, because it cannot change the outcome.

### What the screening shows

- **s is lower than planned.** The plan assumed s = 0.3 and needed s ≥ 0.16. Over batches 1 to 4, s = 20 / 160 = 0.125. With batch 5 at its maximum, s ≤ 30 / 200 = 0.15.
- **`cli` answers most grep-hard prompts.** With the completed key, `cli` named a grade-2 file in 98 of 120 screened phrasings of batches 1 to 4 (82%). The gate keeps phrasings that grep and BM25 rank poorly, yet the agent still found a relevant record. 134 of the 150 sessions ran `lab search`.
- **Key completion removed most failures.** Before completion, 35 topics of batches 1 to 4 were selected. With the completed key, 20 were. 20 sessions turned correct (batches 1–2: 7, batch 3: 8, batch 4: 5). Each named a file that the test set had not judged for its topic and that the blind judges graded 2. The pooled key misses relevant records that an agent finds.
- **Kinds.** Of the 20 selected topics, 9 are paraphrase, 5 supersession, 3 table and 3 cross-record. All 3 cross-record topics come from batches 1 and 2. All batches used the same judge rule (judge-system.md: a record that gives only part of the need is grade 1). From batch 3 on, the batch threads added one step: a fresh blind regrade of every grade-2 unit of the cross-record topics (decision on research-factory-353). Batches 1 and 2 did not have this step. Batches 3 to 5 then admitted few cross-record topics: none in batch 3, T47 in batch 4 and T55 in batch 5. See "Limits".

### Cost and usage

- Notional cost of the 150 sessions: $39.06 (batch 1 $8.81, batch 2 $8.91, batch 3 $8.21, batch 4 $5.29, batch 5 $7.84). The subscription paid; no API credit was spent.
- Weekly usage read 55% before batch 3, 67% before batch 4 and 69% before batch 5. Batches 1 and 2 ran under amendment 1.

### Limits

- Amendment 3 (the narrower leak-check pattern) was set on the board before batch 3, but committed to this file only after batches 3 to 5 ran. As a check, amendment 2's broad pattern was run on batches 3 to 5 afterwards. It flags 10 of 85 sessions (K113-2, K125-2, K132-1, K139-1, K151-1, K159-2, K178-1, K186-2, K200-2, K201-1). Each hit was read by hand: all are archive prose that mentions qrels, a test-set script name, or the counts of a key-completion record. None shows a topic, seed or grade of the test set.
- The extra cross-record regrade ran in batches 3 to 5 only. The 3 selected cross-record topics of batches 1 and 2 did not get it, so their admission may be looser than in later batches.

- Batches 1 and 2 were screened in unfiltered copies (amendment 2). K80 phrasing 2 saw its own key and was rerun. K77 phrasing 2 read figures and README lines through `git show`, but no K77 grade; both K77 phrasings were correct.
- One `cli` session per phrasing decides selection. A topic can pass or fail by chance. E4b's fresh `cli` sessions on its selected topics were correct 6 of 30 times, so a single failure is a weak signal.
- The key grades files, from the paths an answer names, as in E4 and E4b. Each new grade comes from one blind Opus judge per unit.
- Batch 5's 10 topics before completion are an upper bound. Batches 3 and 4 kept 5 of 15 after completion.
- The transcripts hold archive text and stay out of this repository. They are on the `air` storage pool only, never in cloud storage:
  - `air:research-factory/e4c-powered/e4c-screening-b1-b2.tgz`: batches 1 and 2 (transcripts and queues).
  - `air:research-factory/e4c-powered/e4c-screening-b3.tgz`, `e4c-screening-b4.tgz` and `e4c-screening-b5.tgz`: batches 3 to 5.
  - `air:research-factory/e4c-powered/e4c-work.tgz`: the batch script (`screen_batch.sh`), the queues, the batch logs, the measured rows and the selected lists.
