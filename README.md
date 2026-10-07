# tla-mc

[![crates.io](https://img.shields.io/crates/v/tla-mc.svg)](https://crates.io/crates/tla-mc)

**A fast TLA+ model checker, written in Rust.** It reads the same `.tla` and `.cfg`
files as TLC, explores the same state space — down to the same distinct-state
count — and can compile a spec into a native, parallel checker.

- **~4× faster than TLC** on [tlaplus/Examples](https://github.com/tlaplus/Examples)
  models at 8 workers (2.2×–11.9×), ~3× on one worker — [table below](#performance).
- **Scales to 192 cores.** TLC stops getting faster at about 48 workers; tla-mc
  keeps going: ~9× TLC's throughput on a 192-core machine, and a 9.8-billion-state
  model checked in 99 minutes.
- **Exact against TLC.** Same distinct-state counts, same verdicts — including
  `SYMMETRY` + `VIEW`, where matching TLC meant matching its internal ordering
  (exact at one worker; with more, which state stands for a symmetric group
  depends on timing, for TLC as well).
- **Safety and liveness**: invariants, action properties `[][A]_v`, `~>`, `<>`,
  `[]<>`, `<>[]`, `IF` over temporal formulas, `WF`/`SF` fairness, `INSTANCE`
  refinement.
- **Beyond memory**: the seen set, the queue and the liveness graph spill to disk;
  long runs checkpoint and resume.

## Quick start

```sh
cargo install tla-mc   # Rust 1.88+
tla-mc -workers 8 -config MCPaxos.cfg MCPaxos.tla
```

Output follows TLC's (`No error has been found`, `N distinct states found`, a
counterexample trace on a violation), and so do the exit codes: 0 when the
properties hold, 12 on a violation, 10 for a false `ASSUME`, 2 for an error.

**For long runs, compile the spec.** `-compile` turns the spec into Rust, builds it
(the first build ~20 s, later specs ~8 s, a rerun of the same spec none: it is
cached under `~/.cache/tla-mc`), and runs it with the same arguments:

```sh
tla-mc -compile -workers 8 -config MCPaxos.cfg MCPaxos.tla
```

| option | |
|---|---|
| `-workers N` | threads (default: all cores) |
| `-compile` | generate, build (cached) and run a native checker; needs `cargo` |
| `-lib DIR` | where to find modules not beside the spec, e.g. the [CommunityModules](https://github.com/tlaplus/CommunityModules) `modules/` directory (also `TLAMC_LIB`) |
| `-metadir DIR`, `-checkpoint MIN`, `-recover DIR` | where state spills and checkpoints go; resume a stopped run |
| `-fpmem MB`, `-queue-mem MB` | memory for the seen set and the BFS queue before they spill to disk |
| `-engine interp\|closure` | the evaluator when not compiled |
| `-codegen DIR` | write the generated checker's crate to `DIR` and stop |

`scripts/pgo-build.sh` adds profile-guided optimization to a compiled checker,
~8% more for runs worth the extra minutes.

## Performance

### Against TLC, on tlaplus/Examples

Every model below was checked by both; every distinct-state count matched the
Examples manifest. One run each, wall time in seconds, on an Apple M1 (4
performance + 4 efficiency cores, 8 GB); TLC 1.8.0 on Java 25.

| model | distinct states | TLC, 1 worker | tla-mc compiled, 1 worker | TLC, 8 workers | tla-mc interpreted, 8 | **tla-mc compiled, 8** | vs TLC (8) |
|---|---:|---:|---:|---:|---:|---:|---:|
| SlushSmall | 274,678 | 6.18 | 1.97 | 2.57 | 0.99 | **0.36** | 7.1× |
| EWD998PCal | 321,370 | 29.40 | 14.22 | 7.44 | 8.06 | **3.28** | 2.3× |
| MCbtree | 374,727 | 30.82 | 11.54 | 7.28 | 6.06 | **2.54** | 2.9× |
| CoffeeCan1000Beans | 501,500 | 38.64 | 3.70 | 25.15 | 2.34 | **2.12** | 11.9× |
| MCBakery | 655,200 | 7.95 | 3.65 | 5.87 | 5.81 | **2.65** | 2.2× |
| MCLamportMutex | 724,274 | 14.12 | 6.43 | 4.75 | 3.60 | **1.30** | 3.7× |
| PaxosCommit | 1,321,761 | 134.05 | 35.85 | 31.29 | 14.36 | **9.31** | 3.4× |

Geometric mean over TLC: **compiled 4.0× at 8 workers, 3.2× at 1; interpreted
1.9× and 2.0×.** The compile step (17–22 s per model here, cold) is not in these
times: for a run TLC finishes in a few seconds, the interpreter is the right tool;
for anything longer, compile. Details, versions and every log:
[`results/bench-examples-2026-10-07/`](results/bench-examples-2026-10-07/SUMMARY.md).

### At scale

On an AWS c8g.48xlarge (192 Graviton4 cores), on a 756-million-state model
(`LeanP1AllHolds` from [flint](https://github.com/ddalton/flint)):

| | workers | distinct states / s |
|---|---:|---:|
| TLC 1.7.4 | 48 | ~156,000 (93.6 M in its first 600 s) |
| TLC 1.7.4 | 192 | ~150,000 (no faster than 48) |
| **tla-mc compiled** | **192** | **~1,420,000** — the whole model in 531 s |

That is ~9× TLC at its best setting (TLC was not run to completion; its projected
time was ~84 minutes). A larger model of the same spec — **9,821,039,047 distinct
states**, three syncing writers — ran to completion in 99 minutes on 192 cores,
spilling ~790 GB of queue to disk. The runs are recorded in flint's
`lean/formal/results/2026-10-04-leanp1-gate/`, `2026-10-05-ec2/` and `2026-10-06-ec2/`.

### Against other Rust TLA+ model checkers

Same seven models, same machine; 8 workers where the checker is parallel.

| checker | models checked correctly (of 7) | where both finished: tla-mc compiled is |
|---|:---:|---|
| **tla-mc** | **7** | — |
| [tlaplusplus](https://github.com/zoratu/tlaplusplus) 1.2.30 | 5 (2 false violations¹) | 3.8×–30× faster (EWD998PCal, MCbtree, CoffeeCan, MCBakery, PaxosCommit) |
| [tla-rs / `tla-checker`](https://github.com/fabracht/tla-rs) 0.23.3 | 1 (single-threaded²) | 92× faster, one thread each (MCLamportMutex: 592 s vs 6.4 s) |
| [tla2](https://github.com/joweeba/tla2) | — | could not be built (a git dependency returns 404) |

1. On SlushSmall and MCLamportMutex tlaplusplus reported an invariant violation
   where the invariant holds: its own evaluation failed (`failed to resolve NoColor`;
   `cannot enumerate Nat`) and was reported as a violation.
2. tla-rs could not parse two of the models, rejected the cfg of three, and was
   killed by the OS on PaxosCommit.

This is one machine and seven models; the logs are in the results directory, and
`bench.py` reruns it.

## How it is checked

- **tlaplus/Examples**: tla-mc accepts **115 of the 165** exhaustive-search
  models, and **every one it accepts matches TLC's recorded result** (verdict and
  distinct states). What it rejects, it rejects with a message — it never checks
  less than the cfg asks.
- **Against TLC directly**: the 446 entries of [flint](https://github.com/ddalton/flint)'s
  TLA+ gate — **443 decided, all with the gate's expected verdict**, by the interpreter
  and the compiled checker alike; every planted bug caught; on the must-hold runs TLC
  finished, the same distinct-state count (126 of 128; the two others are `SYMMETRY` +
  `VIEW` worlds, where counts vary with timing at several workers, TLC's too).
  Run on the published 0.1.0 crate, 2026-10-07:
  [`results/validation-2026-10-07/`](results/validation-2026-10-07/SUMMARY.md).
- **Its own tests**, several of them checked to fail with the code they guard
  disabled.

## What it supports

TLA+ expressions (sets, functions, records, sequences, `EXCEPT` with `@`,
`CHOOSE`, `CASE`, lazy `LET`, recursion, `LAMBDA`), `Init`/`Next` or
`SPECIFICATION`, `INVARIANT`, `CONSTRAINT`, `CHECK_DEADLOCK`, `SYMMETRY`, `VIEW`,
`ASSUME`, `INSTANCE` with `WITH`, `EXTENDS`; the standard modules, and
CommunityModules written in TLA+ (through `-lib`); theorems and proofs are
skipped, as TLC skips them.

Temporal properties in these shapes: `[][A]_v`, `P ~> Q`, `[](P => <>Q)`,
`<>P`, `[]<>P`, `<>[]P`, `IF c THEN A ELSE B`, conjunctions and `\A` of those,
with `WF`/`SF` fairness. Not yet: arbitrary temporal formulas (TLC builds a
tableau for any formula), `ACTION_CONSTRAINT`, simulation mode, and Java-overridden
modules such as `Graphs`. A cfg that needs any of these is refused, not
partially checked.

## More

- [`docs/DESIGN.md`](docs/DESIGN.md) — how it works and why: the BFS and fingerprints,
  exact `SYMMETRY`/`VIEW`, liveness on disk, the three evaluators, where the time
  went. Written as it was built, inside flint (as `tlc-rs`).
- [`results/`](results/) — the measurements behind every number above.

## License

MIT. `modules/Bags.tla` is the TLA+ standard module, MIT, from
[tlaplus/tlaplus](https://github.com/tlaplus/tlaplus) — see
[`modules/NOTICE.md`](modules/NOTICE.md).
