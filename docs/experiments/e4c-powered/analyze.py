"""E4c analysis: cli against mcp, paired by topic and replicate, with a topic-clustered test.

Usage: uv run --no-project --with scipy --with numpy python analyze.py <measure output> [interim|final]

<measure output> is JSON lines from ../e4b-grep-hard/measure.py. A pair is the
cli and mcp session of one topic with the same replicate number. The look
(default final) sets the alpha of the primary test (README, "Interim look"):
interim 0.00557, final 0.0478, two-sided, Lan-DeMets O'Brien-Fleming spending of
0.05 over two equal looks.

Primary: exact McNemar on `correct` (two-sided binomial test on the discordant
pairs). Topic-clustered: each topic's mean difference d_t = (mcp correct - cli
correct) / pairs of the topic; a sign-flip randomization test on the mean of
d_t (exact up to 20 topics, else 100,000 draws), and a 95% percentile bootstrap
interval over topics (10,000 resamples) for the difference in correct rates.
Seed 343 for both. Secondary tests: ../e4b-grep-hard/analyze.py on the same rows.
"""

import itertools
import json
import sys
from pathlib import Path

import numpy as np
from scipy.stats import binomtest

ALPHA = {"interim": 0.00557, "final": 0.0478}
SEED = 343


def sign_flip(d: np.ndarray, rng: np.random.Generator) -> float:
    obs = abs(d.mean())
    if len(d) <= 20:
        signs = np.array(list(itertools.product((1, -1), repeat=len(d))))
    else:
        signs = rng.choice((1, -1), size=(100_000, len(d)))
    return float(np.mean(np.abs((signs * d).mean(axis=1)) >= obs - 1e-12))


def main() -> None:
    look = sys.argv[2] if len(sys.argv) > 2 else "final"
    alpha = ALPHA[look]
    rows = [json.loads(line) for line in Path(sys.argv[1]).read_text().splitlines()]
    by = {(r["condition"], r["topic"], r["rep"]): r["correct"] for r in rows}
    pairs = sorted({(t, rep) for _, t, rep in by if ("cli", t, rep) in by and ("mcp", t, rep) in by})
    cli = [by[("cli", t, rep)] for t, rep in pairs]
    mcp = [by[("mcp", t, rep)] for t, rep in pairs]
    only_cli = sum(c and not m for c, m in zip(cli, mcp))
    only_mcp = sum(m and not c for c, m in zip(cli, mcp))
    both = sum(c and m for c, m in zip(cli, mcp))
    d = only_cli + only_mcp
    p = binomtest(only_mcp, d, 0.5).pvalue if d else 1.0

    topics = sorted({t for t, _ in pairs})
    diff = np.array([
        np.mean([by[("mcp", t, rep)] - by[("cli", t, rep)] for u, rep in pairs if u == t])
        for t in topics
    ])
    rng = np.random.default_rng(SEED)
    p_cluster = sign_flip(diff, rng)
    boot = rng.choice(diff, size=(10_000, len(diff))).mean(axis=1)
    lo, hi = np.percentile(boot, [2.5, 97.5])

    n = len(pairs)
    print(f"Look: {look} (alpha {alpha}). Topics: {len(topics)}; pairs: {n}.\n")
    print("| Measure | cli | mcp | p |")
    print("| --- | ---: | ---: | ---: |")
    print(f"| correct | {sum(cli)}/{n} | {sum(mcp)}/{n} | {p:.3g} (exact McNemar) |")
    print(f"| topic-clustered difference (mcp - cli) | | {diff.mean():+.3f} "
          f"[{lo:+.3f}, {hi:+.3f}] | {p_cluster:.3g} (sign-flip) |")
    print(f"\nPairs: both {both}, cli only {only_cli}, mcp only {only_mcp}, "
          f"neither {n - both - d}.")
    primary = p < alpha
    agrees = p_cluster < 0.05 and np.sign(diff.mean()) == np.sign(only_mcp - only_cli) != 0
    robust = primary and agrees
    print(f"Primary test significant at {alpha}: {'yes' if primary else 'no'}. "
          f"Clustered test agrees (p<0.05, same direction): {'yes' if agrees else 'no'}.")
    if look == "interim":
        if primary and robust:
            print("Interim decision: stop for efficacy.")
        elif only_mcp <= only_cli:
            print("Interim decision: stop for futility (mcp only <= cli only).")
        else:
            print("Interim decision: continue.")


if __name__ == "__main__":
    main()
