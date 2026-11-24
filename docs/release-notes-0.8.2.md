**AInode 0.8.2 for Mac, Windows and Linux.**

SHA-256 of each download:
```
da72c2a37131cca1c2754a06e6fb4dbc0a9edca4033e664d568cb01293244827  AInode-0.8.2.dmg
e45d8b7c35a5e03a3a89291547f865f8f85c6a9d5aab8a9288275a373b7d49c0  AInode_0.8.2_aarch64.app.tar.gz
2ca98af9a2d4742825bde306cb3d1f257b5bb30bea3f7a851702f6b7e7db8250  AInode_0.8.2_x64-setup.exe
deddeb9f1a69ad181f2993191d178b7169d393d341670103a6a022bcc3d471b7  AInode_0.8.2_amd64.deb
bc2579f72f1a4af370e334bc491da9ab6bb3bb371b0e9b6cddde13501be1dc1d  AInode_0.8.2_amd64.AppImage
```

**Updating:** AInode 0.8.0 and 0.8.1 offer 0.8.2 by themselves (**Restart** under the status line). From 0.8.2 on, AInode installs its updates by itself. Coming from 0.7.1 on Windows or Linux: download this one by hand once (0.7.1 has no updater).

## New in 0.8.2
- **Updates install by themselves.** AInode looks for a new version every hour (it was every 6 hours) and installs it at the first quiet moment: no chat open, no model downloading, no panel open. It closes its engine the way Quit does, opens again the way it was (minimized stays minimized), and then says "Updated to AInode 0.8.2." Your models, chats and settings stay as they are. Every check it already made stays: the file must be signed for this version, and AInode never goes back to an older one.
- **The Linux `.deb` still asks first,** because installing a package needs your password. There the **Restart** bar stays, framed in green.
- **When an update could not install by itself** (no connection, say), the **Restart** bar comes back and calls for attention: a green light runs around its frame. It pauses while AInode is hidden or minimized; with "reduce motion" on, and on Linux, it is a steady green frame.
- **The BTX nodes on the home line open easybtx.com/nodes,** the list of the computers keeping BTX's chain.
- **Mac: a click on AInode's window in the background lands** on what you clicked, instead of only bringing the window to the front.
- **Mac: AInode keeps working while it is minimized and the Mac sits idle.** Before, after a while it stopped checking in, healing its engine and looking for updates until someone used the Mac again. It now keeps doing them, once a minute, with its animations still paused.

## Fixed
- **Windows: nothing animates while AInode's window is minimized,** also when it starts minimized with the computer. The turning ring kept a little of the processor busy there.

## Good to know
- AInode only downloads. It never serves, pays or earns, and has no wallet.
- "Checked" means every 4 MiB piece is intact, not that a model is safe or correct.
- Bundled: BTX `btx-modeld` v0.34.15 (MIT, unmodified), llama.cpp b11559 (MIT), OpenSSL 3.5.9 (Apache-2.0), pdf.js 6.3.289 (Apache-2.0), Tauri (MIT or Apache-2.0), opening paragraphs of Wikipedia's Vital Articles (CC BY-SA 4.0), postquantum.wiki (all rights reserved, included with its author's permission).

Feedback: Send feedback in the app, open an issue here, or write in the BTX chat (reach it from @btx_ecosystem).
