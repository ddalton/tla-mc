#!/usr/bin/env bash
# Run one tla-mc search over several machines (`-cluster`), from a machine
# that can ssh to each of them (they need not reach one another by ssh,
# only by TCP on PORT).
#
#   HOSTS="h0 h1 h2" scripts/cluster-run.sh SPECDIR [tla-mc options] -config X.cfg X.tla
#
# SPECDIR is copied to every node, and the options are given as from inside
# it. The first host is node 0: it builds tla-mc (and with -compile the
# spec's checker; a Rust toolchain is needed there only), prints the result,
# and its output is shown here; every node's is kept in LOGS. The binary
# reaches the others through this machine.
#
#   HOSTS        the nodes, node 0 first (names they resolve for each other,
#                or IP addresses)
#   CLUSTER_USER the ssh user (default: yours)
#   PORT         the TCP port every node listens on (default 27470: below
#                32768, where Linux starts giving outgoing connections their
#                local ports, and one of those holding it fails the listen)
#   REMOTE_DIR   under each node's home (default tla-mc-cluster)
#   LOGS         where the nodes' output is kept here (default cluster-logs/<time>)
set -euo pipefail

read -r -a hosts <<< "${HOSTS:?set HOSTS to the nodes, node 0 first}"
[ ${#hosts[@]} -ge 2 ] || { echo "cluster-run: HOSTS needs at least two nodes" >&2; exit 2; }
at=${CLUSTER_USER:+$CLUSTER_USER@}
port=${PORT:-27470}
remote=${REMOTE_DIR:-tla-mc-cluster}
src=$(cd "$(dirname "$0")/.." && pwd)
specdir=${1:?usage: HOSTS=... cluster-run.sh SPECDIR [tla-mc options] X.tla}
shift
logs=${LOGS:-cluster-logs/$(date +%Y%m%d-%H%M%S)}
mkdir -p "$logs"

compile=0
args=()
for a in "$@"; do
    if [ "$a" = -compile ]; then compile=1; else args+=("$a"); fi
done
quoted=$(printf '%q ' "${args[@]}")
r() { ssh -o BatchMode=yes "$at$1" "${@:2}"; }
# copy directory $1 here to $3 under $remote on node $2, over what is there
# (tar: the nodes need not have rsync); then the excludes
put() {
    local dir=$1 host=$2 dst=$3
    shift 3
    COPYFILE_DISABLE=1 tar -C "$dir" -czf - "${@/#/--exclude=}" . | r "$host" "mkdir -p $remote/$dst && tar -C $remote/$dst --warning=no-unknown-keyword -xzf -"
}
build=${hosts[0]}

echo "cluster-run: building on $build" >&2
put "$src" "$build" src ./target ./.git ./results ./cluster-logs
r "$build" "rm -rf $remote/spec"
put "$specdir" "$build" spec ./states
r "$build" "cd $remote/src && ~/.cargo/bin/cargo build --release --quiet && cp target/release/tla-mc ../tla-mc"
bin=tla-mc
if [ $compile = 1 ]; then
    # the spec's checker, generated and built once, run by every node
    r "$build" "cd $remote/spec && rm -rf ../gen && ../tla-mc -codegen ../gen $quoted >&2 && cd ../gen && PATH=\$HOME/.cargo/bin:\$PATH cargo build --release --quiet && cp \$(find target/release -maxdepth 1 -type f -perm -u+x -name 'tlcgen-*') ../checker"
    bin=checker
fi

# the binary comes here once and goes from here to each node: not one
# connection to the build host per node at once (its sshd drops some of
# those, MaxStartups), and a node keeps it only if all of it arrived
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
r "$build" "cat $remote/$bin" > "$tmp/$bin"
size=$(r "$build" "wc -c < $remote/$bin")
[ "$(wc -c < "$tmp/$bin" | tr -d ' ')" = "$size" ] || { echo "cluster-run: $bin from $build came short" >&2; exit 2; }
push() {
    r "$1" "cat > $remote/$bin.tmp && [ \$(wc -c < $remote/$bin.tmp) = $size ] && chmod +x $remote/$bin.tmp && mv $remote/$bin.tmp $remote/$bin" < "$tmp/$bin"
}

echo "cluster-run: copying $bin and the spec to ${hosts[*]:1}" >&2
pids=()
for h in "${hosts[@]:1}"; do
    { r "$h" "rm -rf $remote/spec" && put "$specdir" "$h" spec ./states && push "$h" ||
        { echo "cluster-run: copying to $h failed" >&2; exit 1; }; } &
    pids+=($!)
done
copied=1
for p in "${pids[@]}"; do wait "$p" || copied=0; done
[ $copied = 1 ] || exit 2

addrs=$(printf "%s:$port," "${hosts[@]}")
addrs=${addrs%,}
stop() {
    # ([.] so the pattern does not match the shell that runs pkill)
    for h in "${hosts[@]}"; do r "$h" "pkill -f '[.][.]/$bin -cluster' || true" & done
    wait
}
trap 'stop; exit 130' INT TERM
stop
for k in $(seq 1 $((${#hosts[@]} - 1))); do
    r "${hosts[$k]}" "cd $remote/spec && exec ../$bin -cluster $addrs -node $k $quoted" > "$logs/node$k.log" 2>&1 &
done
set +e
r "$build" "cd $remote/spec && exec ../$bin -cluster $addrs -node 0 $quoted" 2>&1 | tee "$logs/node0.log"
rc=${PIPESTATUS[0]}
wait
for k in $(seq 1 $((${#hosts[@]} - 1))); do
    echo "node $k: $(tail -n 1 "$logs/node$k.log")"
done
exit "$rc"
