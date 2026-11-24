# The signed macOS release

`app/scripts/release-macos.sh` builds the public DMG: Developer ID signed,
notarized by Apple, stapled, so it opens with no Gatekeeper warning. First done
for 0.5.0 on 2026-10-13: both the app and the DMG came back `Accepted`,
`spctl` says `source=Notarized Developer ID`, and the signed app launched.

## Credentials

They are the maintainer's and never in a repository. `release-macos.sh` reads
`~/Bonuz-CodeSigning/notarize.env`: the team, the signing identity, the
dedicated keychain and its password, and the App Store Connect API key
(`.p8`) for notarization. Only the maintainer can make an official release; a
build from source is unsigned (`README.md`).

## Why not `tauri build` with the identity set

Tauri's bundler signs the main binary, frameworks and sidecars, and seals
`Contents/Resources` as data. AInode's engine lives in
`Contents/Resources/resources/engine`: upstream's `btx-modeld`, our OpenSSL
CLI, `llama-server` and ten dylibs. Tauri leaves those unsigned, and Apple's
notary refuses an unsigned Mach-O anywhere in the bundle. So the script:

1. builds unsigned and runs `check-bundle.sh` (the engine bytes are still
   upstream's: signing changes them, so the check comes first);
2. signs every engine Mach-O inside out, hardened runtime, timestamp, then
   fails if any Mach-O under Resources is not signed by the team (plain
   `grep ... >/dev/null`, never `grep -q` on a pipe: under `pipefail` the
   writer dies of SIGPIPE and a signed file reads as unsigned);
3. signs the app binary and the bundle with no entitlements (a restricted one
   makes a Developer ID build unlaunchable: pq-wallet shipped that once),
   notarizes a zip of the app, staples;
4. makes the DMG with `hdiutil` from the stapled app, signs, notarizes and
   staples it too (the DMG carries the quarantine flag on download);
5. launches the signed app for six seconds: a valid signature says nothing
   about starting.

Apple Silicon only, like the engine. The DMG and its sha256 are printed at the
end; copy both wherever the download is published.

## Publishing

Releases are published on `MendeMatthias/ainode-releases` (since 0.7.0), a
releases-only repository. The app's updater reads its feed from there.

## Updates (0.8.0)

Installed AInodes read
`https://github.com/MendeMatthias/ainode-releases/releases/latest/download/latest.json`,
the feed of the release marked Latest, and install what it offers only when
the file is signed by AInode's updater key and signed under its release name
(`app/src-tauri/src/update.rs`).

- **The key**, made once by `app/scripts/updater-key.sh`: the private key
  encrypted in `~/Bonuz-CodeSigning/ainode-updater.key`, its random password
  in the login Keychain ("AInode updater key"), the public key in
  `tauri.conf.json`. Back up both the key file and the password: lose either
  and no installed AInode can be updated again. It is AInode's own key, not
  easyBTX's or easyNode's.
- **The Mac file**: `release-macos.sh` packs the stapled app as
  `AInode_<v>_aarch64.app.tar.gz` and signs it (`updater-feed.py sign`).
- **Windows and Linux files** come from the Windows and Linux build machine; sign them here under their
  release names: `python3 app/scripts/updater-feed.py sign AInode_<v>_x64-setup.exe`
  (and `_amd64.AppImage`, `_amd64.deb`).
- **Publishing**: `app/scripts/publish-release.sh <v> --notes-file notes.md
  <files>` uploads the files and adds each updater file to the release's
  `latest.json`, keeping the platforms already there. The feed has one version
  for all platforms: `darwin-aarch64`, `windows-x86_64`, `linux-x86_64`
  (AppImage) and `linux-x86_64-deb` (a .deb install looks for this key first).
- **A re-uploaded `latest.json` reaches apps late**: GitHub's asset CDN served
  the replaced copy for about two minutes in the scratch test. Never replace a
  file a feed already names; add new files only.

Proven on 2026-10-14 against a scratch releases repo
(`MendeMatthias/ainode-releases-scratch`, test key, debug builds with
`AINODE_UPDATE_SELFTEST`, a scratch data folder and socket): a feed signed
under the bundler's name `AInode.app.tar.gz` was refused before download; a
tarball with one byte changed was refused at download, with nothing stopped;
the genuine one replaced the 0.7.9 app with the 0.8.0 bundle, identical file
for file, and the new binary started. A tarball of the notarized app unpacks
with its signature and staple intact (`codesign --deep --strict`, `spctl`:
Notarized Developer ID).

## Windows (Authenticode)

The Windows files were unsigned up to 0.8.2 (security audit 2026-10-18, M2:
`Get-AuthenticodeSignature ainode.exe` said `NotSigned`). Updates were never
at risk: every update is checked against AInode's updater key (above). A
first download was protected only by GitHub's HTTPS and the sha256 lines in
the release notes, and SmartScreen warns on every new version.

`app/scripts/release-windows.ps1` signs when it is given a sign command,
`-SignCommand "<command> %1"` or `$env:AINODE_SIGN_COMMAND` (`%1` is the
file). Tauri runs it on `ainode.exe`, NSIS's plugins, the uninstaller and the
installer; the script then refuses to finish unless the installer carries a
valid signature and the `ainode.exe` packed in it one by the same
certificate, and prints `UNSIGNED` when no command was given. The packed copy
is read from the installer's payload (`app/scripts/nsis-packed-signed.py`,
nothing is run): Tauri puts the unsigned build back in `target
elease` after
packing, so that file says `NotSigned` even in a signed build. The engine's files keep the bytes `MANIFEST.json` pins
(upstream's `llama-server.exe` and DLLs as built, Microsoft's runtime DLLs
with Microsoft's own signature). Tested 2026-10-19 with a throwaway
self-signed certificate (deleted afterwards): the installer and the packed
`ainode.exe` signed by it, and the published unsigned 0.8.2 installer
reported `NOT signed`.

The certificate is the maintainer's to get:

- **Azure Trusted Signing** (Microsoft's service, about USD 10 a month). The
  sign command is Microsoft's `signtool` with its Trusted Signing client
  (`signtool sign /fd sha256 /tr http://timestamp.acs.microsoft.com /td
  sha256 /dlib <Azure.CodeSigning.Dlib.dll> /dmdf <metadata.json> %1`) or
  `trusted-signing-cli -e <endpoint> -a <account> -c <profile> %1`, with the
  Azure credentials in the environment. Microsoft checks the publisher's
  identity once and limits who may sign up by country: check its current
  list at sign-up.
- **An OV code-signing certificate** from a CA, ideally with cloud signing
  (the key stays in the CA's HSM, nothing to plug in): the CA's tool or
  `signtool sign /fd sha256 /tr <the CA's timestamp URL> /td sha256 /sha1
  <thumbprint> %1`. SmartScreen still warns until the certificate has
  reputation, but the publisher's name shows and any change to the file
  breaks the signature.
- **The Microsoft Store (MSIX)**: Microsoft signs it for free; a larger
  change (docs/windows.md, "Signing and distribution").

Whatever signs: put `signtool`'s folder (Windows SDK) on `PATH`, timestamp
every signature (`/tr`), keep the credentials out of the repository, and
check a released file with `Get-AuthenticodeSignature` (`Valid`).
