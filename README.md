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
- **Across machines**: `-cluster` spreads one search over several nodes: 4.4×
  on six, and 693 M distinct states in 134 s on 39 ([below](#on-a-cluster));
  safety properties for now.

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

On many cores, compile: the interpreter clones shared values as it reads them, and
at 192 workers it spent most of its time on their reference counts (~850 distinct
states/s per core on one model, against ~31,000 compiled).

| option | |
|---|---|
| `-workers N` | threads (default: all cores) |
| `-pin`, `-pin=ORDER` | keep each worker on one CPU, one hardware thread per core first; ORDER `compact` (default: fill one node, then the next), `spread` (deal workers to the nodes in turn) or `llc` (to the last-level caches in turn); Linux |
| `-compile` | generate, build (cached) and run a native checker; needs `cargo` |
| `-lib DIR` | where to find modules not beside the spec, e.g. the [CommunityModules](https://github.com/tlaplus/CommunityModules) `modules/` directory (also `TLAMC_LIB`) |
| `-metadir DIR`, `-checkpoint MIN`, `-recover DIR` | where state spills and checkpoints go; resume a stopped run |
| `-fpmem MB`, `-queue-mem MB` | memory for the seen set and the BFS queue before they spill to disk (default: an eighth of the memory available, at least 1024 / 256 MB) |
| `-cluster HOST:PORT,...`, `-node K` | one search over several machines, each started with the same list and its own place in it ([below](#on-a-cluster)) |
| `-engine interp\|closure` | the evaluator when not compiled |
| `-version` | print the version |
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

### On a cluster

`-cluster` runs one search over several machines: each node keeps the states
whose fingerprints it owns (its part of the seen set and of every BFS level,
with its own workers, memory and disk), and a successor another node owns goes
there — its 8-byte key first, the state itself only if the owner says it is
new. Node 0 prints the result and the counterexample, whose states may have
been found on any node. Every node needs the same spec, cfg and tla-mc:

```sh
# on each machine, its own place in the list as -node
tla-mc -cluster h0:27470,h1:27470,h2:27470 -node 1 -workers 40 -config MC.cfg MC.tla
# or from a machine that can ssh to them all: builds on the first, copies, starts all
HOSTS="h0 h1 h2" scripts/cluster-run.sh SPECDIR -compile -workers 40 -config MC.cfg MC.tla
```

Use a port below 32768: above it, Linux gives outgoing connections their
local ports, and one holding the port makes the listen fail. The list takes
names or IP addresses; each node looks its peers up once, all at once, before
it starts waiting for them.

On nodes of 20 cores each (2× Xeon Silver 4114, 125 GB, 10 GbE), compiled, 40
workers a node, with the same counts on every number of nodes:

| flint model | one node | six nodes | 39 nodes |
|---|---:|---:|---:|
| `LeanP1ReaderScopeHolds` (19.4 M distinct states) | 90.2 s | **20.5 s** (4.4×) | not run |
| `LeanP1AllHolds` (693.2 M distinct, 3.2 G generated, depth 41) | not run | 740 s | **134.3 s** |

On 39 nodes `LeanP1AllHolds` runs 5.5× faster than on six (6.5× the nodes):
5.2 M distinct states/s, each node keeping 17.8 M of them, the widest level 61 M
states and nothing spilled to disk. Not yet on a cluster: liveness properties,
checkpoints and `-recover` (refused). How it works:
[`docs/DESIGN.md`](docs/DESIGN.md#across-machines--cluster).

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

- **tlaplus/Examples**: tla-mc accepts **125 of the 191** exhaustive-search models
  (Examples `bce9f21`), and **every one it accepts matches TLC's recorded result**
  (verdict and distinct states). Of the rest, it refuses 60 with a message — it
  never checks less than the cfg asks — and does not finish 6 in 10 minutes (TLC's
  recorded runtimes for four of them are 50 minutes to 7 hours). 0.2.0 accepted 124
  (2026-10-30): it crashed loading DiningPhilosophers.cfg, whose invariant is
  `ENABLED Next`, as 0.1.2 did; 0.2.1 checks it.
- **Against TLC directly**: the 446 entries of [flint](https://github.com/ddalton/flint)'s
  TLA+ gate — **443 decided, all with the gate's expected verdict**, by the interpreter
  and the compiled checker alike; every planted bug caught; on the must-hold runs TLC
  finished, the same distinct-state count (126 of 128; the two others are `SYMMETRY` +
  `VIEW` worlds, where counts vary with timing at several workers, TLC's too).
  Run on the published 0.1.0 crate, 2026-10-07:
  [`results/validation-2026-10-07/`](results/validation-2026-10-07/SUMMARY.md).
  0.1.2 (a cache, memory-default and exit-code fix; no change to evaluation) was
  re-run on the 343 of those entries that take under 5 s, plus 10 through
  `-compile`: every verdict the same, and every must-hold and liveness count:
  [`results/validation-2026-10-12/`](results/validation-2026-10-12/SUMMARY.md).
  0.2.0 (the performance series — 1.1–3.5× faster on flint's models at 8 workers,
  a new queue format, so 0.1.2 checkpoints are refused — and tabs counted as SANY
  does) was run side by side with 0.1.2 on those 343 entries, the 8 slowest
  exact-count models and 4 through `-compile`: every verdict the same, every
  exact count equal:
  [`results/validation-2026-10-29/`](results/validation-2026-10-29/SUMMARY.md).
  Then the full gate on 192 cores, 0.1.2 beside it, the 412 fast entries interpreted
  and the 34 slow ones compiled at 192 workers: the same 443 decided, every one with
  the gate's verdict and 0.1.2's, every exact count TLC's but the two `SYMMETRY` +
  `VIEW` worlds; 2.0× faster on flint's models and 1.2× on lean's interpreted, 1.6×
  and 1.4× compiled (geometric means), peak memory up 24–48%:
  [`results/validation-2026-10-30/`](results/validation-2026-10-30/SUMMARY.md).
  0.2.1 changes one thing: an invariant that is `ENABLED` of an action and names no
  variable itself is evaluated in each state, not once at load, where it crashed;
  nothing changes for a model that loaded before. The gate passed on it: the tests,
  114 Examples models, DiningPhilosophers.cfg among them, and the 343 flint entries.
  0.2.2 changes one thing: an `EXCEPT` changes the entry in place when nothing else
  holds the function, where it copied the whole function once per clause; every
  verdict and count the same, 1.30–1.35× faster on lean's models compiled at 192
  workers and 1.25–1.4× on the M1, flint's unchanged
  ([#7](https://github.com/ddalton/tla-mc/pull/7)). The gate passed on it: the
  tests, 114 Examples models and the 343 flint entries.
- **Its own tests**, several of them checked to fail with the code they guard
  disabled.
- **On every pull request** ([`gate/`](gate/README.md)): the tests; the 114 Examples
  models it accepts in under 30 s, against their manifests; and the 343 fast entries of
  flint's gate, three of them through `-compile`, against the verdicts and exact counts
  recorded on 0.2.0. A change that refuses a model it checked before, or checks one to
  another result, cannot merge.

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
