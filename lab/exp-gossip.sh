#!/usr/bin/env bash
# E22b: does a second host reach fetchers that were only told about the first?
#
#   exp-gossip.sh PKG_DIR MBIT RTT_MS
#
# host 1 imports the package and serves on 127.0.0.1:29471. host 2 becomes a
# host the plain way: a helper with
# -modelbind=127.0.0.1:29472 that FETCHES the link from host 1. Then a fresh
# fetcher is given ONLY host 1 (behind a WAN proxy, so the download takes long
# enough for a swarm to form) and fetches the link.
#
# Reported: whether the fetcher ever connected to host 2 (host 2's proxy-less
# port is watched with lsof), the result, and the error if any.
# FETCHER_MODELD swaps the fetcher binary, as in exp-multihost.sh.
set -uo pipefail
PKG=$1 MBIT=$2 RTT=$3
source "$(dirname "$0")/env.sh"
mkdir -p "$LAB_ROOT/results"
N=${E22B_NAME:-e22b}
H1="$N-h1" H2="$N-h2" F="$N-f"
for n in "$H1" "$H2" "$F"; do lab_wipe "$n"; done

lab_start "$H1" -modelbind=127.0.0.1:29471 -modelhost=1 || exit 1
URI=$(lab_rpc "$H1" hostmodel "\"$PKG\"" '{"pin":true,"publish":false}' \
      | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["uri"])') || exit 1

# host 2: fetch from host 1 directly (fast), with its own public bind
lab_start "$H2" -modelbind=127.0.0.1:29472 -modelhost=1 -modelpeer=127.0.0.1:29471 || exit 1
lab_fetch "$H2" "$URI" --timeout 600 > "$LAB_ROOT/$N-h2fetch.json"
echo "host 2 fetched: $(cat "$LAB_ROOT/$N-h2fetch.json")"
echo "host 1 network info: $(lab_rpc "$H1" getmodelnetworkinfo | python3 -I -c 'import json,sys; d=json.load(sys.stdin); print({k: d[k] for k in d if "peer" in k or "provider" in k or "pex" in k})' 2>/dev/null)"

python3 -I "$LAB_PY/wanproxy.py" --listen 127.0.0.1:39471 --to 127.0.0.1:29471 \
  --rtt-ms "$RTT" --mbit "$MBIT" --log "$LAB_ROOT/$N-p1.jsonl" & P1=$!
sleep 0.5
MODELD=${FETCHER_MODELD:-$MODELD} lab_start "$F" -modelpeer=127.0.0.1:39471 || exit 1
FPID=$(cat "$LAB_ROOT/$F/pid")
lab_fetch "$F" "$URI" --timeout 1200 --trace "$LAB_ROOT/$N-trace.jsonl" > "$LAB_ROOT/$N-f.json" &
FP=$!
SEEN=0
while kill -0 "$FP" 2>/dev/null; do
  if lsof -nP -a -p "$FPID" -iTCP:29472 2>/dev/null | grep ESTABLISHED >/dev/null; then SEEN=1; fi
  sleep 0.5
done
wait "$FP"
kill "$P1" 2>/dev/null; wait "$P1" 2>/dev/null
python3 -I - "$(cat "$LAB_ROOT/$N-f.json")" "$SEEN" > "$LAB_ROOT/results/$N.json" <<'EOF'
import json, sys
s = json.loads(sys.argv[1]) if sys.argv[1].strip().startswith("{") else {"raw": sys.argv[1]}
s["fetcher_connected_to_host2"] = sys.argv[2] == "1"
print(json.dumps(s))
EOF
cat "$LAB_ROOT/results/$N.json"
for n in "$F" "$H2" "$H1"; do lab_stop "$n"; done
