#!/usr/bin/env bash
# The public macOS release: Developer ID signed, notarized by Apple, stapled, so
# it opens with no Gatekeeper warning. Apple Silicon only (the bundled engine is
# arm64: btx-modeld built from upstream's tag, llama.cpp).
#
#   app/scripts/release-macos.sh              # signed + notarized (the real thing)
#   app/scripts/release-macos.sh --sign-only  # signed, NOT notarized: local checks only
#
# The updater (plan-0.8.0.md 3.1): the stapled app is also packed as
# AInode_<version>_aarch64.app.tar.gz and signed under that name with
# AInode's updater key (scripts/updater-key.sh made it; updater-feed.py signs),
# which is what the app's update.rs requires. scripts/publish-release.sh
# uploads it with the DMG and adds it to the feed.
#
# AINODE_TAURI_CONFIG=<file.json> merges a config into the build (`tauri build
# --config`): the scratch updater test only (another version, another feed,
# a throwaway key). The real release reads tauri.conf.json alone.
#
# Credentials: ~/Bonuz-CodeSigning/notarize.env (never in the repo), the same
# file pq-wallet's release uses: APPLE_TEAM_ID, APPLE_SIGNING_IDENTITY,
# SIGNING_KEYCHAIN + SIGNING_KEYCHAIN_PASSWORD (a dedicated keychain), and for
# notarization APPLE_API_ISSUER, APPLE_API_KEY, APPLE_API_KEY_PATH (.p8).
#
# Why the signing is done here and not by `tauri build`: Tauri signs the main
# binary, frameworks and sidecars, and seals Resources as data. The engine lives
# in Contents/Resources/resources/engine (btx-modeld, llama-server and
# its dylibs), which Tauri would leave unsigned, and Apple's notary refuses an
# unsigned Mach-O anywhere in the bundle. So:
#   1. build unsigned, run check-bundle.sh (the engine bytes are the staged,
#      pinned ones: the helper built from upstream's tag, llama.cpp's release);
#   2. sign every engine Mach-O inside out, hardened runtime, timestamp;
#   3. sign the app (no entitlements: a restricted one makes a Developer ID
#      build unlaunchable, pq-wallet docs/SIGNING.md), notarize, staple;
#   4. make the DMG from the stapled app, sign, notarize and staple it too
#      (Tauri never notarizes the DMG itself, and the DMG is what carries the
#      quarantine flag on download);
#   5. launch the signed app: a valid signature says nothing about starting.
set -euo pipefail

cd "$(dirname "$0")/.."
SIGN_ONLY=0
[[ "${1:-}" == "--sign-only" ]] && SIGN_ONLY=1

ENV_FILE="$HOME/Bonuz-CodeSigning/notarize.env"
[[ -f "$ENV_FILE" ]] || { echo "error: $ENV_FILE not found (see pq-wallet docs/SIGNING.md)" >&2; exit 1; }
# shellcheck disable=SC1090
set -a; source "$ENV_FILE"; set +a
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID not set in $ENV_FILE}"
: "${APPLE_SIGNING_IDENTITY:?APPLE_SIGNING_IDENTITY not set in $ENV_FILE}"

if pgrep -x ainode >/dev/null 2>&1; then
  echo "error: AInode is running. Quit it first: the launch check at the end would hand off to it." >&2
  exit 1
fi

# The updater key: the app trusts the public key in tauri.conf.json, and the
# tarball must be signed by its private half.
CONF_PUBKEY=$(python3 -c 'import json;print(json.load(open("src-tauri/tauri.conf.json"))["plugins"]["updater"]["pubkey"])')
TAURI_CONFIG=()
if [[ -n "${AINODE_TAURI_CONFIG:-}" ]]; then
  [[ -f "$AINODE_TAURI_CONFIG" ]] || { echo "error: $AINODE_TAURI_CONFIG not found" >&2; exit 1; }
  echo "==> Test build: config merged from $AINODE_TAURI_CONFIG (do NOT publish this build)"
  TAURI_CONFIG=(--config "$AINODE_TAURI_CONFIG")
else
  [[ "$CONF_PUBKEY" == "$(cat "$HOME/Bonuz-CodeSigning/ainode-updater.key.pub" 2>/dev/null)" ]] || {
    echo "error: tauri.conf.json's updater pubkey is not ~/Bonuz-CodeSigning/ainode-updater.key.pub (run scripts/updater-key.sh once)" >&2; exit 1; }
