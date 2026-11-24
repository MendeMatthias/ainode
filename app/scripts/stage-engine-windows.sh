#!/usr/bin/env bash
# Windows x86_64: populate src-tauri/resources/engine/ for the Windows app
# (docs/windows.md, route 1). Run INSIDE WSL (an Ubuntu 24.04+ distribution
# with the Linux build tools), on the Windows checkout:
#
#   wsl -d <build distro> -- bash /mnt/c/<checkout>/app/scripts/stage-engine-windows.sh
#
# What the Windows app ships, and why:
#   - upstream's Linux helper exactly as on Linux (btx-modeld and lib/; no
#     OpenSSL tool since v0.34.16): it runs inside AInode's own WSL
#     distribution. Staged by stage-engine-linux.sh into a scratch folder and
#     taken from there, with its manifest entries.
#   - ainode-wsl (app/wsl-host): the small static Linux program that starts
#     the helper inside WSL and relays the app's JSON-RPC over one pipe.
#   - wsl/rootfs.tar.gz: Canonical's Ubuntu Base 24.04.5 (30 MB), imported as
#     the `AInode` distribution on the first Start after consent.
#   - llama/: llama.cpp b11559's Windows Vulkan build, llama-server.exe and the
#     DLLs it loads (CPU backends picked at run time, Vulkan when a driver is
#     there), plus the Visual C++ runtime DLLs next to it (Microsoft allows
#     app-local copies; a fresh Windows may have none).
# Every download is checked against its pinned sha256 before use, and each
# part is run once here the way the app runs it.
set -euo pipefail

APP_DIR=$(cd "$(dirname "$0")/.." && pwd)
OUT="$APP_DIR/src-tauri/resources/engine"

# ── Pins ────────────────────────────────────────────────────────────────────
LLAMA_TAG=b11559
LLAMA_ARCHIVE=llama-$LLAMA_TAG-bin-win-vulkan-x64.zip
LLAMA_URL=https://github.com/ggml-org/llama.cpp/releases/download/$LLAMA_TAG/$LLAMA_ARCHIVE
LLAMA_SHA256=bf2fb2a7fc5d7669ccabbb6de3f6dd6b54058f213b32e731abc62b590f8566dc
# The llama.cpp licence text (MIT) is in its Linux archive, not in the zip.
LLAMA_LICENSE_ARCHIVE=llama-$LLAMA_TAG-bin-ubuntu-vulkan-x64.tar.gz
LLAMA_LICENSE_URL=https://github.com/ggml-org/llama.cpp/releases/download/$LLAMA_TAG/$LLAMA_LICENSE_ARCHIVE
LLAMA_LICENSE_SHA256=3c864ef7eaac254b179a52cfc648ea5b31bcea67e3523710acc6828f44e12d9e

ROOTFS_ARCHIVE=ubuntu-base-24.04.5-base-amd64.tar.gz
ROOTFS_URL=https://cdimage.ubuntu.com/ubuntu-base/releases/24.04/release/$ROOTFS_ARCHIVE
ROOTFS_SHA256=e77b6f10c2590cef872b33ee9f635a0e3fd1f57fb074c0e52b5c7f56147a0c86

# The Visual C++ runtime llama.cpp's Windows build links (MSVCP140,
# VCRUNTIME140, VCRUNTIME140_1), copied from this machine's System32 and
# refused below this version (the build is newer than VS 2019's 14.29).
VCRT_DLLS=(msvcp140.dll vcruntime140.dll vcruntime140_1.dll)
VCRT_MIN=14.40

CACHE=${AINODE_STAGE_CACHE:-"${XDG_CACHE_HOME:-$HOME/.cache}/ainode-stage"}
mkdir -p "$CACHE/archives"

say() { printf '%s\n' "$*" >&2; }
die() { say "stage-engine-windows: $*"; exit 1; }
sha256() { sha256sum "$1" | awk '{print $1}'; }

[ "$(uname -s)" = Linux ] && grep -qi microsoft /proc/version || die "run this inside WSL (it builds Linux parts and checks Windows ones)"
for t in curl sha256sum unzip objdump jq cargo; do
  command -v "$t" >/dev/null || die "missing tool: $t (unzip: apt install unzip)"
done
# By path: a build distribution may keep Windows' PATH out (appendWindowsPath).
PWSH=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe
[ -x "$PWSH" ] || die "no $PWSH (WSL interop must be on)"

