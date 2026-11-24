#!/usr/bin/env python3
"""AInode's updater files: sign, verify, and the feed (plan-0.8.0.md 3.1).

  updater-feed.py sign   <file>                        # <file>.sig, under its own name
  updater-feed.py verify <file>                        # like the app will, before download
  updater-feed.py add    <feed.json> <platform> <file> --version V [--notes N] [--repo R]

`add` puts one platform's entry into `latest.json` and keeps every other
entry, so Windows and Linux can join a release the Mac opened (and the other
way round). It refuses a file whose name, signature or version does not match
what the app's update.rs will accept: a feed the app refuses stops every
update. The feed has one version for all platforms.

Signing uses `npx tauri signer sign` with the key in ~/Bonuz-CodeSigning and
its password from the login Keychain (app/scripts/updater-key.sh made both).
AINODE_UPDATER_KEY and AINODE_UPDATER_PASSWORD override them, for the scratch
test only; the public key is read from tauri.conf.json unless
AINODE_UPDATER_PUBKEY is set, also for that test.

Signature format (the same as easyNode's verify-updater-sig.py): a Tauri .sig
is base64 of a minisign file; its signature line is base64 of
alg[2] || key_id[8] || ed25519_sig[64], `ED` signing BLAKE2b-512 of the file,
`Ed` the file itself; the global signature covers sig || trusted comment, so
the `file:` name in the comment is authenticated too.
"""

import base64
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

APP = Path(__file__).resolve().parent.parent
CONF = APP / "src-tauri" / "tauri.conf.json"
KEY = Path.home() / "Bonuz-CodeSigning" / "ainode-updater.key"
KEYCHAIN_SERVICE = "AInode updater key"
DEFAULT_REPO = "MendeMatthias/ainode-releases"
PREFIX = "AInode_"
# The same table as update.rs PLATFORM_SUFFIXES (a Rust test checks the two).
SUFFIXES = {
    "darwin-aarch64": "_aarch64.app.tar.gz",
    "linux-x86_64": "_amd64.AppImage",
    "linux-x86_64-deb": "_amd64.deb",
    "windows-x86_64": "_x64-setup.exe",
}
VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")


def die(msg):
    sys.exit(f"error: {msg}")


def b64(value, what):
    try:
        return base64.b64decode(value.strip(), validate=True)
    except Exception:
        die(f"{what} is not valid base64")


def expected_name(platform, version):
    if platform not in SUFFIXES:
        die(f"{platform} is not a platform AInode ships ({', '.join(SUFFIXES)})")
    return f"{PREFIX}{version}{SUFFIXES[platform]}"


def pubkey():
    text = os.environ.get("AINODE_UPDATER_PUBKEY") or json.loads(CONF.read_text())["plugins"]["updater"]["pubkey"]
    if text.startswith("NO-KEY-YET"):
        die("tauri.conf.json has no updater key yet: run app/scripts/updater-key.sh once")
    lines = [l for l in b64(text, "updater pubkey").decode().splitlines() if l.strip()]
    raw = b64(lines[-1], "updater pubkey body")
    if len(raw) != 42 or raw[:2] != b"Ed":
        die("the updater pubkey is not a minisign Ed25519 key")
    return raw[2:10], Ed25519PublicKey.from_public_bytes(raw[10:])


def read_sig(sig_text):
    lines = [l for l in b64(sig_text, "signature").decode("utf-8", "replace").splitlines() if l.strip()]
    if len(lines) < 4 or not lines[2].startswith("trusted comment: "):
        die("the signature is not a minisign file")
    raw = b64(lines[1], "signature line")
    if len(raw) != 74:
        die("the signature line has the wrong length")
    trusted = lines[2][len("trusted comment: "):].encode()
    return raw[:2], raw[2:10], raw[10:], trusted, b64(lines[3], "global signature")


def signed_name(trusted):
    for field in trusted.decode("utf-8", "replace").split("\t"):
        if field.startswith("file:"):
            return field[5:]
    return None


