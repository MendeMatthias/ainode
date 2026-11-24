#!/usr/bin/env bash
# Build upstream's btx-modeld for macOS arm64 from a pinned upstream tag, with
# the helper's sources untouched.
#
#   scripts/build-engine-macos.sh --out DIR [--work DIR] [--cache DIR]
#                                 [--btx-repo ~/repos/btx | --from-github]
#                                 [--fetch] [--python /path/to/python3.11+]
#                                 [--jobs N] [--no-probe]
#   scripts/build-engine-macos.sh --verify FILE     # only check a helper built earlier
#
# Why this exists: upstream publishes no macOS archive for v0.34.16 or later
# (Linux x86_64 only), so stage-engine.sh has nothing to take the Mac helper
# from. stage-engine.sh runs this script when its cache holds no built
# helper, and accepts the result only if its sha256 is the one pinned there.
# It signs nothing. It builds one file, DIR/btx-modeld, and writes beside it
# DIR/BUILD-MANIFEST.json (the tag, the commit, every input's sha256, the
# toolchain, the flags, and every deviation from upstream's tree) and the two
# licence texts that must travel with the file (BTX's, OpenSSL's).
#
# What it does, in order:
#   1. takes the source of BTX_COMMIT, either as `git archive` out of a local
#      btx checkout (read-only: no checkout, no fetch unless --fetch) or as
#      GitHub's commit tarball checked against BTX_TARBALL_SHA256;
#   2. builds upstream's own `depends` (boost, libevent, sqlite), sources
#      checked by depends' own hashes;
#   3. builds OpenSSL (static) from the pinned release tarball;
#   4. builds LLVM's OpenMP runtime as a static libomp.a from the pinned LLVM
#      source: upstream's configure refuses an Apple build without it unless
#      BTX_ALLOW_SERIAL_PROVER is set, which upstream marks "Do not ship this
#      build". This script never sets that option;
#   5. configures and builds the one target `btx-modeld`;
#   6. checks the result (libraries, version, a start with no OpenSSL tool).
#
# The one change outside the helper's sources (step 5): upstream includes
# cmake/SwapNet.cmake for every configuration (src/CMakeLists.txt), and that
# file requires Python >= 3.11 with its headers and `cryptography`, because
# btxd embeds Python. btx-modeld does not use SwapNet. There is no cmake
# option that turns it off. So, unless --python names a suitable interpreter,
# that single include line is commented out in the extracted copy, and the
# manifest records the exact diff. No file under src/modelnet and not
# src/modeld.cpp is ever changed; the script checks that before it builds.
#
# Network: github.com (the pinned tarballs) and whatever upstream's depends
# fetches (boost, libevent, sqlite), only when not already in the cache.
set -euo pipefail

# ── Pins ────────────────────────────────────────────────────────────────────
BTX_TAG=v0.34.16
BTX_COMMIT=074dac167c26c1e091198f691ed5727817dba88a
BTX_TREE=f8c185ee84298f47043b4a6616f73bc1cc57c478
BTX_TARBALL_URL=https://github.com/btxchain/btx/archive/$BTX_COMMIT.tar.gz
BTX_TARBALL_SHA256=72c571df7b92dd8cb81171d0b514ded7ecf27a1ed7e5bf04b406953d58368b75
BTX_VERSION_LINE="btx-modeld v0.34.16"
# The commit's own time (git log -1 --format=%ct). Used as SOURCE_DATE_EPOCH so
# that no build date goes into the binary (OpenSSL's "built on" string).
BTX_COMMIT_TIME=1794722053

OPENSSL_VERSION=3.5.9
OPENSSL_ARCHIVE=openssl-$OPENSSL_VERSION.tar.gz
OPENSSL_URL=https://github.com/openssl/openssl/releases/download/openssl-$OPENSSL_VERSION/$OPENSSL_ARCHIVE
OPENSSL_SHA256=603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a
# The same Configure line as stage-engine.sh. no-shared: nothing to find at run time.
OPENSSL_CONFIGURE=(darwin64-arm64-cc no-shared no-tests no-docs)