# fetch NAME URL SHA256 -> the path of a verified archive (cached).
fetch() {
  local name=$1 url=$2 want=$3 f
  case $url in https://github.com/*/releases/download/*|https://cdimage.ubuntu.com/ubuntu-base/releases/*) ;; *) die "refusing $url" ;; esac
  f=$(find -L "$CACHE" -maxdepth 3 -type f -name "$name" 2>/dev/null | head -1 || true)
  if [ -z "$f" ]; then
    f="$CACHE/archives/$name"
    say "downloading $url"
    curl -fL --proto '=https' --tlsv1.2 --retry 3 -o "$f.part" "$url"
    mv "$f.part" "$f"
  fi
  [ "$(sha256 "$f")" = "$want" ] || die "sha256 MISMATCH for $f, pinned $want. Refusing to use it."
  printf '%s\n' "$f"
}

WORK=$(mktemp -d "${TMPDIR:-/tmp}/ainode-stage-win.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
STAGE="$WORK/engine"
mkdir -p "$STAGE/llama" "$STAGE/wsl"
LINES="$WORK/manifest.jsonl"
: >"$LINES"

record() {  # record PATH_IN_ENGINE SOURCE_URL SOURCE_SHA256 HOW
  jq -cn --arg path "$1" --arg url "$2" --arg src "$3" --arg how "$4" \
     --arg sha "$(sha256 "$STAGE/$1")" --argjson size "$(stat -c %s "$STAGE/$1")" \
     '{path:$path, source_url:$url, source_sha256:$src, how:$how, sha256:$sha, size:$size}' >>"$LINES"
}

# ── upstream's Linux helper, as on Linux ────────────────────────────────────
say "staging the Linux helper (stage-engine-linux.sh)"
AINODE_ENGINE_OUT="$WORK/linux" bash "$APP_DIR/scripts/stage-engine-linux.sh" >&2
# The helper, its libraries, and the licence texts for both.
for f in btx-modeld lib LICENSE-btx.txt LICENSE-openssl.txt licenses; do
  [ -e "$WORK/linux/$f" ] || die "the Linux staging has no $f"
  cp -a "$WORK/linux/$f" "$STAGE/$f"
done
jq -c '.files[] | select(.path | startswith("llama/") | not)' "$WORK/linux/MANIFEST.json" >>"$LINES"

# ── ainode-wsl ──────────────────────────────────────────────────────────────
say "building ainode-wsl (static, musl)"
rustup target list --installed 2>/dev/null | grep -q x86_64-unknown-linux-musl || rustup target add x86_64-unknown-linux-musl >&2
rm -rf "$WORK/wsl-host" && cp -r "$APP_DIR/wsl-host" "$WORK/wsl-host" && rm -rf "$WORK/wsl-host/target"
[ -f "$WORK/wsl-host/Cargo.lock" ] || die "app/wsl-host/Cargo.lock is missing (cargo generate-lockfile, then commit it)"
(cd "$WORK/wsl-host" && CARGO_TARGET_DIR="$CACHE/wsl-host-target" cargo test --release --locked --target x86_64-unknown-linux-musl -q >&2 \
  && CARGO_TARGET_DIR="$CACHE/wsl-host-target" cargo build --release --locked --target x86_64-unknown-linux-musl -q >&2) \
  || die "ainode-wsl did not build or its tests failed"
cp "$CACHE/wsl-host-target/x86_64-unknown-linux-musl/release/ainode-wsl" "$STAGE/ainode-wsl"
chmod 755 "$STAGE/ainode-wsl"
[ -z "$(objdump -p "$STAGE/ainode-wsl" | awk '/NEEDED/')" ] || die "ainode-wsl links shared libraries; it must be static"
record ainode-wsl "" "" "built from app/wsl-host: cargo build --release --target x86_64-unknown-linux-musl (static)"

# ── the Linux it runs in ────────────────────────────────────────────────────
rootfs=$(fetch "$ROOTFS_ARCHIVE" "$ROOTFS_URL" "$ROOTFS_SHA256")
cp "$rootfs" "$STAGE/wsl/rootfs.tar.gz"
tar -tzf "$STAGE/wsl/rootfs.tar.gz" usr/sbin/useradd usr/lib/x86_64-linux-gnu/libc.so.6 >/dev/null \
  || die "the rootfs lacks useradd or glibc"
# Since v0.34.16 upstream's helper takes libstdc++ and libgcc_s from the
# system it runs in, which here is this rootfs: it must have them, and its
# libstdc++ must carry the symbol versions the helper needs.
mkdir -p "$WORK/rootfs-check"
tar -xzf "$STAGE/wsl/rootfs.tar.gz" -C "$WORK/rootfs-check" --wildcards \
  'usr/lib/x86_64-linux-gnu/libstdc++.so.6*' 'usr/lib/x86_64-linux-gnu/libgcc_s.so.1' 2>/dev/null \
  || die "the rootfs has no libstdc++.so.6 or libgcc_s.so.1; upstream's helper needs both since v0.34.16"
rootfs_cxx=$(find "$WORK/rootfs-check" -name 'libstdc++.so.6.*' -type f | head -1)
[ -n "$rootfs_cxx" ] || die "the rootfs's libstdc++.so.6 points at no file"
for want in GLIBCXX_3.4.32 CXXABI_1.3.15; do
  grep -aq "$want" "$rootfs_cxx" || die "the rootfs's libstdc++ lacks $want, which upstream's v0.34.16 helper needs"
done
say "verified: the rootfs has libgcc_s and a libstdc++ with GLIBCXX_3.4.32 and CXXABI_1.3.15"
record wsl/rootfs.tar.gz "$ROOTFS_URL" "$ROOTFS_SHA256" "Canonical's Ubuntu Base 24.04.5 tarball, bytes unmodified"

# ── llama.cpp for Windows ───────────────────────────────────────────────────
llama=$(fetch "$LLAMA_ARCHIVE" "$LLAMA_URL" "$LLAMA_SHA256")
mkdir -p "$WORK/llama-win" && unzip -q -o "$llama" -d "$WORK/llama-win"
L="$WORK/llama-win"
[ -f "$L/llama-server.exe" ] || die "llama-server.exe not in $LLAMA_ARCHIVE"
cp "$L/llama-server.exe" "$STAGE/llama/"
# The backends are loaded at run time from the exe's folder: every CPU
# variant and Vulkan, not the RPC backend (remote machines only).
cp "$L"/ggml-cpu-*.dll "$L/ggml-vulkan.dll" "$STAGE/llama/"
# Everything they import that the zip has, by name (Windows looks next to
# the exe first). The rest must be Windows itself, UCRT, Vulkan's loader or
# the VC runtime handled below.
imports() { objdump -p "$1" | awk '/DLL Name/ {print $3}'; }
queue=("$STAGE/llama/llama-server.exe" "$STAGE"/llama/*.dll)
while [ ${#queue[@]} -gt 0 ]; do
  bin=${queue[0]}; queue=("${queue[@]:1}")
  while IFS= read -r dep; do
    here=$(find "$STAGE/llama" -maxdepth 1 -iname "$dep" | head -1)
    [ -n "$here" ] && continue
    src=$(find "$L" -maxdepth 1 -iname "$dep" | head -1)
    if [ -n "$src" ]; then
      cp "$src" "$STAGE/llama/"
      queue+=("$STAGE/llama/$(basename "$src")")
    elif ! printf '%s\n' "$dep" | grep -qiE '^(kernel32|advapi32|ws2_32|crypt32|shell32|user32|psapi|ole32|bcrypt|ntdll|vulkan-1|msvcp140|vcruntime140|vcruntime140_1|api-ms-win-crt-[a-z0-9-]+)\.dll$'; then
      die "$(basename "$bin") imports $dep: not in the zip and not part of Windows"
    fi
  done < <(imports "$bin")
done
[ -f "$L/LICENSE-LLVM-OpenMP" ] && cp "$L/LICENSE-LLVM-OpenMP" "$STAGE/llama/"
lic=$(fetch "$LLAMA_LICENSE_ARCHIVE" "$LLAMA_LICENSE_URL" "$LLAMA_LICENSE_SHA256")
tar -xzf "$lic" -C "$WORK" "llama-$LLAMA_TAG/LICENSE" && cp "$WORK/llama-$LLAMA_TAG/LICENSE" "$STAGE/llama/LICENSE"
for f in "$STAGE"/llama/*; do
  rel=llama/$(basename "$f")
  case $rel in
    llama/LICENSE) record "$rel" "$LLAMA_LICENSE_URL" "$LLAMA_LICENSE_SHA256" "LICENSE (MIT) from the Linux archive of the same tag" ;;
    *) record "$rel" "$LLAMA_URL" "$LLAMA_SHA256" "from the archive, bytes unmodified" ;;
  esac
done

# The Visual C++ runtime, app-local.
for d in "${VCRT_DLLS[@]}"; do
  src=/mnt/c/Windows/System32/$d
  [ -f "$src" ] || die "$d is not in this machine's System32; install the Visual C++ 2015-2022 Redistributable (x64)"
  v=$("$PWSH" -NoProfile -Command "(Get-Item 'C:\\Windows\\System32\\$d').VersionInfo.FileVersion" | tr -d '\r')
  [ "$(printf '%s\n%s\n' "$VCRT_MIN" "$v" | sort -V | head -1)" = "$VCRT_MIN" ] || die "$d is $v, older than $VCRT_MIN"
  # Microsoft's own bytes: a valid Authenticode signature by Microsoft, not
  # whatever a build machine happens to hold under that name.
  sig=$("$PWSH" -NoProfile -Command "\$s = Get-AuthenticodeSignature 'C:\\Windows\\System32\\$d'; \$s.Status.ToString() + ' ' + \$s.SignerCertificate.Subject" | tr -d '\r')
  case $sig in "Valid "*"O=Microsoft Corporation"*) ;; *) die "$d in System32 is not validly signed by Microsoft: $sig" ;; esac
  cp "$src" "$STAGE/llama/$d"
  # System32's copies are read-only, and Tauri copies that attribute into
  # target/, where the next build then cannot overwrite them.
  chmod 644 "$STAGE/llama/$d"
  record "llama/$d" "" "" "Microsoft Visual C++ 2015-2022 runtime $v (x64), app-local, from the build machine's System32, Authenticode signature by Microsoft verified"
done

# ── Install + prove ─────────────────────────────────────────────────────────
# `engine` (what Status shows about the helper) is the Linux staging's: the
# helper is the same file.
ENGINE_IS=$(jq -c '.engine // empty' "$WORK/linux/MANIFEST.json")
[ -n "$ENGINE_IS" ] || die "the Linux staging's MANIFEST.json does not say what the helper is (engine)"
jq -s --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson engine "$ENGINE_IS" \
  '{schema:1, platform:"windows-x86_64", generated_at:$at, generator:"app/scripts/stage-engine-windows.sh", engine:$engine, files:.}' \
  "$LINES" >"$STAGE/MANIFEST.json"
mkdir -p "$OUT"
find "$OUT" -mindepth 1 -maxdepth 1 ! -name .keep -exec rm -rf {} +
cp -R "$STAGE"/. "$OUT"/

# llama-server.exe runs on Windows (WSL interop) from the staged folder.
v=$( (cd /mnt/c && "$OUT/llama/llama-server.exe" --version 2>&1) | tr -d '\r' | grep -m1 '^version:' || true)
[ -n "$v" ] || die "the staged llama-server.exe did not run"
say "verified: staged llama-server.exe runs on Windows ($v)"

# The helper, through ainode-wsl, exactly as the app starts it: from the
# Windows folder, a pipe in, replies out, and a clean stop when it closes.
probe="$WORK/probe"; mkdir -p "$probe"
reply=$( { sleep 4
           echo '{"jsonrpc":"1.0","id":1,"method":"getmodelnetworkinfo","params":[]}'
           sleep 1; } | env -i HOME="$HOME" PATH=/usr/bin:/bin "$OUT/ainode-wsl" serve "$probe/s.sock" -- \
             "-modeldir=$probe/md" -modelstorage=1GiB -modelseed=manual -modelfollowpeers=0 "-modelrpcsocket=$probe/s.sock" 2>"$probe/err" || true)
printf '%s\n' "$reply" | grep -q '"pq1_ready":true' || { cat "$probe/err" >&2; die "the staged helper did not answer through ainode-wsl"; }
printf '%s\n' "$reply" | grep -q '"ainode_exit":{"code":0' || die "the helper did not stop cleanly when the pipe closed: $reply"
say "verified: the staged helper reaches PQ1 ready through ainode-wsl and stops cleanly"

say "staged into $OUT ($(du -sh "$OUT" | cut -f1), $(jq '.files | length' "$OUT/MANIFEST.json") files)"
