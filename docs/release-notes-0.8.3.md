**AInode 0.8.3 for Mac, Windows and Linux.**

SHA-256 of each download: added when the files are published.

**Updating:** AInode 0.8.2 installs 0.8.3 by itself at the first quiet moment. AInode 0.8.0 and 0.8.1 offer it with **Restart** under the status line. The Linux `.deb` asks for your password, as any package does.

## Safer
Two security reviews went through AInode. They found no open port: AInode never listens on the network, and its chat server answers only on this computer and only with its key. What they fixed:

- **Your AInode folder is yours alone.** On Windows, a folder on a second drive (such as `D:\AInode`) was open to every other account on the computer. It is now private to you, and AInode checks it at every start. If another account owned the folder or left files in it, AInode takes it back. The engine does not start until it has. On Mac and Linux, AInode's files are readable by you only, the ones an older version wrote too.
- **Windows: the engine's Linux sees two folders.** AInode's engine runs in its own small Linux on Windows (WSL). That Linux used to see all your drives. It now sees only the engine's own files (read only) and the models folder. It cannot start Windows programs or reach your screen, clipboard or microphone. With this update AInode sets that Linux up again once from a fresh Ubuntu base. Your models stay where they are.
- **Fewer doors in the window.** The app's window may use only the four built-in commands it needs. Links leave the app only as plain `https://` addresses, never with a user name in them.
- **Leaner Windows updates.** The Windows updater no longer contains the code for unpacking zip files, which it never needed.
- **Windows signing is ready.** The Windows files will carry a code signature as soon as AInode has a certificate. These files are not signed yet, so Windows may warn before the first start.

## Also
- **Linux release builds try again** when a packaging tool fails for a moment, instead of stopping.

## Good to know
- AInode only downloads. It never serves, pays or earns, and has no wallet.
- "Checked" means every 4 MiB piece is intact, not that a model is safe or correct.
- Report a security problem privately through "Report a vulnerability" on the repository, not in a public issue.
- Bundled: BTX `btx-modeld` v0.34.15 (MIT, unmodified), llama.cpp b11559 (MIT), OpenSSL 3.5.9 (Apache-2.0), pdf.js 6.3.289 (Apache-2.0), Tauri (MIT or Apache-2.0), Ubuntu Base 24.04.5 on Windows (Canonical, its packages' own licences), opening paragraphs of Wikipedia's Vital Articles (CC BY-SA 4.0), postquantum.wiki (all rights reserved, included with its author's permission).

Feedback: Send feedback in the app, open an issue here, or write in the BTX chat (reach it from @btx_ecosystem).
