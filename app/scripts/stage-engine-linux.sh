#!/usr/bin/env bash
# Linux x86_64: populate src-tauri/resources/engine/ from pinned upstream
# releases. The macOS script (stage-engine.sh) hands over to this one on Linux.
#
#   scripts/stage-engine.sh            # uses the cache, downloads only what is missing
#   AINODE_STAGE_CACHE=/some/dir scripts/stage-engine.sh
#
# Same rules as on the Mac (ENGINE-CONTRACT.md "Bundled resources"): every
# archive is checked against the sha256 pinned below BEFORE anything is
# extracted from it, a mismatch stops the script, and MANIFEST.json records
# where each staged file came from and the hash of the bytes we ship.
#
# What differs on Linux:
#   - the helper is upstream's own file, unmodified, from upstream's Linux
#     archive (on the Mac we build it, upstream publishes no Mac archive).
#   - it links the OpenSSL 3.5, SQLite, libgomp, zlib and zstd its release
#     ships in lib/. Those go to engine/lib/, and the app starts the helper
#     with LD_LIBRARY_PATH pointing there, exactly as upstream's own
#     bin/btx-modeld wrapper does (engine.rs helper_env). Its RUNPATH
#     ($ORIGIN/../lib) fits upstream's layout, not ours; LD_LIBRARY_PATH wins.
#   - NEW with v0.34.16: upstream no longer ships libstdc++ and libgcc_s. The
#     helper takes them from the computer and needs GLIBCXX_3.4.32 and
#     CXXABI_1.3.15 (measured on the archive, 2026-11-23). Ubuntu 24.04 and
#     Debian 13 have them. The .deb says so (tauri.linux.conf.json).
#   - no OpenSSL tool any more: since v0.34.16 the helper mints its ML-DSA-44
#     identity in-process (btxchain/btx#228).
#   - llama.cpp's Vulkan build: the CPU build plus a GPU backend loaded at
#     run time, so a computer without Vulkan still chats on the CPU.
#
# Network: only the github.com release assets below, and only when they are
# not already in the cache.
set -euo pipefail

APP_DIR=$(cd "$(dirname "$0")/.." && pwd)
# AINODE_ENGINE_OUT: stage somewhere else (stage-engine-windows.sh takes the
# helper from here).
OUT=${AINODE_ENGINE_OUT:-"$APP_DIR/src-tauri/resources/engine"}

