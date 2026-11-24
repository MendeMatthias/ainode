**AInode 0.8.1 for Mac, Windows and Linux.**

SHA-256 of each download:
```
73f8c5a33519b5b8ec137b07f9eb1ac8548d16fb5425b56156e639d0025a4a68  AInode-0.8.1.dmg
0db4b20325a610864e88accb0d646ebc8152854acf52e7891be504f2bdff348d  AInode_0.8.1_aarch64.app.tar.gz
4efceffb84e3f8207e152cf3bc97acb44fae8c49f97fb7906f1744f79fe63dd5  AInode_0.8.1_x64-setup.exe
572901559131b1cb9f3135a59f80f282037bb69af1023a3a8f5aa2a576f3ce9e  AInode_0.8.1_amd64.deb
836a4c788c3469c85e7688cc235578c82bde61f48122d93478d3b4544098cf60  AInode_0.8.1_amd64.AppImage
```

**Updating:** on the Mac, AInode 0.8.0 offers 0.8.1 by itself (**Restart** under the status line). On Windows and Linux, coming from 0.7.1: download this one by hand once (0.7.1 has no updater). From 0.8.1 on, AInode updates itself there too.

## New in 0.8.1
- **Starts with your computer.** AInode opens by itself when you sign in, minimized, and its engine starts with it: online without a click. Open AInode to see its window. Status > **Start with this computer** or **Only when I open it** turns it off. It is set up after you choose your space, never before.
- **The home line is just the numbers:** `3 AInodes · 1 host · 47 BTX nodes`. The BTX nodes are the computers keeping BTX's chain (from easybtx.com's census, easyNodes among them). AInode is counted every few minutes even with its window hidden.
- **The (i) in Network** opens the guide at easybtx.com/ai/network: what the network is, what to type, what it does not do yet.
- **Beside a BTX node on the same computer** (easyNode):
  - Status shows it: "BTX node here · At block 249,862 · easyNode on this computer".
  - Chats leave it room: at most 4 processor threads while a node runs there. Measured beside a block signer: its blocks then take about 2 s longer during an answer instead of about 5 s.
  - The Graphics card note says why Leave it free keeps the node's blocks quick.
  - Windows: when easyNode restarts WSL to recover its graphics card, AInode says so and starts again by itself, instead of showing an engine error.
- **Linux: far lighter when open.** The turning ring was redrawn 60 times a second without the graphics card and kept 2 to 4 processor cores busy. It now turns in steps, and every animation pauses while the window is hidden or in the background.
- Chats run at below-normal priority (Windows) or nice 10 (Linux): the rest of the computer comes first.

## Fixed
- **Check for updates** no longer says "failed" on Windows and Linux when a release lists only the Mac.
- A double click on Send no longer stops the answer it just started.
- AInode for Linux inside WSL opens links in the Windows browser.
- Send feedback names the processor properly on Windows.

## Good to know
- AInode only downloads. It never serves, pays or earns, and has no wallet.
- "Checked" means every 4 MiB piece is intact, not that a model is safe or correct.
- Bundled: BTX `btx-modeld` v0.34.15 (MIT, unmodified), llama.cpp b11559 (MIT), OpenSSL 3.5.9 (Apache-2.0), pdf.js 6.3.289 (Apache-2.0), Tauri (MIT or Apache-2.0), opening paragraphs of Wikipedia's Vital Articles (CC BY-SA 4.0), postquantum.wiki (all rights reserved, included with its author's permission).

Feedback: Send feedback in the app, open an issue here, or write in the BTX chat (reach it from @btx_ecosystem).
