# tlaplus/Examples benchmark, 2026-10-07

Seven exhaustive-search models from [tlaplus/Examples](https://github.com/tlaplus/Examples),
each run once per checker, sequentially, on an otherwise idle **Apple M1** (4 performance +
4 efficiency cores, 8 GB, macOS 26.3.1). Wall time includes process start (and the JVM's).
`bench.py` is the harness; `run2.jsonl` and `coffee.jsonl` its records; `logs/` every run's
full output.

| checker | version | how it was run |
|---|---|---|
| TLC | 1.8.0 (`tla2tools.jar` of 2026-10-06), Java 25.0.1, `-XX:+UseParallelGC` | `-workers N -config X.cfg X.tla`; CommunityModules-deps.jar and TLAPS.tla on the path where a spec needs them |
| tla-mc interpreter | this repo | `-workers N -config X.cfg X.tla` (`-lib` CommunityModules) |
| tla-mc compiled | this repo | `-codegen`, `cargo build --release` (timed separately), the binary run like the interpreter |
| tlaplusplus | 1.2.30, `2884162` | `run-tla --module X.tla --config X.cfg --workers N --work-dir D --fresh` |
| tla-rs (`tla-checker`) | 0.23.3, `326e89c` | `X.tla --config X.cfg --max-states 1000000000 --max-depth 1000000` (single-threaded) |
| tla2 | — | does not build from source: a git dependency (`andrewdyates/tMIR`) returns 404 |

## Results (seconds; every count shown matched the manifest's distinct states)

| model | distinct | TLC 1w | tla-mc interp 1w | tla-mc compiled 1w | TLC 8w | tla-mc interp 8w | tla-mc compiled 8w | build | tlaplusplus 8w | tla-rs 1w |
|---|---|---|---|---|---|---|---|---|---|---|
| SlushSmall | 274,678 | 6.18 | 3.04 | 1.97 | 2.57 | 0.99 | 0.36 | 17.9 | wrong verdict¹ | parse error |
| EWD998PCal | 321,370 | 29.40 | 26.01 | 14.22 | 7.44 | 8.06 | 3.28 | 19.0 | 17.11 | parse error |
| MCbtree | 374,727 | 30.82 | 16.78 | 11.54 | 7.28 | 6.06 | 2.54 | 19.9 | 77.34 | cfg error |
| CoffeeCan1000Beans | 501,500 | 38.64 | 4.04 | 3.70 | 25.15 | 2.34 | 2.12 | 17.8 | 8.16 | constant error |
| MCBakery | 655,200 | 7.95² | 7.02 | 3.65 | 5.87² | 5.81 | 2.65 | 21.8 | 51.65 | cfg error |
| MCLamportMutex | 724,274 | 14.12 | 11.18 | 6.43 | 4.75 | 3.60 | 1.30 | 21.7 | wrong verdict¹ | 592.3 (exact) |
| PaxosCommit | 1,321,761 | 134.05 | 54.51 | 35.85 | 31.29 | 14.36 | 9.31 | 19.4 | 267.13 | killed³ |

Geometric-mean speedup of tla-mc over TLC: compiled **3.2×** at 1 worker, **4.0×** at 8;
interpreter **2.0×** and **1.9×**. The build is a one-off per spec (`-compile` caches it:
~8.5 s once the library is built, 0 s for a rerun).

1. tlaplusplus reports a violation where the spec holds. SlushSmall: `failed evaluating
   invariant 'TypeInvariant': failed to resolve NoColor` (a model-value constant);
   MCLamportMutex: `failed evaluating invariant 'TypeOK': failed to resolve Clock: cannot
   enumerate Nat`. See `logs/*-tpp-*.log`.
2. TLC needs TLAPS.tla for MCBakery; the harness's first run lacked it (`run2.jsonl` records
   that error). Re-run with `-DTLA-Library` pointing at tlapm's `library/TLAPS.tla`; the
   times in the table and `logs/MCBakery-tlc-w*.log` are that run.
3. tla-rs on PaxosCommit is killed by the OS (SIGKILL, exit 137) after 100 states, ~9 s,
   with ~0.4 GB resident; reproduced twice.

CoffeeCan1000Beans is from `coffee.jsonl`: it needed `IF c THEN <>A ELSE <>B` in a property,
added the same day (the other rows were run on the build just before; timed old vs new on
four property-checking models, no difference).

tlaplusplus's PaxosCommit at 1 worker timed out at 600 s (996,398 distinct by then); its
1-worker times on the rest are in `run2.jsonl`.
