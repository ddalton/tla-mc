#!/bin/bash
# User ~22:10Z: stop phase C (gate_par, 440/443 done) and run the remaining entries' COMPILED checkers one at a time
# at 192 workers (nothing else running), 1200 s each, judged by the gate's rule; strict_run counts vs today's TLC.
export HOME=/root AWS_DEFAULT_REGION=us-west-1; source /root/.cargo/env; ulimit -n 1048576
BK=flint-tlamc-val-20261007; O=/data/out; F=/opt/flint; T=/root/.cargo/bin/tla-mc
log() { echo "$(date -u +%FT%TZ) $*" | tee -a $O/RESULTS.txt; }
systemctl stop tlamc-cancel tlamc-bc; pkill -f gate_par.py; sleep 1; pkill -f "gate-work"; pkill -f "cargo build"; sleep 2; pkill -9 -f "gate-work"; rm -rf /data/gate-work
python3 - > /data/remaining.txt <<PY
import json
done={(json.loads(l)['dir'],json.loads(l)['cfg']) for l in open('$O/gate-par.jsonl')}
tlc={}
for l in open('$O/tlc.jsonl'):
    r=json.loads(l); tlc[(r['dir'],r['cfg'])]=r.get('tlc_distinct')
for l in open('$O/gate-sweep.jsonl'):
    r=json.loads(l)
    if r['status']=='agree' and (r['dir'],r['cfg']) not in done:
        print(r['dir'], r['module'], r['cfg'], r['kind'], tlc.get((r['dir'],r['cfg'])) or '-', r.get('want') or '-')
PY
log "phase C stopped by hand (user) at $(wc -l < $O/gate-par.jsonl)/443; remaining $(wc -l < /data/remaining.txt), compiled, 192 workers, 1200 s each: $(awk '{print $3}' /data/remaining.txt | tr '\n' ' ')"
while read d mod cfg kind tlcn want; do
  rm -rf /data/fin-meta; t0=$(date +%s)
  (cd $F/$d && TLAMC_CACHE=/data/tlamc-cache-fin timeout 1200 $T -compile -workers 192 -checkpoint 0 -metadir /data/fin-meta -fpmem 32000 -queue-mem 120000 -config $cfg $mod.tla > $O/finish-$cfg.out 2>&1); rc=$?
  rm -rf /data/fin-meta
  n=$(grep -oE "[0-9]+ distinct states found" $O/finish-$cfg.out | tail -1 | awk '{print $1}')
  vl=$(grep -E "is violated|were violated" $O/finish-$cfg.out | head -1)
  if [ $rc -eq 124 ]; then j="TIMEOUT (1200 s at 192 workers)"
  elif [ $kind = strict_run ]; then
    if [ $rc -eq 0 ] && [ -z "$vl" ]; then j="AGREE (holds; $n distinct, TLC $tlcn)"; else j="DISAGREE (must hold: rc $rc, $vl)"; fi
  elif [ -n "$vl" ] && { [ "$want" = - ] || grep -qF "$want" $O/finish-$cfg.out || echo "$vl" | grep -qE "$want"; }; then j="AGREE (violated: $vl)"
  else j="DISAGREE (rc $rc, want $kind '$want', got '$vl')"; fi
  log "finish $d/$cfg: $j | $(( $(date +%s) - t0 ))s"
done < /data/remaining.txt
log DONE; echo DONE > $O/DONE; aws s3 cp $O/ s3://$BK/out/ --recursive --quiet
shutdown -h +5
