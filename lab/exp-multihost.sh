#!/usr/bin/env bash
# E22: two hosts for one link. Does a fetcher use both, and does it finish from
# the second when the first dies mid-download?
#
#   exp-multihost.sh PKG_DIR MBIT RTT_MS [KILL_AT_FRACTION]
#
# Two fresh hosts import the SAME package (identical bytes give the identical
# link, E4), each behind its own WAN proxy with its own MBIT line. Three runs:
#   one   fetcher knows host 1 only (baseline)
#   two   fetcher knows both
#   kill  fetcher knows both; host 1 is SIGKILLed once KILL_AT_FRACTION
#         (default 0.3) of the bytes are committed
# Writes $LAB_ROOT/results/e22-*.json with the fetch summary plus the bytes each
# proxy carried to the fetcher.
#
# E22_RUNS picks runs (default "one two kill"; "two2 two3" repeats run two).
# FETCHER_MODELD runs the fetcher on another btx-modeld build (hosts stay the
# release), to test a client-side patch against unmodified hosts.
set -uo pipefail
PKG=$1 MBIT=$2 RTT=$3 KILL_AT=${4:-0.3}
source "$(dirname "$0")/env.sh"
mkdir -p "$LAB_ROOT/results"

down_bytes() { # sum of host->fetcher bytes in a proxy log
  python3 -I -c 'import json,sys; print(sum(json.loads(l).get("down",0) for l in open(sys.argv[1]) if l.strip()))' "$1" 2>/dev/null || echo 0
}

run() { # NAME PEERS(1|2) KILL(0|1)
  local name=$1 peers=$2 kill=$3
  local h1="e22$name-h1" h2="e22$name-h2" f="e22$name-f" p1 p2 uri summary
  for n in "$h1" "$h2" "$f"; do lab_wipe "$n"; done
  lab_start "$h1" -modelbind=127.0.0.1:29461 -modelhost=1 || return 1
  lab_start "$h2" -modelbind=127.0.0.1:29462 -modelhost=1 || return 1
  for n in "$h1" "$h2"; do
    uri=$(lab_rpc "$n" hostmodel "\"$PKG\"" '{"pin":true,"publish":false}' \
          | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["uri"])') || return 1
  done
  rm -f "$LAB_ROOT/e22$name-p1.jsonl" "$LAB_ROOT/e22$name-p2.jsonl"
  python3 -I "$LAB_PY/wanproxy.py" --listen 127.0.0.1:39461 --to 127.0.0.1:29461 \
    --rtt-ms "$RTT" --mbit "$MBIT" --log "$LAB_ROOT/e22$name-p1.jsonl" & p1=$!
  # E22_H2_ADDR=[::1] reaches host 2 over IPv6, so the two hosts are two
  # netgroups to the fetcher's picker, as two real hosts would be.
  local h2a=${E22_H2_ADDR:-127.0.0.1}
  python3 -I "$LAB_PY/wanproxy.py" --listen "$h2a":39462 --to 127.0.0.1:29462 \
    --rtt-ms "$RTT" --mbit "$MBIT" --log "$LAB_ROOT/e22$name-p2.jsonl" & p2=$!
  sleep 0.5
  local fm=${FETCHER_MODELD:-$MODELD}
  if [ "$peers" = 2 ]; then
    MODELD=$fm lab_start "$f" -modelpeer=127.0.0.1:39461 -modelpeer="$h2a":39462 || return 1
  else
    MODELD=$fm lab_start "$f" -modelpeer=127.0.0.1:39461 || return 1
  fi
  local trace="$LAB_ROOT/e22$name-trace.jsonl" total killed_at=""
  rm -f "$trace"  # a previous run's trace would trigger the kill at once
  total=$(du -k "$PKG"/* | awk '{s+=$1} END {print s*1024}')
  lab_fetch "$f" "$uri" --timeout 1800 --trace "$trace" > "$LAB_ROOT/e22$name.out" &
  local fp=$! t0=$SECONDS
  if [ "$kill" = 1 ]; then
    while kill -0 "$fp" 2>/dev/null; do
      # The trace may not exist yet; read it in one python, never a pipe that
      # pipefail can turn into "0\n0".
      local b; b=$(python3 -I -c '
import json, sys
try:
    lines = [l for l in open(sys.argv[1]) if l.strip()]
    print(json.loads(lines[-1]).get("bytes", 0) or 0 if lines else 0)
except (OSError, ValueError):
    print(0)' "$trace")
      if python3 -I -c 'import sys; sys.exit(0 if int(sys.argv[1]) >= float(sys.argv[2]) * int(sys.argv[3]) else 1)' "$b" "$KILL_AT" "$total"; then
        lab_stop "$h1" KILL; killed_at=$((SECONDS - t0)); echo "  host 1 killed at ${killed_at}s, $b bytes committed"; break
      fi
      sleep 0.5
    done
  fi
  wait "$fp"; summary=$(cat "$LAB_ROOT/e22$name.out")
  kill "$p1" "$p2" 2>/dev/null; wait "$p1" "$p2" 2>/dev/null
  python3 -I - "$summary" "$name" "$MBIT" "$RTT" "$(down_bytes "$LAB_ROOT/e22$name-p1.jsonl")" \
      "$(down_bytes "$LAB_ROOT/e22$name-p2.jsonl")" "${killed_at:-}" > "$LAB_ROOT/results/e22-$name.json" <<'EOF'
import json, sys
s = json.loads(sys.argv[1]) if sys.argv[1].strip().startswith("{") else {"raw": sys.argv[1]}
s.update({"run": sys.argv[2], "mbit_per_host": float(sys.argv[3]), "rtt_ms": float(sys.argv[4]),
          "via_host1_bytes": int(sys.argv[5]), "via_host2_bytes": int(sys.argv[6]),
          "host1_killed_at_s": int(sys.argv[7]) if sys.argv[7] else None})
print(json.dumps(s))
EOF
  cat "$LAB_ROOT/results/e22-$name.json"
  for n in "$f" "$h1" "$h2"; do lab_stop "$n"; done
}

RUNS=${E22_RUNS:-one two kill}
for r in $RUNS; do
  case "$r" in
    one)  echo "== one host";  run one 1 0 ;;
    two*) echo "== two hosts ($r)"; run "$r" 2 0 ;;
    kill*) echo "== two hosts, host 1 killed at ${KILL_AT} ($r)"; run "$r" 2 1 ;;
  esac
done
