**AInode 0.8.4 for Mac, Windows and Linux.**

SHA-256 of each download: added when the files are published.

**Updating:** AInode 0.8.2 and 0.8.3 install 0.8.4 by themselves at the first quiet moment. AInode 0.8.0 and 0.8.1 offer it with **Restart** under the status line. The Linux `.deb` asks for your password, as any package does.

## Keeps going by itself
- **Windows: when Windows' Linux gets stuck, AInode gets it going again.** After a graphics driver reset or a Windows update, Windows' Linux support (WSL) can lose the PC's drives until it is restarted. When AInode's own Linux is the only one running, AInode now restarts WSL by itself and starts again, no click needed. When other Linux runs too (a BTX node, your own Linux), AInode never restarts it on its own: it shows one button that names everything that would stop, and acts only when you press it.
- **Windows: AInode waits while easyNode restarts WSL.** On a PC that also runs easyNode, AInode now sees when easyNode is restarting WSL to get its graphics card back, waits for it, and starts again by itself afterwards.
- **Counted and healed even when nobody looks.** AInode's check-in with easybtx.com and the restart of its engine after a crash now run in the app itself, not in its window. Before, AInode for Linux could stop checking in while the computer sat idle with the screen off.

## Safer
- **Windows: your own files in AInode's folder are never deleted.** AInode's repair of its folder's permissions (new in 0.8.3) now reads very long paths correctly and keeps any file it cannot fix. The engine then does not start, and AInode names the file.
- **Windows: a stuck WSL is not mistaken for a broken install.** AInode no longer sets up its Linux again when the real problem is WSL itself. It says so in plain words instead of "Input/output error".

## Good to know
- AInode only downloads. It never serves, pays or earns, and has no wallet.
- "Checked" means every 4 MiB piece is intact, not that a model is safe or correct.
- Report a security problem privately through "Report a vulnerability" on the repository, not in a public issue.
- Bundled: BTX `btx-modeld` v0.34.15 (MIT, unmodified), llama.cpp b11559 (MIT), OpenSSL 3.5.9 (Apache-2.0), pdf.js 6.3.289 (Apache-2.0), Tauri (MIT or Apache-2.0), Ubuntu Base 24.04.5 on Windows (Canonical, its packages' own licences), opening paragraphs of Wikipedia's Vital Articles (CC BY-SA 4.0), postquantum.wiki (all rights reserved, included with its author's permission).

Feedback: Send feedback in the app, open an issue here, or write in the BTX chat (reach it from @btx_ecosystem).
