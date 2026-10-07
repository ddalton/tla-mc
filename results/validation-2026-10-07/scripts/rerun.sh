#!/bin/bash
# User 2026-10-07 ~20:40Z: after phase C, rerun the gate entries that hit the 2400 s cap at 8 workers, one at a
# time with all 192 workers, COMPILED (-compile: the path proven at 192 cores), 1800 s each incl. the build, judged by the gate's expectation. Waits for the main runner (tlamc-val) to
# exit — including its own "shutdown -h +5" — then replaces that shutdown.
export HOME=/root AWS_DEFAULT_REGION=us-west-1; ulimit -n 1048576
BK=flint-tlamc-val-20261007; O=/data/out; F=/opt/flint; T=/root/.cargo/bin/tla-mc
log() { echo "$(date -u +%FT%TZ) $*" | tee -a $O/RESULTS.txt; }
while systemctl is-active --quiet tlamc-val; do sleep 5; done
sleep 2; shutdown -c; shutdown -h +120
python3 -c "
import json
for l in open('$O/gate-sweep.jsonl'):
    r=json.loads(l)
    if r['status']=='timeout': print(r['dir'], r['module'], r['cfg'], r['kind'], r.get('want') or '-')
" > /data/rerun.txt
log "rerun: $(wc -l < /data/rerun.txt) timed-out entries, compiled, at 192 workers, 1800 s each (shutdown re-armed +120)"
while read d mod cfg kind want; do
  grep -q "rerun $d/$cfg:" $O/RESULTS.txt && continue   # already rerun (rerun-now.sh)
  rm -rf /data/rr-meta; t0=$(date +%s)
  (cd $F/$d && TLAMC_CACHE=/data/tlamc-cache timeout 1800 $T -compile -workers 192 -checkpoint 0 -metadir /data/rr-meta -fpmem 32000 -queue-mem 100000 -config $cfg $mod.tla > $O/rerun-$cfg.out 2>&1); rc=$?
  rm -rf /data/rr-meta
  v=$(grep -oE "(Invariant|Action property|Temporal property|Property) [A-Za-z_0-9]+ is violated" $O/rerun-$cfg.out | head -1 | awk '{print $(NF-2)}')
  n=$(grep -oE "[0-9]+ distinct states found" $O/rerun-$cfg.out | tail -1)
  if [ $rc -eq 124 ]; then j="TIMEOUT (1800 s at 192)"
  elif [ $kind = strict_run ] && [ $rc -eq 0 ] && [ -z "$v" ]; then j="AGREE (holds)"
  elif [ $kind != strict_run ] && [ -n "$v" ] && { [ "$want" = - ] || echo "$v" | grep -qE "$want"; }; then j="AGREE (violated $v)"
  else j="DISAGREE (rc $rc, violated '$v', want $kind $want)"; fi
  log "rerun $d/$cfg: $j | $n | $(( $(date +%s) - t0 ))s"
done < /data/rerun.txt
log DONE2; aws s3 cp $O/ s3://$BK/out/ --recursive --quiet
shutdown -h +5
