#!/usr/bin/env bash
# Build AInode for Linux x86_64: a .deb and an AppImage, each checked.
#
#   scripts/release-linux.sh            # stages the engine if needed, builds, checks
#   OUT=/some/dir scripts/release-linux.sh
#
# Tauri builds both bundles. Two things are then put right, and both outputs
# are proven before anything is printed as done:
#
#   - The AppImage bundler (linuxdeploy) patches every ELF file it finds with
#     a RUNPATH, including upstream's btx-modeld, its libssl/libcrypto and our
#     openssl CLI (observed 2026-10-13: `RUNPATH [$ORIGIN]` added, 24 KB
#     rewritten by patchelf). The engine must ship as upstream built it
#     (ENGINE-CONTRACT.md), so the AppImage is unpacked, the staged engine put
#     back, and it is packed again with its own runtime and compression.
#   - Tauri names the Debian package after the product in kebab case
#     ("a-inode"); it is renamed "ainode", and data files lose the exec bit
#     Tauri gives every resource.
#
# Proof, for each output: every file under resources/engine matches the
# sha256 in its MANIFEST.json (which stage-engine-linux.sh wrote from the
# pinned archives), and every program in it is executable.
set -euo pipefail

APP_DIR=$(cd "$(dirname "$0")/.." && pwd)
TAURI_DIR="$APP_DIR/src-tauri"
ENGINE="$TAURI_DIR/resources/engine"
BUNDLE="$TAURI_DIR/target/release/bundle"
OUT=${OUT:-"$APP_DIR/dist-linux"}
PKG=ainode

say() { printf '%s\n' "$*" >&2; }
die() { say "release-linux: $*"; exit 1; }

[ "$(uname -s)-$(uname -m)" = "Linux-x86_64" ] || die "Linux x86_64 only"
for t in npx dpkg-deb mksquashfs unsquashfs python3 sha256sum; do
  command -v "$t" >/dev/null || die "missing tool: $t (apt install squashfs-tools for mksquashfs)"
done

VERSION=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$TAURI_DIR/tauri.conf.json")
[ -f "$ENGINE/MANIFEST.json" ] && python3 -c 'import json,sys; sys.exit(json.load(open(sys.argv[1])).get("platform") != "linux-x86_64")' "$ENGINE/MANIFEST.json" \
  || { say "staging the Linux engine"; bash "$APP_DIR/scripts/stage-engine.sh"; }

# check_engine DIR LABEL: every MANIFEST file present, byte-identical, and
# executable unless it is data.
check_engine() {
  python3 - "$1" "$2" <<'PY'
import hashlib, json, os, stat, sys
e, label = sys.argv[1], sys.argv[2]
m = json.load(open(os.path.join(e, "MANIFEST.json")))
bad = []
for f in m["files"]:
    p = os.path.join(e, f["path"])
    if not os.path.isfile(p):
        bad.append(f"missing {f['path']}"); continue
    if hashlib.sha256(open(p, "rb").read()).hexdigest() != f["sha256"]:
        bad.append(f"changed {f['path']}")
    data = f["path"].endswith((".txt", ".cnf", ".json")) or os.path.basename(f["path"]) == "LICENSE"
    if not data and not os.stat(p).st_mode & stat.S_IXUSR:
        bad.append(f"not executable {f['path']}")
extra = sorted(set(os.path.relpath(os.path.join(d, n), e) for d, _, ns in os.walk(e) for n in ns)
               - {f["path"] for f in m["files"]} - {"MANIFEST.json", ".keep"})
bad += [f"not in the manifest {x}" for x in extra]
if bad:
    print(f"{label}: " + "; ".join(bad)); sys.exit(1)
print(f"{label}: {len(m['files'])} engine files identical to MANIFEST.json")
PY
}
check_engine "$ENGINE" "staged engine" >&2 || die "the staged engine does not match its own manifest; re-run scripts/stage-engine.sh"

