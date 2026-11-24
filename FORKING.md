# Forking AInode

Forks are welcome. More independent doors into BTX's model network make it
harder to break, and that is what the MIT licence is for.

This page is the short list: the things a fork must change so it does not
collide with this app on a user's computer or speak in its name, two files a
fork may not simply keep, and what a fork can keep as it is.

## Change these five

**1. The name and icon.** `productName` and the window `title` in
`app/src-tauri/tauri.conf.json` (the title is repeated in
`tauri.linux.conf.json` and `tauri.windows.conf.json`), and the files in
`app/src-tauri/icons/`. The licence covers the code, not the AInode or easyBTX
name or logo ([CONTRIBUTING.md](CONTRIBUTING.md#a-note-on-the-name)). A user
should always be able to tell whose build they are running.

**2. The app identifier.** `identifier` in `tauri.conf.json`, today
`com.m2promende.ainode`. The operating system keeps an app's settings and
storage under its identifier, so two apps with the same one share them. The
same string is the Mac login item in `app/src-tauri/src/login.rs`
(`com.m2promende.ainode.plist` and its label) and is named once in
`app/scripts/release-macos.sh`.

**3. The folders and names on the computer.** Two apps that share these stop
each other's engine and read each other's models:

- the data folder `AInode` (`app/src-tauri/src/state.rs`:
  `~/Library/Application Support/AInode`, `$XDG_DATA_HOME/AInode`,
  `%LOCALAPPDATA%\AInode\data`);
- the engine's control socket `<temp>/ainode/modeld.sock` (`state.rs`), and
  inside WSL `/tmp/ainode/modeld.sock` (`app/src-tauri/src/wsl.rs`);
- on Windows, the WSL distribution `AInode` (`wsl.rs`).

**4. The update feed and its key.** `plugins.updater.endpoints` and
`plugins.updater.pubkey` in `tauri.conf.json`, and `ASSET_PREFIX` in
`app/src-tauri/src/update.rs`: an update is taken only when it is signed under
its exact release name, `AInode_<version>_<platform>`. If you keep the feed
and the key, your users are offered our next release, and installing it
replaces your app with ours. If you keep our feed with your own key, each of
our releases shows your users an update that is then refused. So:

- Generate your own key pair, from `app`: `npx tauri signer generate`
  (`app/scripts/updater-key.sh` shows how ours is kept).
- Give it a passphrase, and keep the private half out of the repository.
  `*.key` and `*.pem` are ignored here, and CI fails on key material.
- Publish your own feed and point the endpoint at it, or turn the updater off.
  `app/scripts/updater-feed.py` and `app/scripts/publish-release.sh` name our
  releases repository; so does the feedback link (`FEEDBACK_REPO` in
  `app/src/logic.ts`).

The endpoint is compiled into every build, so settle it before your first
release: changing it later strands every copy already installed.
[docs/SIGNING.md](docs/SIGNING.md) explains how ours works.

**5. The check-in.** Every 5 minutes an open AInode tells easybtx.com that it
is open, unless the user turned "Count me" off: a random number for the
install, the version, the platform, whether the engine runs, and how many
models and hosts it has (`CHECKIN_URL` and `agent` in
`app/src-tauri/src/backend/keeper.rs`). The number shown in the app is read
from the same site (`app/src/api.ts`), and `tauri.conf.json` allows the web
view to connect to it (`connect-src`). That count is of AInodes. Point your
fork at your own address or take the check-in out; at the least change
`agent`, today `ainode/<version>`, so your installs are not counted as ours.

## Two files you may not simply keep

- **`app/src/knowledge/pq-wiki.json`** is not under the MIT licence. It is
  postquantum.wiki, all rights reserved, included here with its author's
  permission ([THIRD-PARTY.md](THIRD-PARTY.md)). A fork needs its own
  permission, or takes the file out (`app/src/knowledge/worker.ts` loads it,
  `notes.ts` uses it).
- **`app/src/knowledge/library.md`** is what every model is told in AInode's
  own voice: who made it, what its maker recommends. A fork speaks for
  itself: rewrite the sections about the maker, the ecosystem and the
  recommendations. `instruction.md` and `btx.md` name AInode too.

`app/src/knowledge/wiki.json` is CC BY-SA 4.0 (Wikipedia contributors). Keep
`wiki-attribution.json` beside it, as that licence asks.

## Keep these, if you like

- **The engine pins** in `app/scripts/stage-engine*.sh` and
  `build-engine-macos.sh`: upstream's `btx-modeld` and llama.cpp by sha256,
  and the licence texts that travel with them (`app/scripts/licenses/`).
- **The catalog,** `app/src-tauri/resources/catalog.json`. Every model in it
  is a content-addressed `btx://` link: any host that carries the same bytes
  serves it.
- **The model host** in `app/src-tauri/resources/hosts.json`. It is one small
  public BTX model host. A fork with many users should run its own and list
  it first: upstream's documentation says how to host, and every download is
  checked piece by piece whoever serves it.
- **The three lines** in [docs/DIRECTION.md](docs/DIRECTION.md), if you want
  your users to have the same promises: not a node, not a wallet, never a host
  the user cannot see.

## Stay in touch

If you fix something that matters for every user, a pull request here helps
your users and ours. The engine itself is developed upstream at
[btxchain/btx](https://github.com/btxchain/btx).