fi

echo "==> Signing identity: $APPLE_SIGNING_IDENTITY"
if [[ -n "${SIGNING_KEYCHAIN:-}" ]]; then
  security unlock-keychain -p "${SIGNING_KEYCHAIN_PASSWORD:?}" "$SIGNING_KEYCHAIN"
  # codesign searches the login keychain unless the dedicated one is in the list.
  # Added once: prepending it on every run filled the list with copies (nine
  # by 2026-10-17). Each entry once, in its order.
  KEYCHAINS=$(security list-keychains -d user | tr -d '" ' | awk 'NF && !seen[$0]++')
  case "$KEYCHAINS" in
    *"/${SIGNING_KEYCHAIN##*/}"*) ;;
    *) KEYCHAINS="$SIGNING_KEYCHAIN"$'\n'"$KEYCHAINS" ;;
  esac
  # shellcheck disable=SC2086
  security list-keychains -d user -s $KEYCHAINS
fi
# Not `grep -q`: under pipefail security dies of SIGPIPE when grep stops
# reading early, and a present identity reads as missing (seen 2026-10-17).
security find-identity -v -p codesigning | grep "$APPLE_TEAM_ID" >/dev/null \
  || { echo "error: no signing identity for team $APPLE_TEAM_ID in the keychains." >&2; exit 1; }

if [[ $SIGN_ONLY -eq 0 ]]; then
  : "${APPLE_API_ISSUER:?APPLE_API_ISSUER not set}"
  : "${APPLE_API_KEY:?APPLE_API_KEY not set}"
  : "${APPLE_API_KEY_PATH:?APPLE_API_KEY_PATH not set}"
  [[ -f "$APPLE_API_KEY_PATH" ]] || { echo "error: API key missing at $APPLE_API_KEY_PATH" >&2; exit 1; }
  echo "==> Notarization: key $APPLE_API_KEY (issuer ${APPLE_API_ISSUER:0:8}…)"
else
  echo "==> --sign-only: no notarization (do NOT publish this build)"
fi
NOTARY=(xcrun notarytool submit --key "${APPLE_API_KEY_PATH:-}" --key-id "${APPLE_API_KEY:-}" --issuer "${APPLE_API_ISSUER:-}" --wait)

# --- 1. Build unsigned and check the bundle ------------------------------------------
echo
echo "==> Building (unsigned)"
# Unset so the Tauri bundler does not sign or notarize on its own.
env -u APPLE_SIGNING_IDENTITY -u APPLE_API_ISSUER -u APPLE_API_KEY -u APPLE_API_KEY_PATH -u APPLE_ID -u APPLE_PASSWORD \
  npx tauri build --bundles app ${TAURI_CONFIG[@]+"${TAURI_CONFIG[@]}"}
BUNDLE="src-tauri/target/release/bundle"
APP="$BUNDLE/macos/AInode.app"
[[ -d "$APP" ]] || { echo "error: no .app at $APP" >&2; exit 1; }
echo
echo "==> check-bundle (before signing: the engine bytes must still be upstream's)"
scripts/check-bundle.sh "$APP"

