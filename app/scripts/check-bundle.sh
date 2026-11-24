#!/bin/bash
# Fail a release whose app bundle carries anything it should not. Run after
# `npx tauri build`; exits non-zero with the reason.
#
#   - Contents/MacOS must hold the app binary and nothing else: the Tauri
#     bundler copies EVERY Cargo bin target there, which is how the dev-only
#     devbridge once shipped inside the app (it is an example now).
#   - The engine resources must match the MANIFEST.json stage-engine.sh wrote.
#     Run it before Developer ID signing, which changes the Mach-O bytes.
set -euo pipefail
APP="${1:-$(dirname "$0")/../src-tauri/target/release/bundle/macos/AInode.app}"
[ -d "$APP" ] || { echo "no app at $APP" >&2; exit 1; }
fail=0
extra=$(ls "$APP/Contents/MacOS" | grep -vx ainode || true)
if [ -n "$extra" ]; then
  echo "FAIL: Contents/MacOS has more than the app binary: $extra" >&2
  fail=1
fi
ENG="$APP/Contents/Resources/resources/engine"
if [ -f "$ENG/MANIFEST.json" ]; then
  # Every file the MANIFEST names must be in the bundle with that sha256.
  if ! /usr/bin/python3 -I - "$ENG" "$(dirname "$0")/stage-engine.sh" <<'PY'
import hashlib, json, os, sys
eng = sys.argv[1]
bad = []
for f in json.load(open(os.path.join(eng, "MANIFEST.json")))["files"]:
    p = os.path.join(eng, f["path"])
    h = hashlib.sha256()
    try:
        with open(p, "rb") as fh:
            for b in iter(lambda: fh.read(1 << 20), b""):
                h.update(b)
    except OSError as e:
        bad.append(f"{f['path']}: {e}")
        continue
    if h.hexdigest() != f["sha256"]:
        bad.append(f"{f['path']}: sha256 differs")
# What Status shows about the engine comes from this manifest: it must say
# it, and say the tag stage-engine.sh pins.
m = json.load(open(os.path.join(eng, "MANIFEST.json")))
pin = None
for line in open(sys.argv[2]):
    if line.startswith("BTX_TAG="):
        pin = line.strip().split("=", 1)[1]
e = m.get("engine") or {}
if e.get("btx_version") != pin or e.get("helper") != "built-by-ainode":
    bad.append(f"MANIFEST.json engine is {e!r}, stage-engine.sh pins {pin!r} (built-by-ainode)")
for b in bad:
    print("  " + b, file=sys.stderr)
sys.exit(1 if bad else 0)
PY
  then
    echo "FAIL: engine files differ from their MANIFEST" >&2
    fail=1
  fi
else
  echo "FAIL: no engine MANIFEST.json in the bundle" >&2
  fail=1
fi
if strings "$APP/Contents/MacOS/ainode" | grep -q "127.0.0.1:1455"; then
  echo "FAIL: the app binary contains the dev bridge address" >&2
  fail=1
fi
[ "$fail" = 0 ] && echo "bundle OK: $(ls "$APP/Contents/MacOS"), engine matches MANIFEST"
exit "$fail"