def verify(path, sig_text=None):
    """The file's signature is the app's key, over these bytes, under this name."""
    path = Path(path)
    sig_text = sig_text if sig_text is not None else Path(f"{path}.sig").read_text()
    key_id, key = pubkey()
    alg, sig_id, sig, trusted, global_sig = read_sig(sig_text)
    if sig_id != key_id:
        die(f"{path.name} is signed with key {sig_id.hex().upper()}, the app trusts {key_id.hex().upper()}")
    data = path.read_bytes()
    if alg == b"ED":
        data = hashlib.blake2b(data, digest_size=64).digest()
    elif alg != b"Ed":
        die(f"unknown signature algorithm {alg!r}")
    try:
        key.verify(sig, data)
        key.verify(global_sig, sig + trusted)
    except InvalidSignature:
        die(f"{path.name}: the signature does not match")
    name = signed_name(trusted)
    if name != path.name:
        die(f"{path.name} is signed as {name!r}: sign it under its release name")
    return name


def password():
    if "AINODE_UPDATER_PASSWORD" in os.environ:
        return os.environ["AINODE_UPDATER_PASSWORD"]
    r = subprocess.run(["security", "find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"],
                       capture_output=True, text=True)
    if r.returncode != 0:
        die(f'no Keychain item "{KEYCHAIN_SERVICE}": run app/scripts/updater-key.sh once')
    return r.stdout.rstrip("\n")


def sign(path):
    path = Path(path)
    if not re.match(rf"^{PREFIX}\d+\.\d+\.\d+_", path.name) or not any(path.name.endswith(s) for s in SUFFIXES.values()):
        die(f"{path.name} is not a release name (AInode_<version><platform suffix>)")
    key = os.environ.get("AINODE_UPDATER_KEY", str(KEY))
    env = dict(os.environ, TAURI_SIGNING_PRIVATE_KEY_PASSWORD=password())
    env.pop("TAURI_SIGNING_PRIVATE_KEY", None)
    r = subprocess.run(["npx", "tauri", "signer", "sign", "-f", key, str(path)],
                       cwd=APP, env=env, capture_output=True, text=True)
    if r.returncode != 0:
        die(f"signing {path.name} failed:\n{r.stdout}{r.stderr}")
    verify(path)
    print(f"signed {path.name}")


def add(feed_path, platform, path, version, notes, repo):
    path = Path(path)
    if not VERSION_RE.match(version):
        die(f"{version} is not a release version")
    want = expected_name(platform, version)
    if path.name != want:
        die(f"{platform} at {version} must be {want}, not {path.name}")
    sig_text = Path(f"{path}.sig").read_text().strip()
    verify(path, sig_text)
    feed_path = Path(feed_path)
    feed = json.loads(feed_path.read_text()) if feed_path.exists() else None
    if feed is None:
        feed = {"version": version, "notes": notes or f"AInode {version}",
                "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "platforms": {}}
    elif feed.get("version") != version:
        die(f"the feed is for {feed.get('version')}, not {version}: one version for all platforms")
    if notes:
        feed["notes"] = notes
    feed["platforms"][platform] = {
        "signature": sig_text,
        "url": f"https://github.com/{repo}/releases/download/v{version}/{want}",
    }
    # What update.rs checks, over the whole feed: every entry bound.
    for key, entry in feed["platforms"].items():
        name = signed_name(read_sig(entry["signature"])[3])
        if name != expected_name(key, version):
            die(f"the feed's {key} entry is signed as {name!r}, not {expected_name(key, version)}")
    feed_path.write_text(json.dumps(feed, indent=2) + "\n")
    print(f"{feed_path}: {version} for {', '.join(sorted(feed['platforms']))}")


def main(argv):
    if len(argv) >= 2 and argv[1] == "sign" and len(argv) == 3:
        return sign(argv[2])
    if len(argv) >= 2 and argv[1] == "verify" and len(argv) == 3:
        print(f"OK {verify(argv[2])}")
        return
    if len(argv) >= 5 and argv[1] == "add":
        opts = {"--version": None, "--notes": None, "--repo": DEFAULT_REPO}
        rest = argv[5:]
        while rest:
            if rest[0] not in opts or len(rest) < 2:
                die(f"unknown option {rest[0]}")
            opts[rest[0]] = rest[1]
            rest = rest[2:]
        if not opts["--version"]:
            die("--version is required")
        return add(argv[2], argv[3], argv[4], opts["--version"], opts["--notes"], opts["--repo"])
    sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
