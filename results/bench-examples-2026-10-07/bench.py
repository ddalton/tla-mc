#!/usr/bin/env python3
"""TLA+ checker comparison on tlaplus/Examples models. Sequential runs, wall time per run, distinct-state
count and verdict parsed from each checker's output; raw logs kept. Usage: bench.py OUT.jsonl [spec-filter]"""
import json, os, re, shutil, subprocess, sys, time

B = os.path.dirname(os.path.abspath(__file__))
EX = f"{B}/Examples/specifications"
CM = f"{B}/CommunityModules/modules"
TLC_CP = f"{B}/tla2tools-1.8.0.jar"
# TLC 1.8.0 (2026-10-06) with the latest CommunityModules, only for the specs that use them
TLC_CP_CM = f"{B}/tla2tools-1.8.0.jar:{B}/CommunityModules-deps.jar"
NEEDS_CM = {"EWD998PCal", "MCbtree"}
TLAMC = f"{B}/../tgt-tlamc/release/tla-mc"  # a release build of this repo
TPP = f"{B}/tlaplusplus/target/release/tlaplusplus"
TLARS = f"{B}/tla-rs/target/release/tla"
LOGS = f"{B}/logs"
TIMEOUT = 600
os.makedirs(LOGS, exist_ok=True)

SPECS = [  # dir, module, cfg, distinct states recorded in the Examples manifest
    ("SlushProtocol", "Slush", "SlushSmall", 274678),
    ("ewd998", "EWD998PCal", "EWD998PCal", 321370),
    ("btree", "MCbtree", "MCbtree", 374727),
    ("CoffeeCan", "CoffeeCan", "CoffeeCan1000Beans", 501500),
    ("Bakery-Boulangerie", "MCBakery", "MCBakery", 655200),
    ("lamport_mutex", "MCLamportMutex", "MCLamportMutex", 724274),
    ("transaction_commit", "PaxosCommit", "PaxosCommit", 1321761),
]


def run(tag, cmd, cwd, env=None):
    log = f"{LOGS}/{tag}.log"
    t0 = time.monotonic()
    try:
        p = subprocess.run(cmd, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=TIMEOUT,
                           env={**os.environ, **(env or {})})
        out, rc = p.stdout.decode(errors="replace"), p.returncode
    except subprocess.TimeoutExpired as e:
        out, rc = (e.stdout or b"").decode(errors="replace") + "\n[TIMEOUT]", "timeout"
    wall = time.monotonic() - t0
    open(log, "w").write(" ".join(cmd) + "\n\n" + out)
    m = re.findall(r"([\d,]+) distinct states found", out) or re.findall(r"(?i)distinct states[:= ]+([\d,]+)", out) \
        or re.findall(r"(?i)([\d,]+) (?:unique |distinct )?states? (?:explored|found|visited)", out)
    distinct = int(m[-1].replace(",", "")) if m else None
    if re.search(r"No error has been found|violation=false|Model checking completed successfully", out) and rc in (0,):
        verdict = "holds"
    elif re.search(r"(?i)is violated|violation=true|invariant .* violated", out):
        verdict = "violated"
    elif rc == "timeout":
        verdict = "timeout"
    else:
        verdict = "error"
    return {"wall": round(wall, 2), "distinct": distinct, "verdict": verdict, "rc": rc, "log": log,
            "tail": out.strip().splitlines()[-1][:160] if out.strip() else ""}


def main():
    outp, filt = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else "")
    for d, mod, cfg, want in SPECS:
        if filt and filt not in cfg:
            continue
        cwd = f"{EX}/{d}"
        rows = []
        md = f"/tmp/bench-md"
        # tla-mc generated checker: compile once (timed), run below
        gen, gt = f"{B}/gen/{cfg}", f"{B}/gt/{cfg}"
        shutil.rmtree(gen, ignore_errors=True)
        t0 = time.monotonic()
        g = subprocess.run([TLAMC, "-lib", CM, "-codegen", gen, "-config", f"{cfg}.cfg", f"{mod}.tla"], cwd=cwd,
                           capture_output=True)
        b = subprocess.run(["cargo", "build", "--release", "-q"], cwd=gen, capture_output=True,
                           env={**os.environ, "CARGO_TARGET_DIR": gt}) if g.returncode == 0 else None
        compile_s = round(time.monotonic() - t0, 1)
        gbin = None
        if b is not None and b.returncode == 0:
            gbin = [f"{gt}/release/{x}" for x in os.listdir(f"{gt}/release") if x.startswith("tlcgen-") and not x.endswith(".d")][0]
        rows.append({"checker": "tla-mc compile", "workers": 0, "wall": compile_s,
                     "verdict": "ok" if gbin else "error", "distinct": None,
                     "tail": "" if gbin else (g.stderr.decode()[-300:] + (b.stderr.decode()[-300:] if b else ""))})
        for w in (1, 8):
            shutil.rmtree(md, ignore_errors=True)
            rows.append({"checker": "TLC", "workers": w, **run(f"{cfg}-tlc-w{w}",
                ["java", "-XX:+UseParallelGC", "-cp", TLC_CP_CM if cfg in NEEDS_CM else TLC_CP, "tlc2.TLC", "-workers", str(w), "-metadir", md,
                 "-config", f"{cfg}.cfg", f"{mod}.tla"], cwd)})
            shutil.rmtree(md, ignore_errors=True)
            rows.append({"checker": "tla-mc interp", "workers": w, **run(f"{cfg}-tlamc-w{w}",
                [TLAMC, "-lib", CM, "-workers", str(w), "-checkpoint", "0", "-metadir", md, "-config", f"{cfg}.cfg", f"{mod}.tla"], cwd)})
            shutil.rmtree(md, ignore_errors=True)
            if gbin:
                rows.append({"checker": "tla-mc compiled", "workers": w, **run(f"{cfg}-tlamcgen-w{w}",
                    [gbin, "-lib", CM, "-workers", str(w), "-checkpoint", "0", "-metadir", md, "-config", f"{cfg}.cfg", f"{mod}.tla"], cwd)})
                shutil.rmtree(md, ignore_errors=True)
            rows.append({"checker": "tlaplusplus", "workers": w, **run(f"{cfg}-tpp-w{w}",
                [TPP, "run-tla", "--module", f"{mod}.tla", "--config", f"{cfg}.cfg", "--workers", str(w),
                 "--work-dir", md, "--fresh"], cwd)})
            shutil.rmtree(md, ignore_errors=True)
            if w == 1:
                rows.append({"checker": "tla-rs", "workers": 1, **run(f"{cfg}-tlars-w1",
                    [TLARS, f"{mod}.tla", "--config", f"{cfg}.cfg", "--max-states", "1000000000", "--max-depth", "1000000"], cwd)})
        with open(outp, "a") as f:
            for r in rows:
                r.update({"spec": cfg, "want": want})
                f.write(json.dumps(r) + "\n")
                print(f"{cfg:20} {r['checker']:16} w{r['workers']} {r['wall']:>8}s {str(r.get('distinct')):>10} "
                      f"(want {want}) {r['verdict']:9} {r.get('tail','')[:70]}", flush=True)


main()