# ── Pins ────────────────────────────────────────────────────────────────────
# Upstream's v0.34.16 Linux archive. The sha256 is the line in upstream's
# SHA256SUMS on the release (not signed by upstream), checked against the
# downloaded file on 2026-11-23.
BTX_TAG=v0.34.16
BTX_ARCHIVE=btx-${BTX_TAG#v}-x86_64-linux-gnu.tar.gz
BTX_URL=https://github.com/btxchain/btx/releases/download/$BTX_TAG/$BTX_ARCHIVE
BTX_SHA256=9bbfff065299b3d6e863ecdeb0e54917788c1c0395cfa1d2b61bc40bf364932b
BTX_ROOT=btx-${BTX_TAG#v}
BTX_MEMBER=$BTX_ROOT/libexec/btx-modeld.real
# What the helper must find in the archive's lib/ (measured on the archive):
# a missing one means upstream changed its packaging; read before going on.
BTX_LIBS=(libssl.so.3 libcrypto.so.3 libsqlite3.so.0 libgomp.so.1 libz.so.1 libzstd.so.1)

LLAMA_TAG=b11559
LLAMA_ARCHIVE=llama-$LLAMA_TAG-bin-ubuntu-vulkan-x64.tar.gz
LLAMA_URL=https://github.com/ggml-org/llama.cpp/releases/download/$LLAMA_TAG/$LLAMA_ARCHIVE
LLAMA_SHA256=3c864ef7eaac254b179a52cfc648ea5b31bcea67e3523710acc6828f44e12d9e

# The newest glibc symbol version anything we stage may need: upstream's
# helper already needs 2.38 (Ubuntu 24.04, Debian 13 and newer).
GLIBC_MAX=2.38
# The newest libstdc++ symbol versions the helper and its lib/ may need from
# the computer: what Ubuntu 24.04's libstdc++6 (GCC 14) provides. The v0.34.16
# helper needs GLIBCXX_3.4.32 and CXXABI_1.3.15.
GLIBCXX_MAX=3.4.33
CXXABI_MAX=1.3.15
# Libraries a staged binary may take from the computer rather than the
# bundle: glibc's; the C++ runtime every desktop has; OpenSSL 3, which every
# distribution with glibc 2.38 ships (llama.cpp links it for its own HTTPS
# features and needs only OPENSSL_3.0.0 symbols, checked below); and the
# Vulkan loader (only the optional GPU backend links it; without it llama.cpp
# skips that backend and runs on the CPU).
SYSTEM_LIBS='^(libc\.so\.6|libm\.so\.6|libdl\.so\.2|libpthread\.so\.0|librt\.so\.1|ld-linux-x86-64\.so\.2|libstdc\+\+\.so\.6|libgcc_s\.so\.1|libssl\.so\.3|libcrypto\.so\.3|libvulkan\.so\.1)$'

# ── Cache ───────────────────────────────────────────────────────────────────
CACHE=${AINODE_STAGE_CACHE:-"${XDG_CACHE_HOME:-$HOME/.cache}/ainode-stage"}
mkdir -p "$CACHE"

say() { printf '%s\n' "$*" >&2; }
die() { say "stage-engine: $*"; exit 1; }
sha256() { sha256sum "$1" | awk '{print $1}'; }

[ "$(uname -s)-$(uname -m)" = "Linux-x86_64" ] || die "Linux x86_64 only"
for t in curl sha256sum tar readelf objdump jq ss; do
  command -v "$t" >/dev/null || die "missing tool: $t"
done

verify() {  # verify FILE SHA256
  local got; got=$(sha256 "$1")
  [ "$got" = "$2" ] || die "sha256 MISMATCH for $1: got $got, pinned $2. Refusing to use it."
}

# fetch NAME URL SHA256 -> prints the path of a verified archive.
fetch() {
  local name=$1 url=$2 want=$3 f
  case $url in https://github.com/*/releases/download/*) ;; *) die "refusing non-release URL $url" ;; esac
  f=$(find -L "$CACHE" -maxdepth 3 -type f -name "$name" 2>/dev/null | head -1 || true)
  if [ -z "$f" ]; then
    mkdir -p "$CACHE/archives"
    f="$CACHE/archives/$name"
    say "downloading $url"
    curl -fL --proto '=https' --tlsv1.2 --retry 3 -o "$f.part" "$url"
    mv "$f.part" "$f"
  else
    say "cached: $f"
  fi
  verify "$f" "$want"
  printf '%s\n' "$f"
}

WORK=$(mktemp -d "${TMPDIR:-/tmp}/ainode-stage.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
STAGE="$WORK/engine"
mkdir -p "$STAGE/llama" "$STAGE/lib"
MANIFEST_LINES="$WORK/manifest.jsonl"
: >"$MANIFEST_LINES"

# record PATH_IN_ENGINE SOURCE_URL SOURCE_SHA256 HOW
record() {
  jq -cn --arg path "$1" --arg url "$2" --arg src "$3" --arg how "$4" \
     --arg sha "$(sha256 "$STAGE/$1")" --argjson size "$(stat -c %s "$STAGE/$1")" \
     '{path:$path, source_url:$url, source_sha256:$src, how:$how, sha256:$sha, size:$size}' >>"$MANIFEST_LINES"
}

needed() { readelf -d "$1" | awk '/\(NEEDED\)/ {gsub(/[][]/, "", $NF); print $NF}'; }

# copy_closure FROM_DIR TO_DIR FILE...: copy into TO_DIR every library FILE
# needs (transitively) that FROM_DIR has, dereferenced under the exact name
# the loader asks for. Anything else must be a SYSTEM_LIBS name.
copy_closure() {
  local from=$1 to=$2; shift 2
  local queue=("$@") bin dep
  while [ ${#queue[@]} -gt 0 ]; do
    bin=${queue[0]}; queue=("${queue[@]:1}")
    while IFS= read -r dep; do
      [ -f "$to/$dep" ] && continue
      if [ -e "$from/$dep" ]; then
        cp -L "$from/$dep" "$to/$dep"
        chmod 755 "$to/$dep"
        queue+=("$to/$dep")
      elif ! printf '%s\n' "$dep" | grep -qE "$SYSTEM_LIBS"; then
        die "$(basename "$bin") needs $dep: not in the archive and not a system library every desktop has"
      fi
    done < <(needed "$bin")
  done
}

# ── btx-modeld + its libraries ──────────────────────────────────────────────
btx=$(fetch "$BTX_ARCHIVE" "$BTX_URL" "$BTX_SHA256")
mkdir -p "$WORK/btx"
tar -xzf "$btx" -C "$WORK/btx" "$BTX_MEMBER" "$BTX_ROOT/lib"
cp "$WORK/btx/$BTX_MEMBER" "$STAGE/btx-modeld"
chmod 755 "$STAGE/btx-modeld"
[ "$(sha256 "$STAGE/btx-modeld")" = "$(sha256 "$WORK/btx/$BTX_MEMBER")" ] || die "btx-modeld changed while staging"
record btx-modeld "$BTX_URL" "$BTX_SHA256" "libexec/btx-modeld.real from the archive, bytes unmodified"
copy_closure "$WORK/btx/$BTX_ROOT/lib" "$STAGE/lib" "$STAGE/btx-modeld"
for l in "${BTX_LIBS[@]}"; do
  [ -f "$STAGE/lib/$l" ] || die "the helper's closure from the archive's lib/ has no $l"
done
for f in "$STAGE"/lib/*; do
  record "lib/$(basename "$f")" "$BTX_URL" "$BTX_SHA256" "lib/$(basename "$f") from the archive, bytes unmodified"
done

# ── licence texts for the helper and the libraries beside it ────────────────
# Upstream's archive carries none for them, so they come from
# scripts/licenses (sources and checks: PROVENANCE.md there).
. "$APP_DIR/scripts/licenses/stage.sh"
stage_licenses toplevel common linux-libs

# ── llama.cpp llama-server ──────────────────────────────────────────────────
llama=$(fetch "$LLAMA_ARCHIVE" "$LLAMA_URL" "$LLAMA_SHA256")
mkdir -p "$WORK/llama"
tar -xzf "$llama" -C "$WORK/llama"
LSRC="$WORK/llama/llama-$LLAMA_TAG"
[ -x "$LSRC/llama-server" ] || die "llama-server not in $LLAMA_ARCHIVE"
cp "$LSRC/llama-server" "$STAGE/llama/llama-server"
chmod 755 "$STAGE/llama/llama-server"
[ -f "$LSRC/LICENSE" ] && cp "$LSRC/LICENSE" "$STAGE/llama/LICENSE"
# The backends are not linked: llama.cpp loads libggml-<backend>.so from its
# own directory at run time (one CPU build per instruction set, picked for
# this processor, and Vulkan). Every CPU variant and Vulkan; not the RPC
# backend, which only talks to remote machines.
for f in "$LSRC"/libggml-cpu-*.so "$LSRC"/libggml-vulkan.so; do
  [ -e "$f" ] || die "missing backend $(basename "$f") in $LLAMA_ARCHIVE"
  cp -L "$f" "$STAGE/llama/$(basename "$f")"
  chmod 755 "$STAGE/llama/$(basename "$f")"
done
# ggml needs OpenMP (libgomp.so.1), which llama.cpp's release does not ship
# and a minimal install may lack. Every llama library has RUNPATH $ORIGIN, so
# the copy upstream BTX ships for its helper (pinned above), placed next to
# them, is the one they load.
cp "$STAGE/lib/libgomp.so.1" "$STAGE/llama/libgomp.so.1"
copy_closure "$LSRC" "$STAGE/llama" "$STAGE/llama/llama-server" "$STAGE"/llama/libggml-*.so
for f in "$STAGE"/llama/*; do
  rel=${f#"$STAGE/"}
  case $rel in
    llama/LICENSE) record "$rel" "$LLAMA_URL" "$LLAMA_SHA256" "LICENSE from the archive" ;;
    llama/libgomp.so.1) record "$rel" "$BTX_URL" "$BTX_SHA256" "lib/libgomp.so.1 from the BTX archive (OpenMP for ggml), bytes unmodified" ;;
    *) record "$rel" "$LLAMA_URL" "$LLAMA_SHA256" "from the archive (symlinks dereferenced), bytes unmodified" ;;
  esac
done

# ── Install + prove ─────────────────────────────────────────────────────────
# `engine` is what the app shows in Status ("BTX v0.34.16 engine, upstream's
# file unmodified"): the tag pinned above (engine.rs `bundled`).
jq -s --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg btx "$BTX_TAG" \
  '{schema:1, platform:"linux-x86_64", generated_at:$at, generator:"app/scripts/stage-engine-linux.sh", engine:{btx_version:$btx, helper:"upstream-unmodified"}, files:.}' \
  "$MANIFEST_LINES" >"$STAGE/MANIFEST.json"

mkdir -p "$OUT"
find "$OUT" -mindepth 1 -maxdepth 1 ! -name .keep -exec rm -rf {} +
cp -R "$STAGE"/. "$OUT"/

# Every staged ELF needs only the bundle (its own dir; lib/ for the helper)
# and SYSTEM_LIBS, and no glibc newer than GLIBC_MAX.
check_links() {  # check_links FILE LIBDIR
  local f=$1 dir=$2 dep v
  while IFS= read -r dep; do
    [ -f "$dir/$dep" ] && continue
    printf '%s\n' "$dep" | grep -qE "$SYSTEM_LIBS" || die "$(basename "$f") links $dep, outside the bundle and the system"
  done < <(needed "$f")
  v=$(objdump -T "$f" | { grep -oE 'GLIBC_[0-9]+\.[0-9]+' || true; } | sed 's/GLIBC_//' | sort -uV | tail -1)
  [ -z "$v" ] || [ "$(printf '%s\n%s\n' "$v" "$GLIBC_MAX" | sort -V | tail -1)" = "$GLIBC_MAX" ] \
    || die "$(basename "$f") needs glibc $v, newer than $GLIBC_MAX"
  # The computer's libstdc++: nothing newer than Ubuntu 24.04's.
  v=$(objdump -T "$f" | { grep -oE 'GLIBCXX_[0-9]+\.[0-9]+(\.[0-9]+)?' || true; } | sed 's/GLIBCXX_//' | sort -uV | tail -1)
  [ -z "$v" ] || [ "$(printf '%s\n%s\n' "$v" "$GLIBCXX_MAX" | sort -V | tail -1)" = "$GLIBCXX_MAX" ] \
    || die "$(basename "$f") needs GLIBCXX_$v, newer than $GLIBCXX_MAX"
  v=$(objdump -T "$f" | { grep -oE 'CXXABI_[0-9]+\.[0-9]+(\.[0-9]+)?' || true; } | sed 's/CXXABI_//' | sort -uV | tail -1)
  [ -z "$v" ] || [ "$(printf '%s\n%s\n' "$v" "$CXXABI_MAX" | sort -V | tail -1)" = "$CXXABI_MAX" ] \
    || die "$(basename "$f") needs CXXABI_$v, newer than $CXXABI_MAX"
  # The system's OpenSSL 3 may be any 3.x: nothing newer than 3.0.0's symbols.
  if [ ! -f "$dir/libssl.so.3" ]; then
    v=$(objdump -T "$f" | { grep -oE 'OPENSSL_[0-9.]+' || true; } | sort -uV | tail -1)
    [ -z "$v" ] || [ "$v" = OPENSSL_3.0.0 ] || die "$(basename "$f") needs $v from the system's OpenSSL, newer than 3.0.0"
  fi
}
check_links "$OUT/btx-modeld" "$OUT/lib"
for f in "$OUT"/lib/*; do check_links "$f" "$OUT/lib"; done
for f in "$OUT/llama/llama-server" "$OUT"/llama/*.so*; do check_links "$f" "$OUT/llama"; done
say "verified: every staged binary links only its bundle dir and glibc/libstdc++ (glibc <= $GLIBC_MAX, GLIBCXX <= $GLIBCXX_MAX, CXXABI <= $CXXABI_MAX)"

# Run the STAGED llama-server from an unrelated cwd with an empty environment,
# and check every llama/ggml library the loader opened came from the bundle.
(cd / && env -i LD_DEBUG=libs "$OUT/llama/llama-server" --version) >"$WORK/version.txt" 2>"$WORK/loaded.txt" \
  || { tail -5 "$WORK/loaded.txt" >&2; die "staged llama-server --version failed"; }
version_line=$(grep -h '^version:' "$WORK/version.txt" "$WORK/loaded.txt" | head -1 || true)
[ -n "$version_line" ] || die "llama-server printed no version"
outside=$(grep -oE 'calling init: [^ ]*lib(llama|ggml|mtmd)[^ ]*' "$WORK/loaded.txt" | grep -v "$OUT/llama/" || true)
[ -z "$outside" ] || die "llama-server loaded libraries from outside the bundle: $outside"
say "verified: staged llama-server runs from / ($version_line), its llama/ggml libraries all from $OUT/llama/"

# Start the STAGED helper exactly as the app does (the same allowlisted
# environment, LD_LIBRARY_PATH = lib/, no OpenSSL tool anywhere, the socket
# in a folder of its own that is 0700) on a throwaway modeldir: no peers, no
# listener. It must mint its ML-DSA-44 identity in-process, print "PQ1 ready"
# and open no listening socket. This also proves that THIS computer's
# libstdc++ is new enough for the helper.
probe="$WORK/probe"
mkdir -p "$probe" "$WORK/psock"
chmod 700 "$WORK/psock"
probe_sock="$WORK/psock/p.sock"
env -i HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  LD_LIBRARY_PATH="$OUT/lib" "$OUT/btx-modeld" \
  -modeldir="$probe/md" -modelstorage=1GiB -modelseed=manual -modelfollowpeers=0 \
  -modelrpcsocket="$probe_sock" >"$probe/out" 2>"$probe/err" &
probe_pid=$!
for _ in $(seq 1 600); do
  grep -q "PQ1 ready" "$probe/out" 2>/dev/null && [ -S "$probe_sock" ] && break
  kill -0 "$probe_pid" 2>/dev/null || break
  sleep 0.1
done
# Listening TCP or any UDP socket of the helper (ss shows the owning pid).
# Fail closed: the helper must still be running, and ss must run without
# an error.
probe_alive=0; kill -0 "$probe_pid" 2>/dev/null && probe_alive=1
ss_ok=1
ss_out=$(ss -Hlntup 2>"$probe/ss.err") || ss_ok=0
listening=$(printf '%s\n' "$ss_out" | grep -c "pid=$probe_pid," || true)
kill "$probe_pid" 2>/dev/null || true
for _ in $(seq 1 30); do kill -0 "$probe_pid" 2>/dev/null || break; sleep 0.1; done
kill -KILL "$probe_pid" 2>/dev/null || true
wait "$probe_pid" 2>/dev/null || true
grep -q "PQ1 ready" "$probe/out" || die "staged btx-modeld did not start with no OpenSSL tool: $(head -1 "$probe/err")"
[ -s "$probe/md/tls/cert.pem" ] || die "staged btx-modeld started but wrote no TLS cert"
[ "$probe_alive" = 1 ] || die "staged btx-modeld had exited before its sockets could be checked"
[ "$ss_ok" = 1 ] || die "ss failed, so the helper's sockets were not checked: $(head -1 "$probe/ss.err")"
[ "$listening" = 0 ] || die "staged btx-modeld opened $listening listening or UDP sockets with the app's flags; it must open none"
say "verified: staged btx-modeld starts with no OpenSSL tool, mints its identity, opens no listening socket"

say "staged into $OUT ($(du -sh "$OUT" | cut -f1)):"
(cd "$OUT" && find . -type f ! -name .keep | sort | sed 's|^\./|  |') >&2
