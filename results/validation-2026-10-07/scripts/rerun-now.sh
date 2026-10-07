#!/bin/bash
# User ~20:45Z: rerun the two timed-out entries NOW on the idle cores (phase A's last entry uses 8): compiled,
# side by side at 92 workers each, 1800 s each incl. build, own build cache each (same module: a shared target
# dir could overwrite one checker with the other's). Judged by the gate's expectation.
export HOME=/root AWS_DEFAULT_REGION=us-west-1; source /root/.cargo/env; ulimit -n 1048576
O=/data/out; F=/opt/flint; T=/root/.cargo/bin/tla-mc
log() { echo "$(date -u +%FT%TZ) $*" | tee -a $O/RESULTS.txt; }
one() { # dir module cfg kind want
  local d=$1 mod=$2 cfg=$3 kind=$4 want=$5 t0=$(date +%s)
  rm -rf /data/rn-meta-$cfg
  (cd $F/$d && TLAMC_CACHE=/data/tlamc-cache-$cfg timeout 1800 $T -compile -workers 92 -checkpoint 0 -metadir /data/rn-meta-$cfg -fpmem 32000 -queue-mem 60000 -config $cfg $mod.tla > $O/rerun-$cfg.out 2>&1); local rc=$?
  rm -rf /data/rn-meta-$cfg
  local v=$(grep -oE "(Invariant|Action property|Temporal property|Property) [A-Za-z_0-9]+ is violated" $O/rerun-$cfg.out | head -1 | awk '{print $(NF-2)}')
  local n=$(grep -oE "[0-9]+ distinct states found|progress: depth [0-9]+, [0-9]+ generated, [0-9]+ distinct" $O/rerun-$cfg.out | tail -1)
  local j
  if [ $rc -eq 124 ]; then j="TIMEOUT (1800 s, compiled, 92 workers)"
  elif [ $kind = strict_run ] && [ $rc -eq 0 ] && [ -z "$v" ]; then j="AGREE (holds)"
  elif [ $kind != strict_run ] && [ -n "$v" ] && { [ "$want" = - ] || echo "$v" | grep -qE "$want"; }; then j="AGREE (violated $v)"
  else j="DISAGREE (rc $rc, violated '$v', want $kind $want)"; fi
  log "rerun $d/$cfg: $j | $n | $(( $(date +%s) - t0 ))s"
}
python3 -c "
import json
for f in __import__('glob').glob('$O/sweep-*.jsonl'):
    for l in open(f):
        r=json.loads(l)
        if r['status']=='timeout': print(r['dir'], r['module'], r['cfg'], r['kind'], r.get('want') or '-')
" > /data/rerun-now.txt
log "rerun-now: $(wc -l < /data/rerun-now.txt) timed-out entries, compiled, 92 workers each, side by side"
while read d mod cfg kind want; do one $d $mod $cfg $kind "$want" & done < /data/rerun-now.txt
wait
log "rerun-now done"