LLVM_VERSION=23.1.6
LLVM_ARCHIVE=llvm-project-$LLVM_VERSION.src.tar.xz
LLVM_URL=https://github.com/llvm/llvm-project/releases/download/llvmorg-$LLVM_VERSION/$LLVM_ARCHIVE
LLVM_SHA256=d3b86ab29cfb4445a6652e5c2642ccc75e04e38084dde3701d323320f35a31da
# Only these parts of the LLVM tree are extracted; the rest is never unpacked.
LLVM_PARTS=(openmp cmake runtimes llvm/cmake llvm/utils/llvm-lit third-party)

# The helper runs on macOS 13 and later, as upstream's own build sets it.
MACOS_MIN=13.0

# depends: only what a helper needs. SQLite stays: the helper links it since
# v0.34.16 (src/modelnet/security_v2/native.cmake).
DEPENDS_ARGS=(NO_QT=1 NO_QR=1 NO_ZMQ=1 NO_BDB=1 NO_UPNP=1 NO_USDT=1)

# cmake: the helper, nothing else. BTX_MATMUL_METAL_PRECOMPILE_KERNELS=OFF
# makes the MatMul Metal kernels compile at run time instead of shipping
# .metallib files beside the binary (AInode ships the one file); it also
# builds on a Mac without Xcode's Metal Toolchain component. Recorded as a
# deviation from upstream's release archive, which carries libexec/metal/.
CMAKE_ARGS=(
  -DWITH_MODELNET=ON -DENABLE_WALLET=OFF -DBUILD_GUI=OFF
  -DBUILD_TESTS=OFF -DBUILD_BENCH=OFF -DWITH_ZMQ=OFF -DWITH_CCACHE=OFF
  -DOPENSSL_USE_STATIC_LIBS=ON
  -DBTX_MATMUL_METAL_PRECOMPILE_KERNELS=OFF
)
SWAPNET_INCLUDE='include(${PROJECT_SOURCE_DIR}/cmake/SwapNet.cmake)'

# Two runs must give the same bytes, whatever folder they run in:
#   SOURCE_DATE_EPOCH  no build date in the binary;
#   ZERO_AR_DATE       no object-file times in the archives and debug symbols;
#   and, added to the cmake line below once the work folder is known, a
#   debug-prefix map for every language the helper is built from (C, C++,
#   Objective-C++, assembly) and ld's -oso_prefix, so the work folder's own
#   path is not written into the debug symbols.
# Measured 2026-11-23: without these, two runs differed in OpenSSL's build
# date, the object times and the folder path in the debug symbols, and
# therefore in LC_UUID and the linker's ad-hoc signature. With them, two runs
# in two different folders gave the same sha256.
export SOURCE_DATE_EPOCH=$BTX_COMMIT_TIME ZERO_AR_DATE=1
BUILD_ROOT_NAME=/ainode-engine-build

# ── Arguments ───────────────────────────────────────────────────────────────
APP_DIR=$(cd "$(dirname "$0")/.." && pwd)
OUT= WORK= CACHE=${AINODE_STAGE_CACHE:-"$HOME/Library/Caches/ainode-stage"}
BTX_REPO="$HOME/repos/btx" FROM_GITHUB=0 FETCH=0 PYTHON= PROBE=1 VERIFY=
JOBS=$(sysctl -n hw.ncpu 2>/dev/null || echo 4)

say() { printf '%s\n' "$*" >&2; }
die() { say "build-engine-macos: $*"; exit 1; }
sha256() { shasum -a 256 "$1" | awk '{print $1}'; }

while [ $# -gt 0 ]; do
  case $1 in
    --out) OUT=$2; shift 2 ;;
    --work) WORK=$2; shift 2 ;;
    --cache) CACHE=$2; shift 2 ;;
    --btx-repo) BTX_REPO=$2; shift 2 ;;
    --from-github) FROM_GITHUB=1; shift ;;
    --fetch) FETCH=1; shift ;;
    --python) PYTHON=$2; shift 2 ;;
    --jobs) JOBS=$2; shift 2 ;;
    --no-probe) PROBE=0; shift ;;
    --verify) VERIFY=$2; shift 2 ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown argument $1 (see --help)" ;;
  esac
