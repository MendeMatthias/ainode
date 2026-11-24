**AInode 0.8.0 for Mac** (Apple Silicon, macOS 13 or later). Signed with a Developer ID and notarized by Apple.

SHA-256 of `AInode-0.8.0.dmg`:
```
43f92cf686604916ef43d54d6bfbdc30f0e39b46776077b8d97717c31b27a9fa
```

**Coming from 0.7.1: download this one by hand.** 0.7.1 has no updater. From 0.8.0 on, AInode updates itself.

Windows and Linux 0.8.0 follow in a few days on this page; until then their 0.7.1 files are on the [0.7.1 release](https://github.com/MendeMatthias/ainode-releases/releases/tag/v0.7.1).

## New in 0.8.0
- **Updates by themselves.** AInode looks for a newer version when it starts and every 6 hours. When one is ready it says so quietly under the status line: **Restart** or **Later**. It never interrupts a download or a chat, and an update never touches your models, chats or files. Status has **Check for updates**. Every update is signed with AInode's own key and must name its exact version and platform, or it is refused.
- **Send feedback.** In Status: **Report a problem**, **Suggest an idea**, **Ask for a model**; and a small button in the chat. Your browser opens a new issue here, filled in with AInode's version and your computer's system and memory. You read it before you send it (a free GitHub account is needed). AInode itself sends nothing; your models' names only if you tick the box.
- **Space freed right after a download.** The downloaded pieces are deleted as soon as the model's copy is checked, not at its first chat. A model takes its size once from the moment it is ready.
- **Network search finds AInode's models,** with their names, publishers, licences and descriptions, signed by the host.

## Good to know
- AInode only downloads. It never serves, pays or earns, and has no wallet.
- "Checked" means every 4 MiB piece is intact, not that a model is safe or correct.
- Bundled: BTX `btx-modeld` v0.34.15 (MIT, unmodified), llama.cpp b11559 (MIT), OpenSSL 3.5.9 (Apache-2.0), pdf.js 6.3.289 (Apache-2.0), Tauri (MIT or Apache-2.0), opening paragraphs of Wikipedia's Vital Articles (CC BY-SA 4.0), postquantum.wiki (all rights reserved, included with its author's permission).

Feedback: Send feedback in the app, open an issue here, or write in the BTX chat (reach it from @btx_ecosystem).
