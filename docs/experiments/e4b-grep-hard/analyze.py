"""Compares cli and mcp in E4b, paired by topic and replicate.

Usage: uv run --no-project --with scipy python analyze.py <measure output (JSON lines)>

A pair is the cli and mcp session of one topic with the same replicate number
(1 or 2). Primary test: exact McNemar on `correct` (a two-sided binomial test
on the discordant pairs), unadjusted. Secondary tests, Holm-adjusted together:
exact McNemar on `strict`, a topic-level sign test on correct answers (each
topic's correct count out of 2 per condition; ties dropped), and Wilcoxon
signed-rank tests (zero differences dropped) on calls, tokens and cost.
Prints Markdown tables.
"""

import json
import sys
from pathlib import Path
from statistics import mean, median

from scipy.stats import binomtest, wilcoxon

CONDS = ["cli", "mcp"]
COUNTS = [
    ("main_tool_calls", "main tool calls", "{:.1f}"),
    ("all_tool_calls", "all tool calls", "{:.1f}"),
    ("main_context_tokens", "main context tokens", "{:,.0f}"),
    ("total_tokens", "all tokens, all calls", "{:,.0f}"),
    ("output_tokens", "output tokens", "{:,.0f}"),
    ("cache_creation_input_tokens", "cache-creation tokens", "{:,.0f}"),
    ("cost_usd", "notional cost (USD)", "{:.3f}"),
    ("search_share", "search share", "{:.2f}"),
]


def holm(ps: list[float]) -> list[float]:
    order = sorted(range(len(ps)), key=lambda i: ps[i])
    out, top = [0.0] * len(ps), 0.0
    for rank, i in enumerate(order):
        top = max(top, min(1.0, (len(ps) - rank) * ps[i]))
        out[i] = top
    return out


def mcnemar(a: list[bool], b: list[bool]) -> tuple[int, int, float]:
    only_a = sum(x and not y for x, y in zip(a, b))
    only_b = sum(y and not x for x, y in zip(a, b))
    d = only_a + only_b
    return only_a, only_b, binomtest(only_b, d, 0.5).pvalue if d else 1.0


def main() -> None:
    rows = [json.loads(line) for line in Path(sys.argv[1]).read_text().splitlines()]
    for r in rows:
        r["total_tokens"] = sum(
            r[k] for k in ("input_tokens", "output_tokens", "cache_read_input_tokens",
                           "cache_creation_input_tokens"))
    by = {(r["condition"], r["topic"], r["rep"]): r for r in rows}
    pairs = sorted({(t, rep) for _, t, rep in by if all((c, t, rep) in by for c in CONDS)})
    topics = sorted({t for t, _ in pairs})
    n = len(pairs)

    def col(cond: str, key: str) -> list:
        return [by[(cond, t, rep)][key] for t, rep in pairs]

    cli_c, mcp_c = col("cli", "correct"), col("mcp", "correct")
    only_cli, only_mcp, p_primary = mcnemar(cli_c, mcp_c)
    both = sum(x and y for x, y in zip(cli_c, mcp_c))
    neither = n - both - only_cli - only_mcp

    secondary: dict[str, float] = {}
    secondary["strict"] = mcnemar(col("cli", "strict"), col("mcp", "strict"))[2]
    per_topic = {
        t: [sum(by[(c, t, rep)]["correct"] for rep in (1, 2) if (c, t, rep) in by) for c in CONDS]
        for t in topics
    }
    up = sum(m > c for c, m in per_topic.values())
    down = sum(m < c for c, m in per_topic.values())
    secondary["topic sign"] = binomtest(up, up + down, 0.5).pvalue if up + down else 1.0
    for key, _, _ in COUNTS:
        diffs = [y - x for x, y in zip(col("cli", key), col("mcp", key))
                 if x is not None and y is not None]
        secondary[key] = wilcoxon(diffs).pvalue if any(diffs) else 1.0
    adj = dict(zip(secondary, holm(list(secondary.values()))))

    print(f"Topics: {len(topics)}; pairs (topic x replicate): {n}; sessions: {2 * n}.\n")
    print("| Metric | cli | mcp | p |")
    print("| --- | ---: | ---: | ---: |")
    print(f"| correct | {sum(cli_c)}/{n} | {sum(mcp_c)}/{n} | {p_primary:.3g} (exact McNemar) |")
    print(f"| strict | {sum(col('cli', 'strict'))}/{n} | {sum(col('mcp', 'strict'))}/{n} "
          f"| {adj['strict']:.3g} |")
    tc = [sum(v[0] for v in per_topic.values()), sum(v[1] for v in per_topic.values())]
    print(f"| topics with more correct answers | {down} | {up} | {adj['topic sign']:.3g} (sign test) |")
    for key, label, fmt in COUNTS:
        cells = []
        for c in CONDS:
            vals = [v for v in col(c, key) if v is not None]
            cells.append(f"{fmt.format(mean(vals))} / {fmt.format(median(vals))}" if vals else "-")
        print(f"| {label} (mean / median) | {cells[0]} | {cells[1]} | {adj[key]:.3g} |")
    print("| sessions with a search | " + " | ".join(
        f"{sum(1 for v in col(c, 'searches') if v)}/{n}" for c in CONDS) + " | |")
    print("| sessions that ended in success | " + " | ".join(
        f"{sum(col(c, 'ok'))}/{n}" for c in CONDS) + " | |")

    print(f"\nPaired correct answers ({n} pairs): both {both}, cli only {only_cli}, "
          f"mcp only {only_mcp}, neither {neither}. Correct answers by topic total "
          f"{tc[0]} (cli) and {tc[1]} (mcp) of {2 * len(topics)}.")
    print("Secondary p-values are Holm-adjusted over "
          f"{len(secondary)} tests; the correct-answer p is the primary test, unadjusted.\n")

    print("| Topic | cli correct | mcp correct |")
    print("| --- | ---: | ---: |")
    for t in topics:
        c, m = per_topic[t]
        print(f"| {t} | {c}/2 | {m}/2 |")

    print("\nRun order: mean cache-creation tokens and notional cost by start position.\n")
    print("| Condition | started first in its topic | started 2nd-4th |")
    print("| --- | ---: | ---: |")
    for c in CONDS:
        cells = []
        for first in (True, False):
            rs = [r for r in rows if r["condition"] == c and (r["position"] == 1) == first]
            cells.append(
                f"{mean(r['cache_creation_input_tokens'] for r in rs):,.0f} tokens, "
                f"${mean(r['cost_usd'] or 0 for r in rs):.3f} (n={len(rs)})" if rs else "-")
        print(f"| {c} | {cells[0]} | {cells[1]} |")
    total = sum(r["cost_usd"] or 0 for r in rows)
    print(f"\nTotal notional cost: ${total:.2f} over {len(rows)} sessions.")


if __name__ == "__main__":
    main()