done
# check_binary FILE: system libraries only, the pinned version, the pinned OpenSSL.
# Sets `libs` and `version`.
check_binary() {
  libs=$(otool -L "$1" | awk 'NR>1 {print $1}')
  if printf '%s\n' "$libs" | grep -vE '^(/usr/lib/|/System/)'; then
    die "btx-modeld links a non-system library (see above)"
  fi
  version=$("$1" -version 2>&1 | head -2)
  printf '%s\n' "$version" | head -1 | grep -qxF "$BTX_VERSION_LINE" || die "-version says: $(printf '%s' "$version" | head -1), expected $BTX_VERSION_LINE"
  printf '%s\n' "$version" | grep -q "^OpenSSL $OPENSSL_VERSION " || die "the helper did not link OpenSSL $OPENSSL_VERSION"
}
# probe_start FILE: a start as a stock Mac gives it: no BTX_OPENSSL, the
# system's LibreSSL on PATH, a throwaway modeldir, no peers, no listener, the
# socket in its own 0700 folder. It must mint its identity in-process and
# print "PQ1 ready". Waits up to 60 s for that line: on a busy Mac the socket
# can appear before the line is written.
probe_start() {
  local probe pid ok=0 first_err
  probe=$(mktemp -d "${TMPDIR:-/tmp}/ainode-engine-probe.XXXXXX")
  chmod 700 "$probe"
  ( cd "$probe" && exec env -i HOME="$HOME" TMPDIR="${TMPDIR:-/tmp}" PATH=/usr/bin:/bin \
      "$1" -modeldir="$probe/md" -modelstorage=1GiB -modelseed=manual -modelfollowpeers=0 \
      -modelrpcsocket="$probe/s.sock" >"$probe/out" 2>"$probe/err" ) &
  pid=$!
  for _ in $(seq 1 600); do
    if grep -q "PQ1 ready" "$probe/out" 2>/dev/null && [ -S "$probe/s.sock" ] && [ -s "$probe/md/tls/cert.pem" ]; then ok=1; break; fi
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.1
  done
  kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
  kill -KILL "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  first_err=$(head -1 "$probe/err" 2>/dev/null || true)
  rm -rf "$probe"
  [ "$ok" = 1 ] || die "the helper did not start with no OpenSSL tool: ${first_err:-no PQ1 ready line within 60 s}"
  say "verified: the helper starts with no OpenSSL tool and mints its identity"
}

if [ -n "$VERIFY" ]; then
  [ -x "$VERIFY" ] || die "--verify $VERIFY: not an executable file"
  check_binary "$VERIFY"
  probe_start "$VERIFY"
  say "sha256: $(sha256 "$VERIFY")"
  say "-version:"; printf '%s\n' "$version" | sed 's/^/  /' >&2
  say "otool -L:"; printf '%s\n' "$libs" | sed 's/^/  /' >&2
  exit 0
fi
[ -n "$OUT" ] || die "--out DIR is required (where btx-modeld and BUILD-MANIFEST.json go)"

[ "$(uname -s)-$(uname -m)" = "Darwin-arm64" ] || die "macOS arm64 only"
for t in curl shasum tar make cmake ninja clang jq otool xcrun sw_vers; do
  command -v "$t" >/dev/null || die "missing tool: $t"
done
mkdir -p "$OUT" "$CACHE/archives" "$CACHE/depends-sources"
OUT=$(cd "$OUT" && pwd)
AUTO_WORK=0
if [ -z "$WORK" ]; then
  # Removed at the end of a good run only: after a failure the logs stay.
  WORK=$(mktemp -d "${TMPDIR:-/tmp}/ainode-engine-build.XXXXXX")
  AUTO_WORK=1
else
  mkdir -p "$WORK"
  [ -z "$(ls -A "$WORK")" ] || die "--work $WORK is not empty; give a fresh folder"
fi
# The physical path, no symlink and no double slash: cmake and the compiler
# see that one, and the debug-prefix map below must name exactly it. With
# $TMPDIR's own spelling (/var/folders/.../T//...) the map did not match, the
# folder's path went into the debug symbols and the result had another
# sha256 (caught by stage-engine.sh's pin on 2026-11-23).
WORK=$(cd "$WORK" && pwd -P)
DEVIATIONS="$WORK/deviations.jsonl"; : >"$DEVIATIONS"
deviate() {  # deviate WHAT WHY [DIFF]
  jq -cn --arg what "$1" --arg why "$2" --arg diff "${3:-}" '{what:$what, why:$why, diff:$diff}' >>"$DEVIATIONS"
  say "DEVIATION: $1"; say "      why: $2"
  [ -z "${3:-}" ] || printf '%s\n' "$3" | sed 's/^/      /' >&2
}

