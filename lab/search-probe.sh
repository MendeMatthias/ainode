#!/usr/bin/env bash
# What an AInode's Network search sees from the public host (plan 3.5).
#
#   lab/search-probe.sh [HOST:PORT] [TEXT]
#
# Starts a scratch client helper the way the app does (the app's staged engine,
# download only: -modelseed=manual -modelfollowpeers=0, the host as its one
# peer), asks `searchmodels` with scope NETWORK, then again after
# `addmodelindex HOST:PORT`, prints both, stops the helper and deletes its
# scratch folder. Nothing is fetched. Needs app/src-tauri/resources/engine
# (app/scripts/stage-engine.sh).
set -euo pipefail
HOST=${1:-20.196.8.98:29447}
TEXT=${2:-granite}
HERE="$(cd "$(dirname "$0")" && pwd)"
ENGINE="$HERE/../app/src-tauri/resources/engine"
[[ -x "$ENGINE/btx-modeld" ]] || { echo "error: no staged engine at $ENGINE" >&2; exit 1; }
DIR=$(mktemp -d)
SOCK="${TMPDIR%/}/ain-probe-$$.sock"
PID=""
cleanup() {
  [[ -n "$PID" ]] && { python3 -I "$HERE/rpc.py" "$SOCK" stop >/dev/null 2>&1 || kill "$PID" 2>/dev/null || true; }
  for _ in $(seq 1 30); do [[ -n "$PID" ]] && kill -0 "$PID" 2>/dev/null || break; sleep 0.2; done
  [[ -n "$PID" ]] && kill -9 "$PID" 2>/dev/null || true
  rm -f "$SOCK"
  rm -r "$DIR"
}
trap cleanup EXIT

BTX_OPENSSL="$ENGINE/openssl.sh" "$ENGINE/btx-modeld" -modeldir="$DIR/md" -modelstorage=1GiB \
  -modelseed=manual -modelfollowpeers=0 -modelrpcsocket="$SOCK" -modelpeer="$HOST" >"$DIR/out.log" 2>&1 &
PID=$!
for _ in $(seq 1 100); do [[ -S "$SOCK" ]] && break; sleep 0.1; done
[[ -S "$SOCK" ]] || { echo "no socket"; tail -5 "$DIR/out.log"; exit 1; }
rpc() { python3 -I "$HERE/rpc.py" "$SOCK" "$@"; }
# Give the PQ1 connection to the host a moment.
for _ in $(seq 1 30); do
  [[ $(rpc getmodelnetworkinfo | python3 -I -c 'import json,sys; print(json.load(sys.stdin).get("peers_connected",0))') -gt 0 ]] && break
  sleep 1
done
summary() {
  python3 -I -c '
import json, sys
d = json.load(sys.stdin)
c = d.get("coverage", {})
print("  coverage:", {k: c.get(k) for k in ("connected_peers_queried", "index_peers_configured", "index_peers_queried", "responses_received")})
for r in d.get("results", []):
    pub = r.get("publisher") or {}
    print("  -", r.get("name"), "|", pub.get("name") if isinstance(pub, dict) else pub, "|", (r.get("description") or "")[:70],
          "| signed" if (r.get("search") or {}).get("metadata_verified") else "| unsigned", "|", r.get("uri", "")[:24])
print("  results:", d.get("results_returned"))
'
}
Q="{\"text\":\"$TEXT\",\"scope\":\"NETWORK\",\"limit\":30}"
echo "== searchmodels NETWORK \"$TEXT\", host as a peer only"
rpc searchmodels "$Q" | summary
echo "== after addmodelindex $HOST"
rpc addmodelindex "\"$HOST\"" | python3 -I -c 'import json,sys; print("  ", json.load(sys.stdin))'
sleep 2
rpc searchmodels "$Q" | summary
