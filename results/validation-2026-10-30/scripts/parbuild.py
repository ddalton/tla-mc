#!/usr/bin/env python3
"""Build the big entries' compiled checkers the first runner has not built yet, N at a
time, each in its own cache (its own cargo target dir: one shared target dir builds one
crate at a time), and put each binary in the cache phase 2 reads (same name: the key is
the spec, the cfg and the tla-mc binary, not the cache's path)."""
import json, os, shutil, signal, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor
N = int(sys.argv[1]) if len(sys.argv) > 1 else 16
BIN = {"0.2.0": "/data/v020/bin/tla-mc", "0.1.2": "/data/v012/bin/tla-mc"}
big = [r for r in map(json.loads, open("/data/gate-sweep.jsonl")) if r["secs"] >= 60]
built = {(r["version"], r["dir"], r["cfg"]) for r in map(json.loads, open("/data/out/results.jsonl"))
         if r.get("kind") == "build" and r.get("status") == "built"}
todo = [(v, r) for r in big for v in BIN if (v, r["dir"], r["cfg"]) not in built]
print(f"{len(big)} big entries, {len(built)} built, {len(todo)} to build, {N} at a time", flush=True)

def one(vr):
    v, r = vr
    tag = f"{v}-{r['cfg']}"
    cache = f"/data/work/pb/{tag}"
    log = f"/data/out/logs/parbuild-{tag}.log"
    t0 = time.time()
    with open(log, "w") as lf:
        p = subprocess.Popen([BIN[v], "-compile", "-workers", "1", "-metadir", f"/data/work/pbmeta/{tag}", "-config", r["cfg"], r["module"] + ".tla"],
                             cwd=f"/data/flint/{r['dir']}", stdout=lf, stderr=subprocess.STDOUT, start_new_session=True,
                             env=dict(os.environ, TLAMC_CACHE=cache, TLAMC_STOP_AFTER_SECS="0"))
        while p.poll() is None:
            if "tla-mc: built in" in open(log).read():
                os.killpg(p.pid, signal.SIGKILL); p.wait(); break
            if time.time() - t0 > 1800:
                os.killpg(p.pid, signal.SIGKILL); p.wait(); break
            time.sleep(1)
    out = open(log).read()
    n = 0
    for b in os.listdir(f"{cache}/bin") if os.path.isdir(f"{cache}/bin") else []:
        dst = f"/data/work/cache-{v}/bin/{b}"
        if not os.path.exists(dst) and ".tmp" not in b:
            shutil.copy(f"{cache}/bin/{b}", dst + ".pb"); os.rename(dst + ".pb", dst); n += 1
    ok = "tla-mc: built in" in out
    print(f"{time.strftime('%T')} {'built' if ok else 'FAILED'} {tag} {time.time() - t0:.0f}s, {n} binary placed", flush=True)
    shutil.rmtree(f"/data/work/pbmeta/{tag}", ignore_errors=True)
    return ok

os.makedirs("/data/work/pb", exist_ok=True)
with ThreadPoolExecutor(N) as ex:
    res = list(ex.map(one, todo))
print(f"{time.strftime('%T')} parbuild done: {sum(res)} of {len(res)} built", flush=True)
# the first runner's builds still waiting on cargo's lock: their binaries are in place now
subprocess.run(["pkill", "-9", "-f", "/data/work/cache-0"])
subprocess.run(["pkill", "-9", "-f", "^/home/ec2-user/.rustup/toolchains/.*/cargo build --release --quiet$"])
print(f"{time.strftime('%T')} the first runner's waiting builds stopped", flush=True)
