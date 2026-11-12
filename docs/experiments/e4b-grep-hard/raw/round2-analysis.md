Topics: 15; pairs (topic x replicate): 30; sessions: 60.

| Metric | cli | mcp | p |
| --- | ---: | ---: | ---: |
| correct | 6/30 | 9/30 | 0.508 (exact McNemar) |
| strict | 4/30 | 6/30 | 1 |
| topics with more correct answers | 3 | 5 | 1 (sign test) |
| main tool calls (mean / median) | 3.0 / 3.0 | 5.1 / 5.0 | 7.9e-05 |
| all tool calls (mean / median) | 3.0 / 3.0 | 5.1 / 5.0 | 7.9e-05 |
| main context tokens (mean / median) | 47,303 / 46,679 | 47,250 / 46,902 | 1 |
| all tokens, all calls (mean / median) | 181,118 / 174,714 | 223,328 / 217,587 | 0.0497 |
| output tokens (mean / median) | 1,211 / 1,152 | 1,132 / 1,045 | 0.794 |
| cache-creation tokens (mean / median) | 12,272 / 11,651 | 17,214 / 18,741 | 0.000693 |
| notional cost (USD) (mean / median) | 0.156 / 0.148 | 0.202 / 0.207 | 0.00365 |
| search share (mean / median) | 0.48 / 0.33 | 0.96 / 1.00 | 0.00065 |
| sessions with a search | 24/30 | 30/30 | |
| sessions that ended in success | 30/30 | 30/30 | |

Paired correct answers (30 pairs): both 3, cli only 3, mcp only 6, neither 18. Correct answers by topic total 6 (cli) and 9 (mcp) of 30.
Secondary p-values are Holm-adjusted over 10 tests; the correct-answer p is the primary test, unadjusted.

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

Run order: mean cache-creation tokens and notional cost by start position.

| Condition | started first in its topic | started 2nd-4th |
| --- | ---: | ---: |
| cli | 13,274 tokens, $0.172 (n=8) | 11,908 tokens, $0.150 (n=22) |
| mcp | 18,739 tokens, $0.200 (n=7) | 16,750 tokens, $0.202 (n=23) |

Total notional cost: $10.73 over 60 sessions.
