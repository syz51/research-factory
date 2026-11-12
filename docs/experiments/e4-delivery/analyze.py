"""Compares the E4 delivery conditions with paired tests against `cli`.

Usage: uv run --no-project --with scipy python analyze.py <measure output (JSON lines)>

Each topic is one pair (cli vs the other condition). Correct and strict answers use the
exact McNemar test (a two-sided binomial test on the discordant pairs). Tool
calls, main-agent context tokens, cost and search share use the Wilcoxon
signed-rank test (zero differences dropped, "wilcox"). Holm corrects the
p-values over all the tests of the table. Prints a Markdown table.
"""

import json
import sys
from statistics import mean, median

from scipy.stats import binomtest, wilcoxon

CONDITIONS = ["cli", "mcp", "hint", "agent"]
COUNTS = [
    ("main_tool_calls", "main tool calls", "{:.1f}"),
    ("all_tool_calls", "all tool calls", "{:.1f}"),
    ("main_context_tokens", "main context tokens", "{:,.0f}"),
    ("cost_usd", "cost (USD)", "{:.3f}"),
    ("search_share", "search share", "{:.2f}"),
]


def holm(ps: list[float]) -> list[float]:
    order = sorted(range(len(ps)), key=lambda i: ps[i])
    out, top = [0.0] * len(ps), 0.0
    for rank, i in enumerate(order):
        top = max(top, min(1.0, (len(ps) - rank) * ps[i]))
        out[i] = top
    return out


def main() -> None:
    rows = [json.loads(line) for line in open(sys.argv[1])]
    by = {(r["condition"], r["topic"]): r for r in rows}
    topics = sorted({r["topic"] for r in rows if all((c, r["topic"]) in by for c in CONDITIONS)})
    n = len(topics)

    def col(cond: str, key: str) -> list:
        return [by[(cond, t)][key] for t in topics]

    tests: list[tuple[str, str, float]] = []  # (metric, condition, raw p)
    for cond in CONDITIONS[1:]:
        for key in ("correct", "strict"):
            a, b = col("cli", key), col(cond, key)
            only_cli = sum(x and not y for x, y in zip(a, b))
            only_other = sum(y and not x for x, y in zip(a, b))
            d = only_cli + only_other
            p = binomtest(only_other, d, 0.5).pvalue if d else 1.0
            tests.append((key, cond, p))
        for key, _, _ in COUNTS:
            pairs = [
                (x, y)
                for x, y in zip(col("cli", key), col(cond, key))
                if x is not None and y is not None
            ]
            diffs = [y - x for x, y in pairs]
            p = wilcoxon(diffs).pvalue if any(diffs) else 1.0
            tests.append((key, cond, p))
    adjusted = dict(zip([(m, c) for m, c, _ in tests], holm([p for _, _, p in tests])))

    def cell(metric: str, cond: str, text: str) -> str:
        if cond == "cli":
            return text
        return f"{text} (p={adjusted[(metric, cond)]:.3g})"

    print(f"Topics: {n}; sessions: {n * len(CONDITIONS)}.\n")
    print("| Metric | " + " | ".join(CONDITIONS) + " |")
    print("| --- |" + " ---: |" * len(CONDITIONS))
    for key in ("correct", "strict"):
        line = [cell(key, c, f"{sum(col(c, key))}/{n}") for c in CONDITIONS]
        print(f"| {key} | " + " | ".join(line) + " |")
    for key, label, fmt in COUNTS:
        line = []
        for c in CONDITIONS:
            vals = [v for v in col(c, key) if v is not None]
            text = f"{fmt.format(mean(vals))} / {fmt.format(median(vals))}" if vals else "-"
            line.append(cell(key, c, text))
        print(f"| {label} (mean / median) | " + " | ".join(line) + " |")
    line = []
    for c in CONDITIONS:
        line.append(f"{sum(1 for v in col(c, 'searches') if v)}/{n}")
    print("| sessions with a search | " + " | ".join(line) + " |")
    line = [f"{sum(col(c, 'archivist'))}" for c in CONDITIONS]
    print("| archivist calls | " + " | ".join(line) + " |")
    total = sum(r["cost_usd"] or 0 for r in rows)
    print(f"\nTotal cost: ${total:.2f}. p-values: Holm-adjusted over {len(tests)} tests.")


if __name__ == "__main__":
    main()
