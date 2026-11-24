#!/usr/bin/env bash
# Populate src-tauri/resources/engine/ from pinned upstream sources.
#
#   scripts/stage-engine.sh            # uses the cache, downloads and builds only what is missing
#   AINODE_STAGE_CACHE=/some/dir scripts/stage-engine.sh
#   AINODE_HELPER_DIR=/dir scripts/stage-engine.sh   # a folder build-engine-macos.sh wrote
#
# What ends up in resources/engine/ and why is ENGINE-CONTRACT.md "Bundled
# resources". Every archive is checked against the sha256 pinned below BEFORE
# anything is extracted from it, and a mismatch stops the script. MANIFEST.json
# records, per staged file, where it came from, the pinned hash, and the hash
# of the bytes we actually ship, so a reviewer can re-derive every file.
#
# The model helper (btx-modeld) is built here from upstream's tag, because
# upstream publishes no macOS archive since v0.34.16: build-engine-macos.sh,
# sources of the helper untouched, checked against the sha256 pinned below.
#
# Network: llama.cpp's github.com release asset, and what build-engine-macos.sh
# fetches when the helper is not in the cache yet (it says what).
set -euo pipefail

# Linux stages other archives with other checks (ELF, no codesign).
[ "$(uname -s)" = Linux ] && exec "$(dirname "$0")/stage-engine-linux.sh" "$@"

APP_DIR=$(cd "$(dirname "$0")/.." && pwd)
OUT="$APP_DIR/src-tauri/resources/engine"

# ── Pins ────────────────────────────────────────────────────────────────────
# The helper: upstream v0.34.16, built by scripts/build-engine-macos.sh (which
# pins the commit, the tree, OpenSSL and LLVM OpenMP itself). BTX_HELPER_SHA256
# is the sha256 of the file that script writes. It was measured on 2026-11-23:
# three runs from scratch in three different folders gave these bytes
# (lab/evidence/2026-11-23/modelnet-engine/prepare/).
#
# The hash holds for ONE toolchain, the one named in BTX_HELPER_TOOLCHAIN.
# Another Xcode or cmake gives other bytes from the same source. When that
# happens this script stops. Do not edit the pin to make it pass: run
# build-engine-macos.sh twice in two fresh folders, see that both runs agree,
# read the BUILD-MANIFEST.json, and change the pin and the toolchain line in
# a commit of their own.
BTX_TAG=v0.34.16
BTX_COMMIT=074dac167c26c1e091198f691ed5727817dba88a
BTX_HELPER_SHA256=258da6e4037a0493c128c4b674d348c4e0f1d7cbe30e41bfcbed0ee4b1ed0876
BTX_HELPER_TOOLCHAIN="macOS 27.2.1, Xcode 27.2 (Apple clang 21.0.0), cmake 4.4.2"

LLAMA_TAG=b11559
LLAMA_ARCHIVE=llama-$LLAMA_TAG-bin-macos-arm64.tar.gz
LLAMA_URL=https://github.com/ggml-org/llama.cpp/releases/download/$LLAMA_TAG/$LLAMA_ARCHIVE
LLAMA_SHA256=c6cfeaa709b8d2e47e8a50f6833d22071f7ee7079b0b1a6d704e482afe6ee2c6

# ── Cache ───────────────────────────────────────────────────────────────────
# A per-user cache, so a re-stage does not fetch 140 MB again. Point
# AINODE_STAGE_CACHE at any directory that already holds the pinned archives
# (searched three levels deep, symlinks followed); every archive is verified
# against its pinned sha256 wherever it came from.
CACHE=${AINODE_STAGE_CACHE:-"$HOME/Library/Caches/ainode-stage"}
mkdir -p "$CACHE"

say() { printf '%s\n' "$*" >&2; }
die() { say "stage-engine: $*"; exit 1; }
sha256() { shasum -a 256 "$1" | awk '{print $1}'; }

[ "$(uname -s)-$(uname -m)" = "Darwin-arm64" ] || die "macOS arm64 only (ENGINE-CONTRACT.md rule 5)"
for t in curl shasum tar otool codesign install_name_tool jq lsof; do
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
  # -L / symlinks too, so a cache can be a dir of links to archives kept elsewhere.
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
mkdir -p "$STAGE/llama"
MANIFEST_LINES="$WORK/manifest.jsonl"
: >"$MANIFEST_LINES"

# record PATH_IN_ENGINE SOURCE_URL SOURCE_SHA256 HOW
record() {
  jq -cn --arg path "$1" --arg url "$2" --arg src "$3" --arg how "$4" \
     --arg sha "$(sha256 "$STAGE/$1")" --argjson size "$(stat -f %z "$STAGE/$1")" \
     '{path:$path, source_url:$url, source_sha256:$src, how:$how, sha256:$sha, size:$size}' >>"$MANIFEST_LINES"
}