say "building AInode $VERSION (deb, AppImage)"
# No FUSE in containers and WSL: linuxdeploy and appimagetool run extracted.
# The AppImage bundler fails now and then ("failed to run linuxdeploy": 6 times
# on 2026-10-18, each build passing on a retry), so a build that fails there
# is tried up to 3 times. Any other failure stops at once.
log=$(mktemp "${TMPDIR:-/tmp}/ainode-build.XXXXXX")
for try in 1 2 3; do
  rm -rf "$BUNDLE/deb" "$BUNDLE/appimage"
  if (cd "$APP_DIR" && APPIMAGE_EXTRACT_AND_RUN=1 npx tauri build) 2>&1 | tee "$log" >&2; then break; fi
  grep -qi linuxdeploy "$log" || { rm -f "$log"; die "tauri build failed"; }
  [ "$try" -lt 3 ] || { rm -f "$log"; die "the AppImage bundler (linuxdeploy) failed 3 times"; }
  say "release-linux: the AppImage bundler (linuxdeploy) failed, try $try of 3; again in 10 s"
  sleep 10
done
rm -f "$log"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/ainode-release.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$OUT"

# ── .deb ────────────────────────────────────────────────────────────────────
deb_in=$(ls "$BUNDLE"/deb/*.deb | head -1)
dpkg-deb -R "$deb_in" "$WORK/deb"
sed -i "s/^Package: .*/Package: $PKG/" "$WORK/deb/DEBIAN/control"
res="$WORK/deb/usr/lib/AInode/resources"
[ -d "$res/engine" ] || die "no resources/engine in $deb_in"
find "$res" "$WORK/deb/usr/share" -type f \( -name '*.json' -o -name '*.png' -o -name '*.txt' -o -name '*.cnf' -o -name LICENSE \) -exec chmod 644 {} +
check_engine "$res/engine" "deb" >&2 || die "the .deb's engine is not the staged one"
deb_out="$OUT/AInode_${VERSION}_amd64.deb"
dpkg-deb --root-owner-group -Zxz -b "$WORK/deb" "$deb_out" >/dev/null
[ "$(dpkg-deb -f "$deb_out" Package)" = "$PKG" ] || die "package name not set"

# ── AppImage ────────────────────────────────────────────────────────────────
app_in=$(ls "$BUNDLE"/appimage/*.AppImage | head -1)
offset=$(APPIMAGE_EXTRACT_AND_RUN=1 "$app_in" --appimage-offset)
[ "$offset" -gt 0 ] 2>/dev/null || die "could not read the AppImage's runtime size"
head -c "$offset" "$app_in" >"$WORK/runtime"
comp=$(unsquashfs -s -o "$offset" "$app_in" | awk '/^Compression/ {print $2}')
block=$(unsquashfs -s -o "$offset" "$app_in" | awk '/^Block size/ {print $3}')
unsquashfs -q -n -o "$offset" -d "$WORK/appdir" "$app_in" >/dev/null
app_engine="$WORK/appdir/usr/lib/AInode/resources/engine"
[ -d "$app_engine" ] || die "no resources/engine in $app_in"
rm -rf "$app_engine"
cp -a "$ENGINE" "$app_engine"
rm -f "$app_engine/.keep"
check_engine "$app_engine" "AppImage (before packing)" >&2
mksquashfs "$WORK/appdir" "$WORK/fs.squashfs" -comp "$comp" -b "$block" -root-owned -noappend -no-progress -quiet >/dev/null
app_out="$OUT/AInode_${VERSION}_amd64.AppImage"
cat "$WORK/runtime" "$WORK/fs.squashfs" >"$app_out"
chmod 755 "$app_out"

# Prove the packed AppImage, not the folder it was made from.
rm -rf "$WORK/check"
unsquashfs -q -n -o "$offset" -d "$WORK/check" "$app_out" >/dev/null
check_engine "$WORK/check/usr/lib/AInode/resources/engine" "AppImage" >&2 || die "the packed AppImage's engine is not the staged one"
[ -x "$WORK/check/AppRun" ] && [ -x "$WORK/check/usr/bin/ainode" ] || die "the packed AppImage lost AppRun or the app"

# No dev tooling in either (the dev bridge is a Cargo example and never
# bundled; this keeps it so).
for d in "$WORK/deb" "$WORK/check"; do
  if find "$d" -name 'devbridge*' | grep -q .; then die "a dev bridge is in $(basename "$d")"; fi
done

say ""
say "AInode $VERSION for Linux x86_64:"
(cd "$OUT" && sha256sum "$(basename "$deb_out")" "$(basename "$app_out")" | tee SHA256SUMS) >&2
