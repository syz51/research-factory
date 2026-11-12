"""Prints the E4b session queue: `<condition> <topic> <rep>` lines.

Usage: uv run --no-project python order.py topics.txt [first] [last]

Each topic gets 2 sessions per condition. The condition that starts first
alternates per topic: topics 1, 3, 5, ... (1-based) run cli mcp mcp cli, topics
2, 4, 6, ... run mcp cli cli mcp. Run the queue 4 at a time, in order, so the
sessions of one topic run together. [first] and [last] (1-based, inclusive)
pick a batch of topics.
"""

import sys

topics = open(sys.argv[1]).read().split()
first = int(sys.argv[2]) if len(sys.argv) > 2 else 1
last = int(sys.argv[3]) if len(sys.argv) > 3 else len(topics)
for i, topic in enumerate(topics, 1):
    if not first <= i <= last:
        continue
    a, b = ("cli", "mcp") if i % 2 else ("mcp", "cli")
    for cond, rep in ((a, 1), (b, 1), (b, 2), (a, 2)):
        print(cond, topic, rep)
