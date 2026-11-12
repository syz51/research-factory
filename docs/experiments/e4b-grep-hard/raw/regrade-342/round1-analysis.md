Topics: 24; pairs (topic x replicate): 48; sessions: 96.

| Metric | cli | mcp | p |
| --- | ---: | ---: | ---: |
| correct | 45/48 | 44/48 | 1 (exact McNemar) |
| strict | 37/48 | 33/48 | 1 |
| topics with more correct answers | 1 | 0 | 1 (sign test) |
| main tool calls (mean / median) | 3.1 / 2.5 | 5.6 / 5.0 | 2.68e-07 |
| all tool calls (mean / median) | 3.1 / 2.5 | 5.6 / 5.0 | 2.68e-07 |
| main context tokens (mean / median) | 48,355 / 46,512 | 47,490 / 46,846 | 1 |
| all tokens, all calls (mean / median) | 179,893 / 142,217 | 236,980 / 219,434 | 3.81e-05 |
| output tokens (mean / median) | 1,266 / 1,166 | 1,280 / 1,160 | 1 |
| cache-creation tokens (mean / median) | 18,173 / 18,412 | 17,484 / 18,106 | 1 |
| notional cost (USD) (mean / median) | 0.203 / 0.199 | 0.209 / 0.205 | 0.803 |
| search share (mean / median) | 0.29 / 0.00 | 0.89 / 1.00 | 2.89e-06 |
| sessions with a search | 21/48 | 48/48 | |
| sessions that ended in success | 48/48 | 48/48 | |

Paired correct answers (48 pairs): both 44, cli only 1, mcp only 0, neither 3. Correct answers by topic total 45 (cli) and 44 (mcp) of 48.
Secondary p-values are Holm-adjusted over 10 tests; the correct-answer p is the primary test, unadjusted.

| Topic | cli correct | mcp correct |
| --- | ---: | ---: |
| K12 | 0/2 | 0/2 |
| K14 | 2/2 | 2/2 |
| K16 | 2/2 | 2/2 |
| K17 | 2/2 | 2/2 |
| K18 | 2/2 | 2/2 |
| K20 | 2/2 | 2/2 |
| K24 | 2/2 | 2/2 |
| K25 | 2/2 | 2/2 |
| K26 | 2/2 | 2/2 |
| K27 | 2/2 | 2/2 |
| K30 | 2/2 | 2/2 |
| K38 | 2/2 | 2/2 |
| K39 | 2/2 | 2/2 |
| K40 | 2/2 | 2/2 |
| K41 | 2/2 | 2/2 |
| K45 | 2/2 | 2/2 |
| K46 | 2/2 | 2/2 |
| T09 | 1/2 | 0/2 |
| T10 | 2/2 | 2/2 |
| T11 | 2/2 | 2/2 |
| T12 | 2/2 | 2/2 |
| T17 | 2/2 | 2/2 |
| T18 | 2/2 | 2/2 |
| T20 | 2/2 | 2/2 |

Run order: mean cache-creation tokens and notional cost by start position.

| Condition | started first in its topic | started 2nd-4th |
| --- | ---: | ---: |
| cli | 22,146 tokens, $0.240 (n=12) | 16,848 tokens, $0.191 (n=36) |
| mcp | 20,919 tokens, $0.239 (n=12) | 16,339 tokens, $0.199 (n=36) |

Total notional cost: $19.79 over 96 sessions.
