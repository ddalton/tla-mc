# The gates

`.github/workflows/gate.yml` runs three jobs on every pull request and every
push to main. A change that makes tla-mc refuse a model it checked before, or
check one to another verdict or distinct count, fails them; a pull request
cannot merge until they pass.

| job | what runs | what it is held to |
|---|---|---|
| `tests` | `cargo test --release` | its own tests |
| `examples` | the 114 [tlaplus/Examples](https://github.com/tlaplus/Examples) models in `examples.txt`, through the interpreter, the [CommunityModules](https://github.com/tlaplus/CommunityModules) on `TLAMC_LIB` | the result each model's manifest records from TLC: verdict and distinct states (a violated `[][A]_v` counts as either kind of failure: TLC finds it in its liveness pass, tla-mc as an action property) |
| `flint` | the 343 [flint](https://github.com/ddalton/flint) gate entries in `flint-only.txt` through the interpreter, the 3 in `flint-compiled.txt` through `-compile` | `flint-baseline.jsonl`: the verdict of every entry, and the distinct count of every strict and liveness run (a mutation run stops at its first violation, so its count moves with the workers) |

The three trees are pinned, in the workflow's `env`, to the commits the
baselines were recorded on, so a model added or changed upstream cannot fail a pull
request here. `examples.txt` holds the accepted models that finish within the
CI budget; `flint-only.txt` the gate entries that took under 5 s on the
2026-10-07 validation.

## Refreshing

After a change that is meant to move a result, or to pick up new models:

```sh
cargo build --release
# Examples: a survey of every exhaustive-search model, then the accepted ones under 30 s
git clone https://github.com/tlaplus/Examples /tmp/examples
git clone https://github.com/tlaplus/CommunityModules /tmp/community
TLAMC_LIB=/tmp/community/modules WORKERS=4 TIMEOUT=60 python3 scripts/examples-gate.py /tmp/examples target/release/tla-mc > /tmp/survey.txt
awk '$1 == "accepted" && $2+0 < 30 { print $3 }' /tmp/survey.txt > gate/examples.txt
# flint: the sweep at the pinned commit, as the baseline
git clone https://github.com/ddalton/flint /tmp/flint && git -C /tmp/flint checkout $FLINT_REF
FLINT_ROOT=/tmp/flint ONLY=$PWD/gate/flint-only.txt python3 /tmp/flint/formal/tla-mc-gate/sweep.py target/release/tla-mc 120 gate/flint-baseline.jsonl
```

Then move `EXAMPLES_REF` / `COMMUNITY_REF` / `FLINT_REF` in the workflow to the commits used,
and say in the commit what moved and why. The full validation (every gate
entry, the TLC side, all of Examples) is a release step, not a gate: see the
README's "How it is checked".
