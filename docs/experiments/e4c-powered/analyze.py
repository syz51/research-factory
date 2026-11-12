"""E4c analysis: cli against mcp, paired by topic and replicate, with a topic-clustered test.

Usage: uv run --no-project --with scipy --with numpy python analyze.py <measure output>
           --queue <queue.txt> [--look interim|final] [--gate gate.tsv]

<measure output> is JSON lines from ../e4b-grep-hard/measure.py. <queue.txt> is
the order.py output that the sessions ran from. A pair is the cli and mcp
session of one topic with the same replicate number. The queue sets the
analysed pairs: all P pairs of the queue at the final look, the first
floor(P/2) pairs in queue order at the interim look (README, "Interim look").
A queued pair without both sessions is reported as missing.

Primary: exact McNemar on `correct` (two-sided binomial test on the discordant
pairs) at the look's alpha: interim 0.00557, final 0.0478 (Lan-DeMets
O'Brien-Fleming spending of 0.05 over two equal looks).

Topic-clustered: each topic's mean difference d_t = (mcp correct - cli correct)
/ pairs of the topic; a sign-flip randomization test on the mean of d_t (exact
up to 20 topics, else 100,000 draws), and a 95% percentile bootstrap interval
over topics (10,000 resamples). Seed 343 for both.

Secondary, Holm-adjusted together as in E4b: exact McNemar on `strict`, a
topic-level sign test on correct counts (ties dropped), and Wilcoxon
signed-rank tests (zero differences dropped) on main tool calls, main context
tokens and notional cost. Descriptive: the per-topic table, correct answers by
which session of a pair started first, and, with --gate, results by topic kind.
"""

import argparse
import itertools
import json
from pathlib import Path
from statistics import mean

import numpy as np
from scipy.stats import binomtest, wilcoxon

ALPHA = {"interim": 0.00557, "final": 0.0478}
SEED = 343
CONDS = ("cli", "mcp")
COUNTS = (("main_tool_calls", "main tool calls", "{:.1f}"),
          ("main_context_tokens", "main context tokens", "{:,.0f}"),
          ("cost_usd", "notional cost (USD)", "{:.3f}"))


def mcnemar(a: list[bool], b: list[bool]) -> tuple[int, int, float]:
    only_a = sum(x and not y for x, y in zip(a, b))
    only_b = sum(y and not x for x, y in zip(a, b))
    d = only_a + only_b
    return only_a, only_b, binomtest(only_b, d, 0.5).pvalue if d else 1.0


def holm(ps: list[float]) -> list[float]:
    order = sorted(range(len(ps)), key=lambda i: ps[i])
    out, top = [0.0] * len(ps), 0.0
    for rank, i in enumerate(order):
        top = max(top, min(1.0, (len(ps) - rank) * ps[i]))
        out[i] = top
    return out


