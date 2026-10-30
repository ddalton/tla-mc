#!/usr/bin/env python3
"""Follow the runner's results.jsonl and print one line per finished 0.1.2/0.2.0 pair
worth seeing (interpreted pairs of 5 s or more, every compiled pair, every mismatch),
TLC counts that differ from 0.2.0's, Examples results that differ from the manifest,
and the phase lines. Resumes after the lines it has already reported (report.state)."""
import json, math, os, sys, time
OUT = sys.argv[1] if len(sys.argv) > 1 else "/data/out"
STATE = f"{OUT}/report.state"
start = int(open(STATE).read()) if os.path.exists(STATE) else 0
pstart = int(open(STATE + ".p").read()) if os.path.exists(STATE + ".p") else 0
pairs, interp020, tlc, ex = {}, {}, {}, []
n = pn = 0


def fmt(x):
    return f"{x:,}" if isinstance(x, int) else str(x)


def pair_line(kind, k, a, b):
    """a: 0.1.2, b: 0.2.0"""
    kind_g = b.get("gate_kind") or a.get("gate_kind")
    exact = kind_g in ("strict_run", "liveness_mutation_run")
    same_v = a["status"] == b["status"]
    same_d = a.get("distinct") == b.get("distinct")
    flag = []
    if not same_v or b["status"] != "agree":
        flag.append(f"VERDICT 0.1.2 {a['status']} 0.2.0 {b['status']}")
    if exact and a["status"] == b["status"] == "agree" and not same_d:
        flag.append(f"COUNT 0.1.2 {fmt(a.get('distinct'))} 0.2.0 {fmt(b.get('distinct'))}")
    if kind == "interp" and max(a["secs"], b["secs"]) < 5 and not flag:
        return None
    def t(r):
        if r["status"] == "timeout":
            return f">{r['secs']:.0f}s (cap; {fmt(r.get('states_seen'))} states, {r.get('levels')} levels)"
        return f"{r['secs']:.1f}s"
    sp = f" ({a['secs'] / b['secs']:.1f}x)" if a["status"] != "timeout" and b["status"] != "timeout" and b["secs"] > 0 else ""
    cnt = fmt(b.get("distinct")) + " distinct" if b.get("distinct") is not None else ""
    if exact and same_d and b.get("distinct") is not None:
        cnt += ", same"
    who = k[1].replace(".cfg", "") + ("" if k[0] == "formal" else " [lean]")
    tag = {"strict_run": "holds", "mutation_run": "bug", "liveness_mutation_run": "live bug"}.get(kind_g, "")
    return (f"{kind} {who} ({tag}): 0.1.2 {t(a)} -> 0.2.0 {t(b)}{sp}; {cnt}; RSS {a['rss_gb']:.1f} -> {b['rss_gb']:.1f} GB"
            + ("  !! " + "; ".join(flag) if flag else ""))


def summary():
    out = []
    for kind in ("interp", "compiled"):
        r = [(a["secs"], b["secs"]) for (kd, *_), v in pairs.items() if kd == kind and len(v) == 2
             for a, b in [(v["0.1.2"], v["0.2.0"])] if a["status"] == b["status"] == "agree" and a["secs"] >= 1 and b["secs"] > 0]
        if r:
            g = math.exp(sum(math.log(x / y) for x, y in r) / len(r))
            out.append(f"{kind}: {len(r)} pairs of 1 s or more, geometric-mean speedup {g:.2f}x, total {sum(x for x, _ in r):.0f}s -> {sum(y for _, y in r):.0f}s")
    acc = sum(1 for e in ex if e.get("accepted"))
    out.append(f"examples so far: {len(ex)} run, {acc} as the manifest records")
    return out


while True:
    p = f"{OUT}/results.jsonl"
    txt = open(p).read() if os.path.exists(p) else ""
    lines = txt.split("\n")[:-1]
    for i in range(n, len(lines)):
        r = json.loads(lines[i])
        emit = i >= start
        kd, v = r.get("kind"), r.get("version")
        line = None
        if kd in ("interp", "compiled"):
            k = (kd, r["dir"], r["cfg"])
            pairs.setdefault(k, {})[v] = r
            if kd == "interp" and v == "0.2.0":
                interp020[(r["dir"], r["cfg"])] = r
            if len(pairs[k]) == 2:
                line = pair_line(kd, k[1:], pairs[k]["0.1.2"], pairs[k]["0.2.0"])
        elif kd == "tlc":
            b = interp020.get((r["dir"], r["cfg"]))
            if r["status"] != "done":
                line = f"tlc {r['cfg']}: {r['status']} at {r['secs']:.0f}s"
            elif b and r.get("violated") is None and b.get("violated") is None and r.get("distinct") != b.get("distinct"):
                line = f"tlc {r['cfg']}: TLC {fmt(r.get('distinct'))} distinct, 0.2.0 {fmt(b.get('distinct'))}  !! COUNT"
        elif kd == "examples":
            ex.append(r)
            if not r.get("accepted") and r.get("got") in ("success", "safety failure", "liveness failure", "action failure"):
                line = f"examples {r['model']}: want {r['want']} {fmt(r.get('want_distinct'))}, got {r['got']} {fmt(r.get('distinct'))}  !! RESULT"
        elif kd == "build" and r.get("status") != "built":
            line = f"build {v} {r.get('cfg')}: FAILED {r.get('err')}"
        elif r.get("status") == "harness":
            line = f"harness error {r.get('id')}: {r.get('err')}"
        if line and emit:
            print(line, flush=True)
    n = len(lines)
    open(STATE, "w").write(str(n))
    pl = open(f"{OUT}/progress.log").read().split("\n")[:-1] if os.path.exists(f"{OUT}/progress.log") else []
    for i in range(pn, len(pl)):
        l = pl[i]
        if i >= pstart and (" phase " in l or "joins the big" in l or l.endswith("DONE")):
            print("== " + l, flush=True)
            if "phase 1 done" in l or l.endswith("DONE"):
                for s in summary():
                    print("== " + s, flush=True)
    pn = len(pl)
    open(STATE + ".p", "w").write(str(pn))
    if os.path.exists(f"{OUT}/DONE"):
        break
    time.sleep(3)
