#!/bin/bash
S=/private/tmp/claude-503/-Users-ddalton-github-tla-mc/cd8e277d-02b6-47c2-9246-47162a7ae605/scratchpad
FL=/Users/ddalton/github/flint
MAIN=$S/wt-main/target/release/tla-mc
PR2=$S/wt-pr-2/target/release/tla-mc
ONLY=/Users/ddalton/github/tla-mc/results/validation-2026-10-12/only-interp.txt
cd $S/val
echo "[$(date +%T)] leg 1: interpreter sweep, 343 fast entries, main and pr-2 concurrently"
( FLINT_ROOT=$FL ONLY=$ONLY TMPDIR=$S/tmp-main python3 $FL/formal/tla-mc-gate/sweep.py $MAIN 60 $S/val/interp-main.jsonl > $S/val/interp-main.log 2>&1 ) &
( FLINT_ROOT=$FL ONLY=$ONLY TMPDIR=$S/tmp-pr2 python3 $FL/formal/tla-mc-gate/sweep.py $PR2 60 $S/val/interp-pr2.jsonl > $S/val/interp-pr2.log 2>&1 ) &
wait
echo "[$(date +%T)] leg 1 done"

run() { # label bin dir cfg module [extra args]
  local lab=$1 bin=$2 d=$3 cfg=$4 mod=$5; shift 5
  local md=$S/md-$lab; rm -rf $md
  local out=$S/val/$lab-$cfg.out
  ( cd $FL/$d && /usr/bin/time -l $bin "$@" -workers 8 -metadir $md -config $cfg $mod.tla ) > $out 2>&1
  rm -rf $md
  local real=$(grep -E '^\s*[0-9.]+ real' $out | awk '{print $1}')
  local rss=$(grep 'maximum resident set size' $out | awk '{print $1}')
  local dist=$(grep -oE '[0-9]+ distinct states found' $out | tail -1)
  local viol=$(grep -oE '(Invariant|Temporal property|Action property|Property) \S+ is violated' $out | head -1)
  local rc=$(grep -oE 'Command exited with non-zero status [0-9]+' $out | awk '{print $NF}')
  echo "$lab $d/$cfg real=${real}s rss=$((${rss:-0}/1048576))MB ${dist:-no-count} ${viol:-} rc=${rc:-0}"
}

echo "[$(date +%T)] leg 2: slow exact-count entries, interpreter, main then pr-2, one at a time"
while read d cfg mod; do
  run interp-main $MAIN $d $cfg $mod
  run interp-pr2  $PR2  $d $cfg $mod
done <<'LIST'
formal FlintTierSession.cfg FlintTierSession
formal FlintReplicationBounceRaceFixed.cfg FlintReplication
formal FlintCompositionWitness.cfg FlintComposition
lean/formal LeanImmutableSyncQueueShippedInvs.cfg LeanSubtree
lean/formal LeanBarrierLeaseSameBytesVerified.cfg LeanSubtree
formal FlintTierSessionLive.cfg FlintTierSession
formal ForgeSyncLive.cfg ForgeSync
lean/formal LeanCoreCitesBlind.cfg LeanCore
LIST
echo "[$(date +%T)] leg 2 done"

echo "[$(date +%T)] leg 3: -compile, fresh caches; first run builds, second run is cached"
while read d cfg mod; do
  TLAMC_CACHE=$S/cache-main run compile-main-build $MAIN $d $cfg $mod -compile
  TLAMC_CACHE=$S/cache-main run compile-main       $MAIN $d $cfg $mod -compile
  TLAMC_CACHE=$S/cache-pr2  run compile-pr2-build  $PR2  $d $cfg $mod -compile
  TLAMC_CACHE=$S/cache-pr2  run compile-pr2        $PR2  $d $cfg $mod -compile
done <<'LIST'
formal FlintTierSessionNoFence.cfg FlintTierSession
formal FlintTierSession.cfg FlintTierSession
formal FlintTierSessionLive.cfg FlintTierSession
lean/formal LeanImmutableSyncQueueShippedInvs.cfg LeanSubtree
LIST
echo "[$(date +%T)] leg 3 done"
echo ALLDONE