verify() {  # verify FILE SHA256
  local got; got=$(sha256 "$1")
  [ "$got" = "$2" ] || die "sha256 MISMATCH for $1: got $got, pinned $2. Refusing to use it."
}
# fetch NAME URL SHA256 -> prints the path of a verified archive.
fetch() {
  local name=$1 url=$2 want=$3 f
  case $url in https://github.com/*) ;; *) die "refusing non-GitHub URL $url" ;; esac
  f="$CACHE/archives/$name"
  if [ ! -f "$f" ]; then
    say "downloading $url"
    curl -fL --proto '=https' --tlsv1.2 --retry 3 -o "$f.part" "$url"
    mv "$f.part" "$f"
  else
    say "cached: $f"
  fi
  verify "$f" "$want"
  printf '%s\n' "$f"
}

# ── 1. The source ───────────────────────────────────────────────────────────
SRC="$WORK/btx"
mkdir -p "$SRC"
if [ "$FROM_GITHUB" = 1 ]; then
  tarball=$(fetch "btx-$BTX_COMMIT.tar.gz" "$BTX_TARBALL_URL" "$BTX_TARBALL_SHA256")
  tar -xzf "$tarball" -C "$SRC" --strip-components 1
  SOURCE_HOW="GitHub commit tarball $BTX_TARBALL_URL, sha256 $BTX_TARBALL_SHA256"
else
  command -v git >/dev/null || die "missing tool: git (or use --from-github)"
  [ -d "$BTX_REPO" ] || die "no btx checkout at $BTX_REPO (use --btx-repo DIR or --from-github)"
  if [ "$FETCH" = 1 ]; then
    say "fetching tag $BTX_TAG into $BTX_REPO (asked with --fetch)"
    git -C "$BTX_REPO" fetch --no-tags origin "refs/tags/$BTX_TAG:refs/tags/$BTX_TAG"
  fi
  got=$(git -C "$BTX_REPO" rev-parse --verify --quiet "$BTX_COMMIT^{commit}" || true)
  [ "$got" = "$BTX_COMMIT" ] || die "$BTX_REPO does not have commit $BTX_COMMIT (try --fetch or --from-github)"
  tagged=$(git -C "$BTX_REPO" rev-parse --verify --quiet "$BTX_TAG^{commit}" || true)
  [ "$tagged" = "$BTX_COMMIT" ] || die "$BTX_TAG in $BTX_REPO is ${tagged:-missing}, pinned $BTX_COMMIT. The tag moved, or the pin is stale. Refusing."
  tree=$(git -C "$BTX_REPO" rev-parse "$BTX_COMMIT^{tree}")
  [ "$tree" = "$BTX_TREE" ] || die "tree of $BTX_COMMIT is $tree, pinned $BTX_TREE"
  # Read-only on the checkout: git archive reads objects, it never touches the work tree.
  git -C "$BTX_REPO" archive --format=tar "$BTX_COMMIT" | tar -x -C "$SRC"
  SOURCE_HOW="git archive of $BTX_COMMIT (tree $BTX_TREE) from $BTX_REPO"
fi
[ -f "$SRC/src/modeld.cpp" ] && [ -d "$SRC/src/modelnet" ] || die "the source has no helper (src/modeld.cpp, src/modelnet)"
say "source: $SOURCE_HOW"
# The helper's sources as extracted, to prove at the end that none changed.
helper_digest() { (cd "$SRC" && find src/modeld.cpp src/modelnet -type f -print0 | sort -z | xargs -0 shasum -a 256 | shasum -a 256 | awk '{print $1}'); }
HELPER_SRC_SHA=$(helper_digest)

