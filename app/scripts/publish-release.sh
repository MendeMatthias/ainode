#!/usr/bin/env bash
# Publish AInode files to a release on ainode-releases and keep its updater
# feed whole (plan-0.8.0.md 3.1).
#
#   app/scripts/publish-release.sh <version> [--repo OWNER/NAME] [--notes-file F] [--draft] <file>...
#
# Each file is uploaded to the release v<version> (made if it does not exist
# yet, with the notes file). Updater files, each beside its .sig, also get
# their entry in the release's latest.json:
#   AInode_<v>_aarch64.app.tar.gz  darwin-aarch64    (release-macos.sh)
#   AInode_<v>_x64-setup.exe       windows-x86_64    (Track W)
#   AInode_<v>_amd64.AppImage      linux-x86_64      (Track W)
#   AInode_<v>_amd64.deb           linux-x86_64-deb  (Track W)
# Anything else (the DMG) is uploaded as it is. The feed already on the
# release is read first and only the given platforms change, so the Mac can
# open a release and Windows and Linux join it later, or the other way round.
# updater-feed.py refuses any entry the app would refuse.
#
# A Track W file that arrives without a .sig is signed here first (the key
# lives only on this Mac): `python3 app/scripts/updater-feed.py sign <file>`.
#
# The installed apps read .../releases/latest/download/latest.json: the feed of
# the release marked Latest. A draft or a pre-release is not read by anyone.
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION="${1:?usage: publish-release.sh <version> [--repo R] [--notes-file F] [--draft] <file>...}"
shift
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "error: $VERSION is not a release version" >&2; exit 1; }
REPO="MendeMatthias/ainode-releases"
NOTES=""
DRAFT=()
FILES=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) REPO="$2"; shift 2 ;;
    --notes-file) NOTES="$2"; shift 2 ;;
    --draft) DRAFT=(--draft); shift ;;
    *) FILES+=("$1"); shift ;;
  esac
done
[[ ${#FILES[@]} -gt 0 ]] || { echo "error: no files to publish" >&2; exit 1; }

platform_of() {
  case "$(basename "$1")" in
    "AInode_${VERSION}_aarch64.app.tar.gz") echo darwin-aarch64 ;;
    "AInode_${VERSION}_x64-setup.exe") echo windows-x86_64 ;;
    "AInode_${VERSION}_amd64.AppImage") echo linux-x86_64 ;;
    "AInode_${VERSION}_amd64.deb") echo linux-x86_64-deb ;;
    *) echo "" ;;
  esac
}

TAG="v$VERSION"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

if ! gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  [[ -n "$NOTES" ]] || { echo "error: $TAG does not exist on $REPO yet: give --notes-file" >&2; exit 1; }
  echo "==> Making $TAG on $REPO"
  gh release create "$TAG" --repo "$REPO" --title "AInode $VERSION" --notes-file "$NOTES" ${DRAFT[@]+"${DRAFT[@]}"}
fi

# The feed as it is on the release now, if it has one.
if gh release view "$TAG" --repo "$REPO" --json assets -q '.assets[].name' | grep -qx latest.json; then
  gh release download "$TAG" --repo "$REPO" --pattern latest.json --dir "$WORK"
  echo "==> The release's feed has: $(python3 -c 'import json,sys;print(", ".join(sorted(json.load(open(sys.argv[1]))["platforms"])))' "$WORK/latest.json")"
fi

UPLOAD=()
FEED_CHANGED=0
for f in "${FILES[@]}"; do
  [[ -f "$f" ]] || { echo "error: $f not found" >&2; exit 1; }
  p=$(platform_of "$f")
  UPLOAD+=("$f")
  if [[ -n "$p" ]]; then
    [[ -f "$f.sig" ]] || { echo "error: $f has no .sig: python3 scripts/updater-feed.py sign $f" >&2; exit 1; }
    python3 scripts/updater-feed.py add "$WORK/latest.json" "$p" "$f" --version "$VERSION" --repo "$REPO"
    UPLOAD+=("$f.sig")
    FEED_CHANGED=1
  fi
done
[[ $FEED_CHANGED -eq 1 ]] && UPLOAD+=("$WORK/latest.json")

echo "==> Uploading to $TAG"
shasum -a 256 "${FILES[@]}"
gh release upload "$TAG" --repo "$REPO" --clobber "${UPLOAD[@]}"
echo "==> Done: https://github.com/$REPO/releases/tag/$TAG"
