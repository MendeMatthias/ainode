# Source me:  source lab/env.sh
# Local lab for upstream btx-modeld v0.34.15 (macOS arm64 release archive).
#
# Expects (paths overridable):
#   LAB_ROOT     scratch root for helper state, logs, packages
#   BTX_ROOT     the extracted upstream archive (btx-0.34.15/)
#   BTX_OPENSSL  an OpenSSL >= 3.5 CLI (the stock macOS one is LibreSSL and cannot
#                mint the helper's ML-DSA-44 certificate; see docs/experiments.md E1)
#
# Sockets live under $TMPDIR because a unix socket path is capped at 104 bytes
# on macOS and the scratch paths are longer than that.

: "${LAB_ROOT:?set LAB_ROOT}"
: "${BTX_ROOT:?set BTX_ROOT}"
: "${BTX_OPENSSL:?set BTX_OPENSSL}"
export BTX_OPENSSL
LAB_PY="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
MODELD="$BTX_ROOT/bin/btx-modeld"

lab_sock() { echo "${TMPDIR%/}/ain-$1.sock"; }

# lab_start NAME [extra btx-modeld args...]
lab_start() {
  local name=$1; shift
  local dir="$LAB_ROOT/$name" sock; sock=$(lab_sock "$name")
  mkdir -p "$dir"; rm -f "$sock"
  nohup "$MODELD" -modeldir="$dir/md" -modelstorage="${LAB_QUOTA:-8GiB}" -modelseed=auto \
    -modelfollowpeers=0 -modelrpcsocket="$sock" "$@" >>"$dir/out.log" 2>&1 &
  echo $! >"$dir/pid"
  for _ in $(seq 1 50); do [ -S "$sock" ] && break; sleep 0.1; done
  [ -S "$sock" ] || { echo "lab_start $name: no socket"; tail -3 "$dir/out.log"; return 1; }
}

# lab_stop NAME [signal]: polite first, then SIGKILL after 5 s, because a helper
# with a wedged request ignores SIGTERM (docs/experiments.md E12).
lab_stop() {
  local dir="$LAB_ROOT/$1" pid
  [ -f "$dir/pid" ] || return 0
  pid=$(cat "$dir/pid")
  kill "-${2:-TERM}" "$pid" 2>/dev/null
  for _ in $(seq 1 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
  if kill -0 "$pid" 2>/dev/null; then kill -KILL "$pid" 2>/dev/null; wait "$pid" 2>/dev/null; fi
  rm -f "$dir/pid"
}

lab_wipe() { lab_stop "$1" KILL; rm -rf "${LAB_ROOT:?}/$1"; rm -f "$(lab_sock "$1")"; }

lab_rpc() { local name=$1; shift; python3 -I "$LAB_PY/rpc.py" "$(lab_sock "$name")" "$@"; }

lab_fetch() { local name=$1; shift; python3 -I "$LAB_PY/fetchrun.py" "$(lab_sock "$name")" "$@"; }
