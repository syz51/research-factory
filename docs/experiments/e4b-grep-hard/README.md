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
5. **Order and cut.** Topics without a handle come first, then topics by handle in descending order; ties keep test-set order. T09 comes first in any case: it failed in all four E4 conditions. The other E4 `cli` failure cannot be named, because E4's transcripts were not kept. The first 24 topics are the set. 24 leaves room above the minimum of 20, should a session fail to run.

This is a proxy for "the answer has no distinctive words": paraphrase questions, whose words differ from the record's, and cross-record questions, whose words are common across the archive. Whether the `cli` baseline is below ceiling on these topics is a result, reported below.

## Conditions and method

To be completed with the run.
