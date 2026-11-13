import json, glob, collections, sys
S = sys.argv[1]; CUT = "2026-11-12T03:09"
J = {}
for f in sorted(glob.glob(f"{S}/judge/out-*.jsonl")) + sorted(glob.glob(f"{S}/judge/rout-*.jsonl")):
    for l in open(f): o = json.loads(l); J[o["case"]] = o
s = json.load(open(f"{S}/data/sample.json")); late = {r["task"]: r for r in json.load(open(f"{S}/data/late.json"))}
det = collections.defaultdict(dict)
for l in open(f"{S}/data/detect.jsonl"):
    r = json.loads(l); det[r["task"]][r["turn"]] = (r["n_items"], len(r["unfiled"]))
BAD = {"late-hook", "late-prompted", "never"}
# turn-level: (task, turn) with judged BAD item
c = collections.Counter(); wc = collections.Counter()
for t in s["A"]:
    o = J[t]; p = "post" if late[t]["at"] >= CUT else "pre"
    turns = collections.defaultdict(list)
    for i in o["items"]:
        if i["status"] in BAD: turns[i.get("turn") or 0].append(i["where"])
    for tu, ws in turns.items():
        d = det[t].get(tu)
        flag = bool(d and d[1]); c[(p, "badturn")] += 1; c[(p, "flagged")] += flag
        wc[("heading" in ws, flag)] += 1
    # false flags: detector flagged a turn with no judged BAD item
    for tu, d in det[t].items():
        if d[1] and tu not in turns: c[(p, "flag_without_bad")] += 1
        if d[1]: c[(p, "flag_turns")] += 1
print(dict(c)); print("has heading item, flagged:", dict(wc))
# post period unfiled items by where/turn
print(collections.Counter((i["where"], (i.get("turn") or 0) > 0) for t in s["A"] if late[t]["at"] >= CUT for i in J[t]["items"] if i["status"] in BAD))
print("last done", max(r["at"] for r in late.values()))
