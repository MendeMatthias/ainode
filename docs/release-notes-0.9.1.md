**AInode 0.9.1 for Mac (Apple Silicon), Windows and Linux.** The Windows and Linux files were added on 2026-11-25, built from the same source and tested on a Windows PC. The Mac files are unchanged.

SHA-256 of each download:
```
f5cf194f7b3c4056733a824873d93c7a58b88fdb9f62c908834d39185570fbbb  AInode-0.9.1.dmg
63fb1f4251790642587b50ae2d4fca7c687340b92c2110db406cb01bcee45cd8  AInode_0.9.1_aarch64.app.tar.gz
bb42d1ffbeacf61336b452db03ba441cd35ce13f5ecd3c98b65c01dd1adde90f  AInode_0.9.1_x64-setup.exe
1c17b67a62b595eba2059fd59e827198bc8398293855bab3186eadf166ca9dc2  AInode_0.9.1_amd64.deb
e8aa233d37aeca5c661d8b7d25d65ce69f5703cfc6c0e578a47f59cf4d8155b9  AInode_0.9.1_amd64.AppImage
```

**Updating:** AInode 0.8.2 and newer install 0.9.1 by themselves at the first quiet moment: nothing downloading, no chat and no panel open. AInode 0.8.0 and 0.8.1 offer it with **Restart** under the status line. Your models and settings stay. The Linux `.deb` asks for your password, as any package does.

## New
- **The chip on the home screen counts the hosts by itself.** "Powered by BTX ModelNet · 1 host answering" now appears after the engine has started, without opening the BTX ModelNet view first. For that the engine asks the network once per engine start which models are there, with no search words, and only while AInode's window is shown. An AInode that started with the computer and stays minimized asks nothing until you open its window. The number is the hosts that answered this AInode, not a count of the whole network.
- **You can choose more hosts to ask when you search.** The BTX ModelNet view has a new list, "Hosts this AInode asks when it searches". It starts with your model host only. You can add up to 4 hosts and remove them again; the list is kept in your settings and given to the engine after every start. A host you add is asked when a search runs, never in between. AInode still fetches models only from its model host, not from the hosts you add. Some addresses are refused, with the reason, because the engine would never ask them: a host on this computer, a few special address ranges, and the ports of a BTX node. Hosts on your own network are allowed. Your model host cannot be added: every search goes to it anyway.
- **The chat knows BTX as of v0.34.16.** The BTX texts bundled for the chat (upstream's README, release notes and model-network docs) are now those of v0.34.16; in 0.9.0 they were still v0.34.15's. Nothing is looked up online, as before.
- **BTX ModelNet is named where you meet it.** The first-run screen, the home screen before you press start ("Press start to connect to BTX ModelNet") and Status, whose engine row is now labelled "BTX ModelNet".

## Fixed
- **"What are the latest BTX updates?" got almost no answer.** The list of releases reached the model as its first sentence only, so it could say little more than which release is shipping. That was so in 0.9.0 and before. The list is now one sentence per release, newest first.
- **The BTX ModelNet view with no network** now says in plain words that the search ran out of time and to press Refresh, instead of the engine's own message.

## What this adds to what leaves your computer
- **One search per engine start**, to your model host, once the window is shown: one short connection, about 20 to 40 KB. No words are sent in it.
- **Searches also go to every host you add.** Each of them then sees your computer's IP address, the engine's identity (a public key, not a name) and the words you search for, each time a search runs: at the engine's start, when the view opens or refreshes, and when you search. Add only hosts you are willing to show that to. With an empty list a search goes to your model host only. One exception, by the engine's own rule: if the model host named other sources during a download in the last 15 minutes (0.9.0's notes, "Who else can see your AInode's address"), a search asks those too. The model host has named none so far.
- Nothing else leaves your computer that did not in 0.9.0. The engine is the same as in 0.9.0 (upstream BTX v0.34.16, built by AInode for the Mac), with the same six settings.

## Good to know
- Everything under "Good to know" in the 0.9.0 notes still holds. In short: while AInode is open its engine contacts the model host every 5 seconds, about 350 MB over a full day, and records nothing from it (reported upstream, btxchain/btx#946); during a download, and for up to 15 minutes after, the engine may contact other hosts the model host names; upstream publishes no Mac file for this engine, so AInode builds it from upstream's source.
- **Windows and Linux come from 0.8.4** and skip 0.9.0, so what the 0.9.0 notes list as new is new for them here too: the BTX model engine v0.34.16, on these two systems upstream's own file, unmodified. AInode for Linux now uses the system's C++ library and needs the one that comes with GCC 14 or newer (Ubuntu 24.04, Debian 13, Fedora 40, or newer). On an older system the `.deb` does not install, and AInode 0.8.4 keeps running there.
- AInode only downloads. It never serves, pays or earns, and has no wallet.
- "Checked" means every 4 MiB piece is intact, not that a model is safe or correct.
- Report a security problem privately through "Report a vulnerability" on the repository, not in a public issue.
- Bundled in the Mac app, as in 0.9.0: BTX `btx-modeld` v0.34.16 (MIT, built from upstream's source) with OpenSSL 3.5.9 (Apache-2.0), SQLite 3.46.1 (public domain), secp256k1 (MIT), LevelDB (BSD-3-Clause) and libbitcoinpqc (MIT) inside it; llama.cpp b11559 (MIT); pdf.js 6.3.289 (Apache-2.0); Tauri (MIT or Apache-2.0); opening paragraphs of Wikipedia's core articles (CC BY-SA 4.0). The licence texts are in the app, beside the engine.

## Not in this release
- Which host answered a search: the engine does not say, and the view says so.
- Telling the engine which chain it is on (`-pwcchain=main`): left out, as in 0.9.0.
- Sharing models, or paying. Unchanged.

Feedback: Send feedback in the app, open an issue here, or write in the BTX chat (reach it from @btx_ecosystem).
