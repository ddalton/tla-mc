#!/usr/bin/env python3
"""Compare a flint gate sweep (formal/tla-mc-gate/sweep.py output) with the
baseline in gate/: every entry must give the same verdict, and a strict or
liveness run the same distinct count. A mutation run stops at its first
violation, so its count moves with the workers' interleaving and is not
compared. Exits 1 on any difference.

  flint-gate-compare.py BASELINE.jsonl NEW.jsonl [ONLY]   ONLY: a file of cfg names to compare
"""
import json, sys

base = {json.loads(l)["cfg"]: json.loads(l) for l in open(sys.argv[1])}
new = {json.loads(l)["cfg"]: json.loads(l) for l in open(sys.argv[2])}
if len(sys.argv) > 3:
    only = set(open(sys.argv[3]).read().split())
    base = {k: v for k, v in base.items() if k in only}
bad = []
for cfg, b in base.items():
    n = new.get(cfg)
    if n is None:
        bad.append(f"{cfg}: not run")
        continue
    if n["status"] != b["status"]:
        bad.append(f"{cfg}: {b['status']} -> {n['status']} {n.get('err') or ''}".rstrip())
    elif b["kind"] != "mutation_run" and n["distinct"] != b["distinct"]:
        bad.append(f"{cfg}: {b['distinct']} -> {n['distinct']} distinct states")
exact = sum(b["kind"] != "mutation_run" for b in base.values())
print(f"{len(base) - len(bad)} of {len(base)} as the baseline ({exact} with exact counts)" + (f"; {len(bad)} DIFFER:" if bad else ""))
print("\n".join("  " + x for x in bad))
sys.exit(1 if bad else 0)
