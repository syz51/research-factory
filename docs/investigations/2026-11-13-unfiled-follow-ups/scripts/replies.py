"""Extract the closing reply and the later-turn replies of each closed task."""
import json, re, sys, collections
from datetime import datetime, timedelta
D = sys.argv[1]
ts = lambda s: datetime.fromisoformat(s.replace("Z", "+00:00"))
late = {r["task"]: r for r in json.load(open(f"{D}/late.json"))}
txs = [json.loads(l) for l in open(f"{D}/tx.jsonl")]
def done_id(d):
    tid = d["input"].get("id") or ""
    if not tid:
        m = re.search(r"board done ([a-z0-9-]+-\d+)", d["input"].get("command", "")); tid = m.group(1) if m else ""
    return tid
idx = collections.defaultdict(list)
for fi, o in enumerate(txs):
    for i, (k, d, t) in enumerate(o["seq"]):
        if k == "res-done" and done_id(d): idx[done_id(d)].append((fi, i, t))
out = open(f"{D}/replies.jsonl", "w")
n = 0
for tid, r in late.items():
    if r["board"] == "board-feedback": continue
    T = ts(r["at"])
    c = [x for x in idx.get(tid, []) if x[2] and abs((ts(x[2]) - T).total_seconds()) < 900]
    if not c: continue
    fi, i, t0 = min(c, key=lambda x: abs((ts(x[2]) - T).total_seconds()))
    seq = txs[fi]["seq"]
    turn, phase, last = 0, "close", None
    replies = []
    for k, d, t in seq[i + 1:]:
        if k == "res-done" and done_id(d) != tid and turn > 0: break
        if t and ts(t) > ts(t0) + timedelta(hours=24): break
        if k in ("user", "hook", "compact"):
            if last: replies.append((turn, last[0], last[1]))
            last = None; turn += 1
            continue
        if k == "text" and d.strip(): last = (d, k)
    if last: replies.append((turn, last[0], last[1]))
    for turn, text, _ in replies:
        out.write(json.dumps({"task": tid, "board": r["board"], "done_at": r["at"], "file": txs[fi]["file"], "turn": turn, "reply": text}) + "\n")
    n += 1
print(n)