# --- 2. Sign the engine, inside out ----------------------------------------------------
VERSION=$(/usr/libexec/PlistBuddy -c "Print CFBundleShortVersionString" "$APP/Contents/Info.plist")
ENGINE="$APP/Contents/Resources/resources/engine"
sign_file() { codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$1"; }
echo
echo "==> Signing the engine ($VERSION): dylibs, then executables"
for f in "$ENGINE"/llama/*.dylib; do sign_file "$f"; done
for f in "$ENGINE"/llama/llama-server "$ENGINE"/btx-modeld; do sign_file "$f"; done
# Nothing else executable may be left unsigned (the notary would say so, hours
# later). No `grep -q` on a pipe here: under pipefail the writer dies of SIGPIPE
# when grep stops reading, and a signed file is reported as unsigned.
while IFS= read -r f; do
  file -b "$f" | grep "Mach-O" >/dev/null || continue
  if ! codesign -dv "$f" 2>&1 | grep "TeamIdentifier=$APPLE_TEAM_ID" >/dev/null; then
    echo "error: unsigned Mach-O in the bundle: $f" >&2; exit 1
  fi
done < <(find "$APP/Contents/Resources" -type f)

# --- 3. Sign the app, notarize, staple ------------------------------------------------
echo
echo "==> Signing the app"
sign_file "$APP/Contents/MacOS/ainode"
sign_file "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
codesign -dvvv "$APP" 2>&1 | grep -E "Authority|TeamIdentifier|flags|Identifier=" | sed 's/^/    /'
if [[ $SIGN_ONLY -eq 0 ]]; then
  echo
  echo "==> Notarizing the app (a few minutes)"
  ZIP="$BUNDLE/macos/AInode-$VERSION-notarize.zip"
  ditto -c -k --keepParent "$APP" "$ZIP"
  "${NOTARY[@]}" "$ZIP"
  rm -f "$ZIP"
  xcrun stapler staple "$APP"
  echo "==> Gatekeeper assessment (want: source=Notarized Developer ID)"
  spctl -a -vvv -t install "$APP"
fi

# --- 3b. The updater's tarball, from the stapled app ------------------------------------
echo
echo "==> The updater tarball"
TARBALL="$BUNDLE/macos/AInode_${VERSION}_aarch64.app.tar.gz"
rm -f "$TARBALL" "$TARBALL.sig"
COPYFILE_DISABLE=1 tar -czf "$TARBALL" -C "$BUNDLE/macos" AInode.app
python3 scripts/updater-feed.py sign "$TARBALL"

# --- 4. The DMG ------------------------------------------------------------------------
echo
echo "==> Making the DMG"
DMG_DIR="$BUNDLE/dmg"
mkdir -p "$DMG_DIR"
DMG="$DMG_DIR/AInode_${VERSION}_aarch64.dmg"
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -quiet -volname "AInode" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE"
codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$DMG"
if [[ $SIGN_ONLY -eq 0 ]]; then
  echo "==> Notarizing the DMG"
  "${NOTARY[@]}" "$DMG"
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
  spctl -a -vvv -t open --context context:primary-signature "$DMG"
fi

# --- 5. Does it start? -----------------------------------------------------------------
echo
echo "==> Launch check (the signed bundle must start and stay up)"
# In an empty HOME: it stops at the consent screen and writes nothing of this
# Mac's user. With the real, consented profile 0.8.1 would add a LaunchAgent
# naming this build folder (login.rs refreshes it at each start).
LAUNCH_HOME=$(mktemp -d)
HOME="$LAUNCH_HOME" "$APP/Contents/MacOS/ainode" >/dev/null 2>&1 &
LAUNCH_PID=$!
sleep 6
if kill -0 "$LAUNCH_PID" 2>/dev/null; then
  # Quit it the way a user does, so it stops the engine it may have started
  # (a SIGTERM skips the app's quit path and leaves the helper running from
  # the build folder, seen 2026-10-14). Only this copy runs: the script
  # refused to start while AInode was open.
  osascript -e 'tell application id "com.m2promende.ainode" to quit' >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do kill -0 "$LAUNCH_PID" 2>/dev/null || break; sleep 1; done
  kill "$LAUNCH_PID" 2>/dev/null || true
  wait "$LAUNCH_PID" 2>/dev/null || true
  pkill -TERM -f "$APP/Contents/Resources/resources/engine/" 2>/dev/null || true
  echo "    launched and stayed running; quit again"
  # Nothing before consent (rule 1): the empty HOME must still be empty.
  LEFT=$(find "$LAUNCH_HOME" -mindepth 1 | sed "s|^$LAUNCH_HOME|~|")
  if [[ -n "$LEFT" ]]; then
    echo "error: on the consent screen it wrote into HOME:" >&2; echo "$LEFT" >&2
    echo "       DO NOT PUBLISH THIS BUILD." >&2
    exit 1
  fi
  rm -rf "$LAUNCH_HOME"
  echo "    nothing written into HOME before consent"
else
  wait "$LAUNCH_PID" 2>/dev/null; RC=$?
  echo "error: the signed app does not launch (exit $RC). 137 = SIGKILL, usually a restricted entitlement." >&2
  echo "       DO NOT PUBLISH THIS BUILD." >&2
  exit 1
fi

echo
echo "==> Artifacts"
shasum -a 256 "$DMG" "$TARBALL"
ls -lh "$DMG" "$TARBALL" "$TARBALL.sig"
echo
echo "Verify on another Mac by downloading the DMG over the network (a local file has no quarantine flag)."
