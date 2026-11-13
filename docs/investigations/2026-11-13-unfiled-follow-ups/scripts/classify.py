"""Join late filings (board events) with closing transcripts; classify what prompted each."""
import json, re, sys, collections
from datetime import datetime
D = sys.argv[1]
late = {r["task"]: r for r in json.load(open(f"{D}/late.json"))}
ts = lambda s: datetime.fromisoformat(s.replace("Z", "+00:00"))
done_at = collections.defaultdict(list)  # task -> [(file, index)]
txs = []
for line in open(f"{D}/tx.jsonl"):
    o = json.loads(line); txs.append(o)
    for i, (k, d, t) in enumerate(o["seq"]):
        if k == "res-done":
            inp = d["input"]; tid = inp.get("id") or ""
            if not tid:
                m = re.search(r"board done ([a-z0-9-]+-\d+)", inp.get("command", "")); tid = m.group(1) if m else ""
            if tid and not re.search(r"error|refused|not found|cannot", d["result"][:120], re.I):
                done_at[tid].append((len(txs) - 1, i, t))
res = []
for tid, r in late.items():
    if r["board"] == "board-feedback": continue
    cands = done_at.get(tid, [])
    T = ts(r["at"])
    best = min(cands, key=lambda c: abs((ts(c[2]) - T).total_seconds()) if c[2] else 1e12, default=None)
    row = {"task": tid, "board": r["board"], "at": r["at"], "found": best is not None and best[2] and abs((ts(best[2]) - T).total_seconds()) < 900}
    hits = [h for h in r["hits"] if h[0] in ("task", "ask")]
    row["late_ids"] = [h[1] for h in hits]
    if row["found"]:
        o = txs[best[0]]; seq = o["seq"]; row["file"] = o["file"]
        # walk after done: label each filing of a late id with the strongest prompt kind seen since the close
        # (owner or parent prompt > Stop hook > compaction > none); the walk has no time or next-close limit
        trig = []
        prompts = []
        for k, d, t in seq[best[1] + 1:]:
            if k in ("user", "hook", "compact"):
                prompts.append(k)
            if k in ("res-add", "res-ask"):
                for lid in row["late_ids"]:
                    if re.search(rf"{re.escape(lid)}(?!\d)", d["result"]):
                        trig.append((lid, "user" if "user" in prompts else ("hook" if "hook" in prompts else ("compact" if "compact" in prompts else "same-turn"))))
        row["trig"] = trig
        # final reply of the closing turn
        fin = ""
        nxt_user = None
        for j in range(best[1] + 1, len(seq)):
            k, d, t = seq[j]
            if k == "user": nxt_user = j; break
            if k == "text": fin = d
        row["final_len"] = len(fin)
        row["user_after"] = nxt_user is not None
    res.append(row)
json.dump(res, open(f"{D}/classified.json", "w"), indent=0)
c = collections.Counter(); per = collections.defaultdict(collections.Counter)
for r in res:
    if not r["late_ids"]:
        continue
    b = r["board"]; per[b]["late"] += 1
    if not r["found"]: per[b]["no_transcript"] += 1; continue
    kinds = {k for _, k in r.get("trig", [])}
    first = "user" if "user" in kinds else ("hook" if "hook" in kinds else ("same-turn" if "same-turn" in kinds else ("compact" if "compact" in kinds else "unmatched")))
    per[b][first] += 1
for b, a in sorted(per.items()): print(b, dict(a))
print("closed tasks with transcript found:", sum(1 for r in res if r["found"]), "of", len(res))