# ── 2. depends ──────────────────────────────────────────────────────────────
say "building depends (${DEPENDS_ARGS[*]}); log: $WORK/depends.log"
make -C "$SRC/depends" -j "$JOBS" "${DEPENDS_ARGS[@]}" SOURCES_PATH="$CACHE/depends-sources" >"$WORK/depends.log" 2>&1 \
  || die "depends failed, see $WORK/depends.log"
PREFIX=$(find "$SRC/depends" -maxdepth 1 -type d -name 'aarch64-apple-darwin*' | head -1)
[ -n "$PREFIX" ] && [ -f "$PREFIX/toolchain.cmake" ] || die "depends left no toolchain prefix"
for f in lib/libevent_core.a lib/libsqlite3.a include/boost/version.hpp; do
  [ -e "$PREFIX/$f" ] || die "depends did not build $f"
done

# ── 3. OpenSSL, static ──────────────────────────────────────────────────────
ossl_tar=$(fetch "$OPENSSL_ARCHIVE" "$OPENSSL_URL" "$OPENSSL_SHA256")
mkdir -p "$WORK/openssl"
tar -xzf "$ossl_tar" -C "$WORK/openssl" --strip-components 1
say "building OpenSSL $OPENSSL_VERSION (${OPENSSL_CONFIGURE[*]}); log: $WORK/openssl.log"
(
  cd "$WORK/openssl"
  ./Configure "${OPENSSL_CONFIGURE[@]}" "-mmacosx-version-min=$MACOS_MIN"
  make -j "$JOBS" build_libs
) >"$WORK/openssl.log" 2>&1 || die "OpenSSL build failed, see $WORK/openssl.log"
cp "$WORK/openssl/libssl.a" "$WORK/openssl/libcrypto.a" "$PREFIX/lib/"
mkdir -p "$PREFIX/include/openssl"
cp "$WORK/openssl"/include/openssl/*.h "$PREFIX/include/openssl/"
grep -q "OPENSSL_VERSION_TEXT \"OpenSSL $OPENSSL_VERSION " "$PREFIX/include/openssl/opensslv.h" \
  || die "the OpenSSL headers in the prefix are not $OPENSSL_VERSION"

# ── 4. libomp, static ───────────────────────────────────────────────────────
llvm_tar=$(fetch "$LLVM_ARCHIVE" "$LLVM_URL" "$LLVM_SHA256")
mkdir -p "$WORK/llvm"
parts=(); for p in "${LLVM_PARTS[@]}"; do parts+=("llvm-project-$LLVM_VERSION.src/$p"); done
tar -xJf "$llvm_tar" -C "$WORK/llvm" --strip-components 1 "${parts[@]}"
say "building LLVM OpenMP $LLVM_VERSION as a static libomp.a; log: $WORK/libomp.log"
(
  cmake -S "$WORK/llvm/runtimes" -B "$WORK/llvm-build" -G Ninja \
    -DLLVM_ENABLE_RUNTIMES=openmp -DCMAKE_BUILD_TYPE=Release \
    -DLIBOMP_ENABLE_SHARED=OFF -DOPENMP_ENABLE_LIBOMPTARGET=OFF \
    -DLIBOMP_OMPD_SUPPORT=OFF -DLIBOMP_OMPT_SUPPORT=OFF -DOPENMP_ENABLE_OMPT_TOOLS=OFF \
    -DLIBOMP_FORTRAN_MODULES=OFF -DLLVM_INCLUDE_TESTS=OFF \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MIN"
  ninja -C "$WORK/llvm-build" -j "$JOBS" omp
) >"$WORK/libomp.log" 2>&1 || die "libomp build failed, see $WORK/libomp.log"
cp "$WORK/llvm-build/openmp/runtime/src/libomp.a" "$PREFIX/lib/"
cp "$WORK/llvm-build/openmp/runtime/src/omp.h" "$PREFIX/include/"

# ── 5. Configure and build btx-modeld ───────────────────────────────────────
PREFIX_MAP="-fdebug-prefix-map=$WORK=$BUILD_ROOT_NAME"
cmake_args=("${CMAKE_ARGS[@]}" "-DOPENSSL_ROOT_DIR=$PREFIX"
  "-DAPPEND_CXXFLAGS=$PREFIX_MAP"
  "-DAPPEND_LDFLAGS=-Wl,-oso_prefix,$WORK/")
if [ -n "$PYTHON" ]; then
  # The untouched path: a Python >= 3.11 with headers and `cryptography`.
  "$PYTHON" -I -c 'import sys; assert sys.version_info >= (3, 11); import cryptography, sqlite3' \
    || die "--python $PYTHON is not Python >= 3.11 with cryptography and sqlite3"
  cmake_args+=("-DPython3_EXECUTABLE=$PYTHON")
  say "SwapNet.cmake left in place; configuring with $PYTHON"
else
  n=$(grep -cxF "$SWAPNET_INCLUDE" "$SRC/src/CMakeLists.txt" || true)
  [ "$n" = 1 ] || die "expected exactly one '$SWAPNET_INCLUDE' line in src/CMakeLists.txt, found $n. Upstream changed it: read the file before going on."
  cp "$SRC/src/CMakeLists.txt" "$WORK/CMakeLists.txt.upstream"
  awk -v line="$SWAPNET_INCLUDE" '$0 == line { print "# build-engine-macos.sh: not included for a helper-only build. " $0; next } { print }' \
    "$WORK/CMakeLists.txt.upstream" >"$SRC/src/CMakeLists.txt"
  d=$(diff "$WORK/CMakeLists.txt.upstream" "$SRC/src/CMakeLists.txt" || true)
  deviate "src/CMakeLists.txt: the include of cmake/SwapNet.cmake is commented out (1 line)" \
          "SwapNet.cmake requires Python >= 3.11 with headers and cryptography for every configuration; btx-modeld does not use SwapNet; upstream has no option to switch it off. Avoid this deviation with --python." \
          "$d"
fi
deviate "cmake -DBTX_MATMUL_METAL_PRECOMPILE_KERNELS=OFF" \
        "one binary with no .metallib files beside it (upstream's archive ships libexec/metal/); the Metal kernels compile at run time. Also builds without Xcode's Metal Toolchain."
deviate "libomp.a is LLVM OpenMP $LLVM_VERSION built here from source" \
        "upstream expects Homebrew's static libomp.a; this Mac build takes it from the pinned LLVM source instead. BTX_ALLOW_SERIAL_PROVER is NOT used."

say "configuring; log: $WORK/configure.log"
# Upstream's APPEND_CXXFLAGS reaches C++ only. C, Objective-C++ and assembly
# take the same map through cmake's own environment variables, which it adds
# to the toolchain's flags at the first configure.
CFLAGS="$PREFIX_MAP" OBJCXXFLAGS="$PREFIX_MAP" ASMFLAGS="$PREFIX_MAP" \
  cmake -S "$SRC" -B "$WORK/build" -G Ninja --toolchain "$PREFIX/toolchain.cmake" "${cmake_args[@]}" >"$WORK/configure.log" 2>&1 \
  || die "configure failed, see $WORK/configure.log"
for lang in C OBJCXX ASM; do
  grep -q "^CMAKE_${lang}_FLAGS:.*-fdebug-prefix-map=" "$WORK/build/CMakeCache.txt" \
    || die "the debug-prefix map did not reach CMAKE_${lang}_FLAGS; the result would carry this folder's path"
done
grep -q "Using AppleClang OpenMP runtime (static)" "$WORK/configure.log" \
  || die "configure did not pick up the static libomp.a (see $WORK/configure.log)"
! grep -q "BTX_ALLOW_SERIAL_PROVER=ON" "$WORK/configure.log" || die "configure fell back to the serial prover"
[ "$(helper_digest)" = "$HELPER_SRC_SHA" ] || die "a file under src/modelnet or src/modeld.cpp changed. That must never happen."

say "building btx-modeld ($JOBS jobs); log: $WORK/build.log"
cmake --build "$WORK/build" --target btx-modeld -j "$JOBS" >"$WORK/build.log" 2>&1 || die "build failed, see $WORK/build.log"
BIN="$WORK/build/bin/btx-modeld"
[ -x "$BIN" ] || die "the build left no bin/btx-modeld"
[ "$(helper_digest)" = "$HELPER_SRC_SHA" ] || die "a helper source changed during the build"

# ── 6. Check the result ─────────────────────────────────────────────────────
cp "$BIN" "$OUT/btx-modeld"
chmod 755 "$OUT/btx-modeld"
# The licences of what is inside the file we just built: BTX itself (MIT) and
# the OpenSSL linked into it (Apache-2.0). They ship beside the helper.
cp "$SRC/COPYING" "$OUT/LICENSE-btx.txt"
cp "$WORK/openssl/LICENSE.txt" "$OUT/LICENSE-openssl.txt"
[ "$(sha256 "$OUT/btx-modeld")" = "$(sha256 "$BIN")" ] || die "btx-modeld changed while copying"
check_binary "$OUT/btx-modeld"

PROBED=false
if [ "$PROBE" = 1 ]; then
  probe_start "$OUT/btx-modeld"
  PROBED=true
fi

jq -n \
  --arg tag "$BTX_TAG" --arg commit "$BTX_COMMIT" --arg tree "$BTX_TREE" --arg source "$SOURCE_HOW" \
  --arg helper_src "$HELPER_SRC_SHA" \
  --arg sha "$(sha256 "$OUT/btx-modeld")" --argjson size "$(stat -f %z "$OUT/btx-modeld")" \
  --arg version "$version" --arg libs "$libs" --argjson probed "$PROBED" \
  --arg openssl "$OPENSSL_URL sha256 $OPENSSL_SHA256; Configure ${OPENSSL_CONFIGURE[*]} -mmacosx-version-min=$MACOS_MIN; make build_libs" \
  --arg llvm "$LLVM_URL sha256 $LLVM_SHA256; runtimes build of openmp, static, parts: ${LLVM_PARTS[*]}" \
  --arg depends "make -C depends ${DEPENDS_ARGS[*]}" \
  --arg cmake_flags "$(printf '%s' "--toolchain $PREFIX/toolchain.cmake ${cmake_args[*]} ; target btx-modeld ; env SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH ZERO_AR_DATE=1 CFLAGS=OBJCXXFLAGS=ASMFLAGS=$PREFIX_MAP" | sed "s|$WORK|<work>|g")" \
  --arg macos "$(sw_vers -productVersion) ($(sw_vers -buildVersion))" \
  --arg xcode "$(xcodebuild -version 2>/dev/null | tr '\n' ' ' || true)" \
  --arg sdk "$(xcrun --show-sdk-version 2>/dev/null || true)" \
  --arg clang "$(clang --version | head -1)" \
  --arg cmake "$(cmake --version | head -1)" --arg ninja "$(ninja --version)" \
  --arg macos_min "$MACOS_MIN" \
  --slurpfile deviations "$DEVIATIONS" \
  '{schema: 1,
    what: "btx-modeld for macOS arm64, built by AInode from upstream source; unsigned",
    upstream: {tag: $tag, commit: $commit, tree: $tree, source: $source,
               helper_sources_sha256: $helper_src,
               helper_sources_note: "sha256 over the sha256 list of src/modeld.cpp and every file under src/modelnet, checked unchanged before and after the build"},
    inputs: {depends: $depends, openssl: $openssl, libomp: $llvm},
    cmake: $cmake_flags,
    serial_prover_option_used: false,
    toolchain: {macos: $macos, xcode: $xcode, sdk: $sdk, clang: $clang, cmake: $cmake, ninja: $ninja, deployment_target: $macos_min},
    deviations: $deviations,
    result: {path: "btx-modeld", sha256: $sha, size: $size, version: $version,
             links: ($libs | split("\n")), started_with_no_openssl_tool: $probed}}' >"$OUT/BUILD-MANIFEST.json"

say ""
say "built: $OUT/btx-modeld"
say "sha256: $(sha256 "$OUT/btx-modeld")"
say "size:   $(stat -f %z "$OUT/btx-modeld") bytes"
say "-version:"; printf '%s\n' "$version" | sed 's/^/  /' >&2
say "otool -L:"; printf '%s\n' "$libs" | sed 's/^/  /' >&2
say "manifest: $OUT/BUILD-MANIFEST.json"
say "Not signed. stage-engine.sh stages this file only if its sha256 is the one pinned there."
if [ "$AUTO_WORK" = 1 ]; then rm -rf "$WORK"; else say "work folder kept: $WORK"; fi
