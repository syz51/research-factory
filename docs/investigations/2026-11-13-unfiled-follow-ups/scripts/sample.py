"""Pick the judged sample. Seeded, so the record can state how to rerun it."""
import json, random, collections, sys
D = sys.argv[1]; CUT = "2026-11-12T03:09"
random.seed(242)
det = [json.loads(l) for l in open(f"{D}/detect.jsonl")]
cls = {r["task"]: r for r in json.load(open(f"{D}/classified.json"))}
tasks = {}
for r in det:
    tasks.setdefault(r["task"], {"board": r["board"], "done_at": r["done_at"], "flag": False})
    if r["unfiled"]: tasks[r["task"]]["flag"] = True
groups = collections.defaultdict(list)
for t, v in tasks.items():
    groups[(v["board"], "post" if v["done_at"] >= CUT else "pre")].append(t)
N = {"pre": 70, "post": 35}
tot = {p: sum(len(v) for (b, q), v in groups.items() if q == p) for p in N}
A = set()
for (b, p), v in sorted(groups.items()):
    k = max(1, round(N[p] * len(v) / tot[p])) if len(v) >= 1 else 0
    A.update(random.sample(sorted(v), min(k, len(v))))
B = set()
for t, r in cls.items():
    if not r.get("late_ids") or not r.get("found"): continue
    kinds = {k for _, k in r.get("trig", [])}
    if "user" in kinds or not kinds: B.add(t)
Dset = {t for t, v in tasks.items() if v["flag"]}
cases = sorted(A | B | Dset)
json.dump({"A": sorted(A), "B": sorted(B), "D": sorted(Dset), "all": cases}, open(f"{D}/sample.json", "w"))
print(len(A), len(B), len(Dset), len(cases))
print(collections.Counter((tasks[t]["board"], "post" if tasks[t]["done_at"] >= CUT else "pre") for t in A))