def sign_flip(d: np.ndarray, rng: np.random.Generator) -> float:
    obs = abs(d.mean())
    if len(d) <= 20:
        signs = np.array(list(itertools.product((1, -1), repeat=len(d))))
    else:
        signs = rng.choice((1, -1), size=(100_000, len(d)))
    return float(np.mean(np.abs((signs * d).mean(axis=1)) >= obs - 1e-12))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("rows", type=Path)
    ap.add_argument("--queue", type=Path, required=True)
    ap.add_argument("--look", choices=ALPHA, default="final")
    ap.add_argument("--gate", type=Path)
    args = ap.parse_args()
    alpha = ALPHA[args.look]

    queued: list[tuple[str, int]] = []
    for line in args.queue.read_text().splitlines():
        _, topic, rep, *_ = line.split()
        if (topic, int(rep)) not in queued:
            queued.append((topic, int(rep)))
    planned = len(queued)
    if args.look == "interim":
        queued = queued[: planned // 2]

    rows = [json.loads(line) for line in args.rows.read_text().splitlines()]
    by = {(r["condition"], r["topic"], r["rep"]): r for r in rows}
    pairs = [(t, rep) for t, rep in queued if all((c, t, rep) in by for c in CONDS)]
    missing = len(queued) - len(pairs)
    n = len(pairs)

    def col(cond: str, key: str) -> list:
        return [by[(cond, t, rep)][key] for t, rep in pairs]

    cli, mcp = col("cli", "correct"), col("mcp", "correct")
    only_cli, only_mcp, p = mcnemar(cli, mcp)
    both = sum(c and m for c, m in zip(cli, mcp))

    topics = sorted({t for t, _ in pairs})
    per_topic = {t: [[by[(c, t, rep)]["correct"] for u, rep in pairs if u == t] for c in CONDS]
                 for t in topics}
    diff = np.array([np.mean(per_topic[t][1]) - np.mean(per_topic[t][0]) for t in topics])
    rng = np.random.default_rng(SEED)
    p_cluster = sign_flip(diff, rng)
    boot = rng.choice(diff, size=(10_000, len(diff))).mean(axis=1)
    lo, hi = np.percentile(boot, [2.5, 97.5])

    secondary = {"strict": mcnemar(col("cli", "strict"), col("mcp", "strict"))[2]}
    up = sum(sum(m) > sum(c) for c, m in per_topic.values())
    down = sum(sum(m) < sum(c) for c, m in per_topic.values())
    secondary["topic sign"] = binomtest(up, up + down, 0.5).pvalue if up + down else 1.0
    for key, _, _ in COUNTS:
        diffs = [y - x for x, y in zip(col("cli", key), col("mcp", key))
                 if x is not None and y is not None]
        secondary[key] = wilcoxon(diffs).pvalue if any(diffs) else 1.0
    adj = dict(zip(secondary, holm(list(secondary.values()))))

    print(f"Look: {args.look} (alpha {alpha}). Planned pairs: {planned}; pairs in this look: "
          f"{len(queued)}; analysed: {n}; missing: {missing}. Topics: {len(topics)}.\n")
    print("| Measure | cli | mcp | p |")
    print("| --- | ---: | ---: | ---: |")
    print(f"| correct | {sum(cli)}/{n} | {sum(mcp)}/{n} | {p:.3g} (exact McNemar) |")
    print(f"| topic-clustered difference (mcp - cli) | | {diff.mean():+.3f} "
          f"[{lo:+.3f}, {hi:+.3f}] | {p_cluster:.3g} (sign-flip) |")
    print(f"| strict | {sum(col('cli', 'strict'))}/{n} | {sum(col('mcp', 'strict'))}/{n} "
          f"| {adj['strict']:.3g} (Holm) |")
    print(f"| topics with more correct answers | {down} | {up} | {adj['topic sign']:.3g} (Holm) |")
    for key, label, fmt in COUNTS:
        cells = [fmt.format(mean(v for v in col(c, key) if v is not None)) for c in CONDS]
        print(f"| {label} (mean) | {cells[0]} | {cells[1]} | {adj[key]:.3g} (Holm) |")
    print(f"\nPairs: both {both}, cli only {only_cli}, mcp only {only_mcp}, "
          f"neither {n - both - only_cli - only_mcp}.")

    primary = p < alpha
    agrees = p_cluster < 0.05 and np.sign(diff.mean()) == np.sign(only_mcp - only_cli) != 0
    robust = primary and agrees
    print(f"Primary test significant at {alpha}: {'yes' if primary else 'no'}. "
          f"Clustered test agrees (p<0.05, same direction): {'yes' if agrees else 'no'}.")
    if args.look == "interim":
        if robust:
            print("Interim decision: stop for efficacy.")
        elif only_mcp <= only_cli:
            print("Interim decision: stop for futility (mcp only <= cli only).")
        else:
            print("Interim decision: continue.")

    print("\n| Topic | cli correct | mcp correct |")
    print("| --- | ---: | ---: |")
    for t in topics:
        c, m = per_topic[t]
        print(f"| {t} | {sum(c)}/{len(c)} | {sum(m)}/{len(m)} |")

    print("\nCorrect answers by which session of the pair started first (order.log position).\n")
    print("| Condition | started first in its pair | started second |")
    print("| --- | ---: | ---: |")
    for c in CONDS:
        cells = []
        for first in (True, False):
            hits = [by[(c, t, rep)]["correct"] for t, rep in pairs
                    if None not in (by[("cli", t, rep)]["position"], by[("mcp", t, rep)]["position"])
                    and (by[(c, t, rep)]["position"]
                         < by[("mcp" if c == "cli" else "cli", t, rep)]["position"]) == first]
            cells.append(f"{sum(hits)}/{len(hits)}")
        print(f"| {c} | {cells[0]} | {cells[1]} |")

    if args.gate:
        kind = {}
        for line in args.gate.read_text().splitlines()[1:]:
            f = line.split()
            kind[f[0]] = f[6]
        print("\n| Kind | topics | cli correct | mcp correct |")
        print("| --- | ---: | ---: | ---: |")
        for k in sorted({kind.get(t, "-") for t in topics}):
            ts = [t for t in topics if kind.get(t, "-") == k]
            c = sum(sum(per_topic[t][0]) for t in ts)
            m = sum(sum(per_topic[t][1]) for t in ts)
            tot = sum(len(per_topic[t][0]) for t in ts)
            print(f"| {k} | {len(ts)} | {c}/{tot} | {m}/{tot} |")


if __name__ == "__main__":
    main()