# ── btx-modeld ──────────────────────────────────────────────────────────────
# A folder with btx-modeld and BUILD-MANIFEST.json, as build-engine-macos.sh
# writes it: AINODE_HELPER_DIR if given, else the cache, else built now
# (several minutes the first time, then cached).
helper_dir=${AINODE_HELPER_DIR:-"$CACHE/helper/$BTX_COMMIT"}
if [ ! -f "$helper_dir/btx-modeld" ] || [ ! -f "$helper_dir/BUILD-MANIFEST.json" ]; then
  [ -z "${AINODE_HELPER_DIR:-}" ] || die "AINODE_HELPER_DIR=$helper_dir has no btx-modeld and BUILD-MANIFEST.json"
  say "no built helper in the cache: building $BTX_TAG with scripts/build-engine-macos.sh (several minutes)"
  rm -rf "$helper_dir.part"
  "$APP_DIR/scripts/build-engine-macos.sh" --out "$helper_dir.part" --cache "$CACHE" \
    || die "build-engine-macos.sh failed; nothing staged"
  mkdir -p "$(dirname "$helper_dir")"
  rm -rf "$helper_dir"
  mv "$helper_dir.part" "$helper_dir"
else
  say "built helper: $helper_dir"
fi
hm="$helper_dir/BUILD-MANIFEST.json"
[ "$(jq -r '.upstream.commit' "$hm")" = "$BTX_COMMIT" ] \
  || die "the helper in $helper_dir was built from $(jq -r '.upstream.commit' "$hm"), pinned $BTX_COMMIT"
[ "$(jq -r '.serial_prover_option_used' "$hm")" = false ] \
  || die "the helper in $helper_dir was built with BTX_ALLOW_SERIAL_PROVER; upstream marks that build 'do not ship'"
[ "$(jq -r '.result.sha256' "$hm")" = "$(sha256 "$helper_dir/btx-modeld")" ] \
  || die "$helper_dir/btx-modeld is not the file its BUILD-MANIFEST.json describes; delete $helper_dir to rebuild"
got=$(sha256 "$helper_dir/btx-modeld")
if [ "$got" != "$BTX_HELPER_SHA256" ]; then
  say "stage-engine: the built helper is not the pinned one."
  say "  built:   $got"
  say "  pinned:  $BTX_HELPER_SHA256"
  say "  the pin holds for: $BTX_HELPER_TOOLCHAIN"
  say "  this build used:   macOS $(jq -r '.toolchain.macos' "$hm"), $(jq -r '.toolchain.xcode' "$hm")($(jq -r '.toolchain.clang' "$hm")), $(jq -r '.toolchain.cmake' "$hm")"
  say "  Same source, another toolchain, other bytes: that is expected, and it is why this stops."
  say "  The build is kept in $helper_dir (delete that folder to build again)."
  say "  Build twice in two fresh folders (scripts/build-engine-macos.sh --out A, --out B), compare,"
  say "  read the manifest, then change BTX_HELPER_SHA256 and BTX_HELPER_TOOLCHAIN in a commit of their own."
  die "refusing to stage an unpinned helper"
fi
cp "$helper_dir/btx-modeld" "$STAGE/btx-modeld"
chmod 755 "$STAGE/btx-modeld"
cp "$hm" "$STAGE/btx-modeld.BUILD-MANIFEST.json"
# The two licence texts the build wrote from its own pinned sources must be
# the texts kept in scripts/licenses (the ones Linux and Windows stage).
. "$APP_DIR/scripts/licenses/stage.sh"
for l in LICENSE-btx.txt LICENSE-openssl.txt; do
  [ -s "$helper_dir/$l" ] || die "$helper_dir has no $l; delete it to rebuild with the current build-engine-macos.sh"
  cmp -s "$helper_dir/$l" "$(license_src "$l")" || die "$helper_dir/$l differs from scripts/licenses/$l"
  cp "$helper_dir/$l" "$STAGE/$l"
done
# The linker signs it ad hoc. A signature that does not verify means the
# bytes are not what the build wrote: stop, never re-sign here.
codesign -v "$STAGE/btx-modeld" || die "btx-modeld's ad-hoc signature does not verify; refusing to ship it"
[ "$(sha256 "$STAGE/btx-modeld")" = "$BTX_HELPER_SHA256" ] || die "btx-modeld changed while staging"
record btx-modeld "https://github.com/btxchain/btx/tree/$BTX_COMMIT" "" \
  "built by app/scripts/build-engine-macos.sh from upstream $BTX_TAG ($BTX_COMMIT), the helper's sources untouched; every input and deviation is in btx-modeld.BUILD-MANIFEST.json; pinned sha256 $BTX_HELPER_SHA256 for $BTX_HELPER_TOOLCHAIN"
