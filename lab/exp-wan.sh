#!/usr/bin/env bash
# One isolated WAN-emulated retrieve: its own host, proxy and fetcher.
#
#   exp-wan.sh NAME PKG_DIR HOST_PORT PROXY_PORT RTT_MS MBIT [proxy chaos args...]
#
# Writes $LAB_ROOT/results/NAME.json (fetch summary + proxy connection stats)
# and leaves logs under $LAB_ROOT/NAME-{host,fetch}/ and NAME-conns.jsonl.
# Separate hosts per run, because every proxied connection arrives from
# 127.0.0.1 and the host caps inbound at 8 per address (pq1_runtime.h:27).
set -uo pipefail
NAME=$1 PKG=$2 HPORT=$3 PPORT=$4 RTT=$5 MBIT=$6; shift 6
source "$(dirname "$0")/env.sh"
mkdir -p "$LAB_ROOT/results"
H="$NAME-host" F="$NAME-fetch"
lab_wipe "$H"; lab_wipe "$F"

lab_start "$H" -modelbind=127.0.0.1:"$HPORT" -modelhost=1 || exit 1
URI=$(lab_rpc "$H" hostmodel "\"$PKG\"" '{"pin":true,"publish":false}' \
      | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["uri"])') || exit 1

python3 -I "$LAB_PY/wanproxy.py" --listen 127.0.0.1:"$PPORT" --to 127.0.0.1:"$HPORT" \
  --rtt-ms "$RTT" --mbit "$MBIT" --log "$LAB_ROOT/$NAME-conns.jsonl" "$@" &
PROXY=$!
sleep 0.5

lab_start "$F" -modelpeer=127.0.0.1:"$PPORT" || exit 1
SUMMARY=$(lab_fetch "$F" "$URI" --timeout "${EXP_TIMEOUT:-3600}" --trace "$LAB_ROOT/$NAME-trace.jsonl")
RC=$?

# grep -c prints 0 AND exits 1 on no match, so `|| echo 0` would yield "0\n0".
CONNS=$(grep -c '"conn"' "$LAB_ROOT/$NAME-conns.jsonl" 2>/dev/null); CONNS=${CONNS:-0}
DROPS=$(grep -c '"dropped": true' "$LAB_ROOT/$NAME-conns.jsonl" 2>/dev/null); DROPS=${DROPS:-0}
python3 -I - "$SUMMARY" "$NAME" "$RTT" "$MBIT" "$CONNS" "$DROPS" "$URI" "$*" >"$LAB_ROOT/results/$NAME.json" <<'EOF'
import json, sys
s = json.loads(sys.argv[1]) if sys.argv[1].strip().startswith("{") else {"raw": sys.argv[1]}
s.update({"name": sys.argv[2], "rtt_ms": float(sys.argv[3]), "mbit": float(sys.argv[4]),
          "proxy_connections": int(sys.argv[5]), "proxy_drops": int(sys.argv[6]),
          "uri": sys.argv[7], "chaos": sys.argv[8]})
print(json.dumps(s))
EOF
cat "$LAB_ROOT/results/$NAME.json"

kill "$PROXY" 2>/dev/null
lab_stop "$F"; lab_stop "$H"
exit $RC
