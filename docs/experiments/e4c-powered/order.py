"""Prints the E4c comparison queue: `<condition> <topic> <rep> <phrasing>` lines.

Usage: uv run --no-project python order.py selected.txt <r>

selected.txt has one `<topic> <phrasing>` line per selected topic (README,
"Screening"). The topics are sorted by id and shuffled with random.Random(343).
Topic i (1-based, shuffled order) gets r replicates; replicate k runs cli first
when i + k is even, else mcp first. So odd topics start with cli, the start
alternates per replicate, and each condition starts first in half of the pairs
(within one pair when r * topics is odd). Run the queue 4 at a time, in order,
with ../e4b-grep-hard/run_session.sh.
"""

import random
import sys
from pathlib import Path

topics = sorted(line.split() for line in Path(sys.argv[1]).read_text().splitlines() if line.strip())
r = int(sys.argv[2])
random.Random(343).shuffle(topics)
for i, (topic, phrasing) in enumerate(topics, 1):
    for k in range(1, r + 1):
        first, second = ("cli", "mcp") if (i + k) % 2 == 0 else ("mcp", "cli")
        print(first, topic, k, phrasing)
        print(second, topic, k, phrasing)