record btx-modeld.BUILD-MANIFEST.json "" "" "written by app/scripts/build-engine-macos.sh for the btx-modeld beside it"
record LICENSE-btx.txt "https://github.com/btxchain/btx/tree/$BTX_COMMIT" "" "COPYING from upstream's tree at that commit (MIT)"
record LICENSE-openssl.txt "" "" "LICENSE.txt from the OpenSSL release the helper links statically (Apache-2.0); version and sha256 in btx-modeld.BUILD-MANIFEST.json"
# The libraries from BTX's own tree that are compiled into the helper.
stage_licenses common

# ── llama.cpp llama-server ──────────────────────────────────────────────────
llama=$(fetch "$LLAMA_ARCHIVE" "$LLAMA_URL" "$LLAMA_SHA256")
mkdir -p "$WORK/llama"
tar -xzf "$llama" -C "$WORK/llama"
LSRC="$WORK/llama/llama-$LLAMA_TAG"
[ -x "$LSRC/llama-server" ] || die "llama-server not in $LLAMA_ARCHIVE"

# Copy llama-server plus the transitive closure of its @rpath dylibs, nothing
# else (the archive carries ~25 other tools). Each dylib is copied
# dereferenced under the exact name the loader asks for (libllama.0.dylib),
# so the bundle holds no symlinks.
cp "$LSRC/llama-server" "$STAGE/llama/llama-server"
cp "$LSRC/LICENSE" "$STAGE/llama/LICENSE"
queue=("$STAGE/llama/llama-server")
while [ ${#queue[@]} -gt 0 ]; do
  bin=${queue[0]}; queue=("${queue[@]:1}")
  while IFS= read -r dep; do
    case $dep in
      /usr/lib/*|/System/*) continue ;;
      @rpath/*)
        name=${dep#@rpath/}
        [ -f "$STAGE/llama/$name" ] && continue
        [ -e "$LSRC/$name" ] || die "$(basename "$bin") needs $name, not in the archive"
        cp -L "$LSRC/$name" "$STAGE/llama/$name"
        chmod 755 "$STAGE/llama/$name"
        queue+=("$STAGE/llama/$name")
        ;;
      *) die "$(basename "$bin") links $dep, which would not exist on a user's Mac" ;;
    esac
  done < <(otool -L "$bin" | awk 'NR>1 {print $1}' | grep -v "^@rpath/$(basename "$bin")$" || true)
done

# The release binaries already carry LC_RPATH @loader_path, so they find their
# dylibs next to themselves. Add one only if a future build stops doing that.
lc_rpaths() { otool -l "$1" | awk '/cmd LC_RPATH/ {r=1} r && /path / {print $2; r=0}'; }
for f in "$STAGE"/llama/llama-server "$STAGE"/llama/*.dylib; do
  if ! lc_rpaths "$f" | grep -qxE '@loader_path|@executable_path'; then
    install_name_tool -add_rpath @loader_path "$f"
    say "added LC_RPATH @loader_path to $(basename "$f")"
  fi
done
for f in "$STAGE"/llama/llama-server "$STAGE"/llama/*.dylib; do
  codesign -s - -f "$f"
done
for f in "$STAGE"/llama/*; do
  rel=${f#"$STAGE/"}
  case $rel in
    llama/LICENSE) record "$rel" "$LLAMA_URL" "$LLAMA_SHA256" "LICENSE from the archive" ;;
    *) record "$rel" "$LLAMA_URL" "$LLAMA_SHA256" "from the archive (symlinks dereferenced), re-signed ad hoc" ;;
  esac
done

# ── Install + prove ─────────────────────────────────────────────────────────
# `engine` is what the app shows in Status ("BTX v0.34.16 engine, built by
# AInode from upstream's source"): the tag pinned above, never a second copy
# kept by hand in the interface (engine.rs `bundled`, a test reads this line).
jq -s --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg btx "$BTX_TAG" \
  '{schema:1, generated_at:$at, generator:"app/scripts/stage-engine.sh", engine:{btx_version:$btx, helper:"built-by-ainode"}, files:.}' \
  "$MANIFEST_LINES" >"$STAGE/MANIFEST.json"

find "$OUT" -mindepth 1 -maxdepth 1 ! -name .keep -exec rm -rf {} +
cp -R "$STAGE"/. "$OUT"/

# Run the STAGED llama-server from an unrelated cwd with no DYLD_* help, and
# check that every dylib it loaded came from resources/engine/llama/ (not the
# extracted archive, not some Homebrew copy).
loaded="$WORK/loaded.txt"
(cd / && env -u DYLD_LIBRARY_PATH -u DYLD_FALLBACK_LIBRARY_PATH DYLD_PRINT_LIBRARIES=1 \
   "$OUT/llama/llama-server" --version) >"$WORK/version.txt" 2>"$loaded" \
  || { cat "$loaded" >&2; die "staged llama-server --version failed"; }
version_line=$(grep -h '^version:' "$WORK/version.txt" "$loaded" | head -1)
[ -n "$version_line" ] || die "llama-server printed no version"
outside=$(grep -E 'lib(llama|ggml|mtmd)' "$loaded" | grep -v "$OUT/llama/" || true)
[ -z "$outside" ] || die "llama-server loaded dylibs from outside the bundle: $outside"
say "verified: staged llama-server runs from / ($version_line), all $(grep -cE 'lib(llama|ggml|mtmd)' "$loaded") llama/ggml dylibs loaded from $OUT/llama/"

# Every staged Mach-O may link only the OS (/usr/lib, /System) and, through
# @rpath, files that sit next to it in the bundle. Anything else would be a
# library the user's Mac does not have (or one a stranger could plant).
check_links() {  # check_links FILE
  local f=$1 dir dep
  dir=$(dirname "$f")
  while IFS= read -r dep; do
    case $dep in
      /usr/lib/*|/System/*) ;;
      @rpath/*) [ -f "$dir/${dep#@rpath/}" ] || die "$(basename "$f") links $dep, not in $dir" ;;
      *) die "$(basename "$f") links $dep, outside the OS and the bundle" ;;
    esac
  done < <(otool -L "$f" | awk 'NR>1 {print $1}')
}
for f in "$OUT/btx-modeld" "$OUT/llama/llama-server" "$OUT"/llama/*.dylib; do
  check_links "$f"
done
say "verified: every staged binary links only /usr/lib, /System and its own bundle dir"

# Start the STAGED helper exactly as the app does (the same allowlisted
# environment, no OpenSSL tool anywhere, the socket in a folder of its own
# that is 0700) on a throwaway modeldir: no peers, no listener. It must mint
# its ML-DSA-44 identity in-process and print "PQ1 ready", or the app would
# ship a helper that exits 2 on every user's first start.
probe="$WORK/probe"
mkdir -p "$probe"
probe_dir=$(mktemp -d "${TMPDIR:-/tmp}/ainode-stage-probe.XXXXXX")
chmod 700 "$probe_dir"
probe_sock="$probe_dir/s.sock"
env -i HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  "$OUT/btx-modeld" \
  -modeldir="$probe/md" -modelstorage=1GiB -modelseed=manual -modelfollowpeers=0 \
  -modelrpcsocket="$probe_sock" >"$probe/out" 2>"$probe/err" &
probe_pid=$!
for _ in $(seq 1 600); do
  grep -q "PQ1 ready" "$probe/out" 2>/dev/null && [ -S "$probe_sock" ] && break
  kill -0 "$probe_pid" 2>/dev/null || break
  sleep 0.1
done
# Fail closed: the helper must still be running, lsof must be able to read
# its open files at all, and must not complain. (lsof exits 1 when a process
# simply has no network file, so its exit code alone says nothing.)
probe_alive=0; kill -0 "$probe_pid" 2>/dev/null && probe_alive=1
lsof_sees=$(lsof -nP -p "$probe_pid" 2>/dev/null | grep -c . || true)
lsof_net=$(lsof -nP -a -p "$probe_pid" -i 2>"$probe/lsof.err" || true)
listening=$(printf '%s\n' "$lsof_net" | grep -cE 'LISTEN|UDP' || true)
kill "$probe_pid" 2>/dev/null || true
for _ in $(seq 1 30); do kill -0 "$probe_pid" 2>/dev/null || break; sleep 0.1; done
kill -KILL "$probe_pid" 2>/dev/null || true
wait "$probe_pid" 2>/dev/null || true
rm -rf "$probe_dir"
grep -q "PQ1 ready" "$probe/out" || die "staged btx-modeld did not start with no OpenSSL tool: $(head -1 "$probe/err")"
[ -s "$probe/md/tls/cert.pem" ] || die "staged btx-modeld started but wrote no TLS cert"
[ "$probe_alive" = 1 ] || die "staged btx-modeld had exited before its sockets could be checked"
[ "$lsof_sees" -gt 1 ] || die "lsof could not read the open files of the staged btx-modeld; its sockets were not checked"
[ ! -s "$probe/lsof.err" ] || die "lsof complained while the helper's sockets were checked: $(head -1 "$probe/lsof.err")"
[ "$listening" = 0 ] || die "staged btx-modeld opened $listening listening or UDP sockets with the app's flags; it must open none"
say "verified: staged btx-modeld starts with no OpenSSL tool, mints its identity, opens no listening socket"

for f in "$OUT/btx-modeld" "$OUT/llama/llama-server"; do
  codesign -v "$f" || die "signature does not verify: $f"
done
say "staged into $OUT:"
(cd "$OUT" && find . -type f ! -name .keep | sort | sed 's|^\./|  |') >&2
