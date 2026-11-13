"""Mechanical count: tasks whose closing thread filed related items after task_done."""
import json, re, sys, glob, collections
from datetime import datetime, timedelta
D = sys.argv[1]
ev = [json.loads(l) for l in open(f"{D}/events.jsonl")]
tasks = {}
for f in glob.glob(f"{D}/tasks-*.json"):
    for t in json.load(open(f)):
        tasks[t["id"]] = t
ts = lambda s: datetime.fromisoformat(s.replace("Z", "+00:00"))
first_done = {}
for e in ev:
    if e["type"] == "task_done" and e["task"] not in first_done:
        first_done[e["task"]] = e
by_actor = collections.defaultdict(list)
for e in ev:
    by_actor[e["actor"]].append(e)
def mentions(tid, text):
    return re.search(rf"(?<![\w-]){re.escape(tid)}(?!\d)", text or "") is not None
rows = []
for tid, d in first_done.items():
    A, T = d["actor"], ts(d["at"])
    hits = []
    for e in by_actor[A]:
        t2 = ts(e["at"])
        if t2 <= T or t2 > T + timedelta(days=3):
            continue
        if e["type"] == "task_created":
            n = tasks.get(e["task"], {})
            rel = e["payload"].get("parent") == tid or mentions(tid, (n.get("body") or "") + " " + (n.get("title") or ""))
            if rel:
                hits.append(("task", e["task"], (t2 - T).total_seconds(), n.get("proposal")))
        elif e["type"] == "note_added" and e["task"] == tid:
            hits.append(("note-on-closed", e["task"], (t2 - T).total_seconds(), None))
        elif e["type"] == "ask_created" and e["task"] == tid:
            hits.append(("ask", e["task"], (t2 - T).total_seconds(), None))
    rows.append({"task": tid, "board": d["board"], "actor": A, "at": d["at"],
                 "resolution": (d["payload"].get("resolution") or {}).get("kind"),
                 "hits": hits})
json.dump(rows, open(f"{D}/late.json", "w"), indent=0)
agg = collections.defaultdict(lambda: collections.Counter())
for r in rows:
    a = agg[r["board"]]; a["closed"] += 1
    th = [h for h in r["hits"] if h[0] in ("task", "ask")]
    nn = [h for h in r["hits"] if h[0] == "note-on-closed"]
    if th: a["late_task_or_ask"] += 1
    if th and min(h[2] for h in th) > 600: a["late_task_gt10min"] += 1
    if th and min(h[2] for h in th) <= 600: a["late_task_le10min"] += 1
    if nn: a["late_note_on_closed"] += 1
for b, a in sorted(agg.items()):
    print(b, dict(a))
