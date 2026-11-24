#!/usr/bin/env bash
# AInode's updater key, made once on the release Mac (plan-0.8.0.md 3.1).
#
#   app/scripts/updater-key.sh
#
# Makes a minisign key pair with `tauri signer generate`:
#   ~/Bonuz-CodeSigning/ainode-updater.key      the private key, encrypted
#   ~/Bonuz-CodeSigning/ainode-updater.key.pub  the public key
# Its password is random and goes only into the login Keychain (service
# "AInode updater key"), where release-macos.sh and updater-feed.py read it:
# nobody types it. The public key is written into tauri.conf.json, so every
# AInode built after this trusts updates signed with this key and no other.
#
# Back up BOTH the key file and the Keychain item (or copy the password into
# a password manager): lose either and no installed AInode can be updated
# again; users would have to download by hand. Never put either in a repo.
set -euo pipefail

cd "$(dirname "$0")/.."
DIR="$HOME/Bonuz-CodeSigning"
KEY="$DIR/ainode-updater.key"
SERVICE="AInode updater key"

[[ -d "$DIR" ]] || { echo "error: $DIR not found" >&2; exit 1; }
if [[ -e "$KEY" ]]; then
  echo "error: $KEY exists already. One key for the life of AInode: refusing to make another." >&2
  exit 1
fi
if security find-generic-password -s "$SERVICE" >/dev/null 2>&1; then
  echo "error: a Keychain item \"$SERVICE\" exists already, without its key file. Look before deleting it." >&2
  exit 1
fi

PW=$(openssl rand -base64 33 | tr -d '\n=+/')
# -T: the `security` tool may read it without a prompt (the release scripts).
security add-generic-password -a ainode -s "$SERVICE" -l "AInode updater signing key password" \
  -T /usr/bin/security -w "$PW"
npx tauri signer generate --ci -p "$PW" -w "$KEY" >/dev/null
unset PW
chmod 600 "$KEY" "$KEY.pub"

PUB=$(cat "$KEY.pub")
# A text edit, so the file keeps its layout; the placeholder must be there once.
python3 - "$PUB" <<'EOF'
import sys
p = "src-tauri/tauri.conf.json"
s = open(p).read()
old = '"pubkey": "NO-KEY-YET: run app/scripts/updater-key.sh"'
if s.count(old) != 1:
    sys.exit("error: tauri.conf.json has no updater key placeholder (a key is set already?)")
open(p, "w").write(s.replace(old, '"pubkey": "%s"' % sys.argv[1]))
EOF

echo "Made $KEY (encrypted) and $KEY.pub; the password is in the Keychain as \"$SERVICE\"."
echo "tauri.conf.json now trusts this key. Commit that change."
echo "Back up both: the key file and the Keychain item."
