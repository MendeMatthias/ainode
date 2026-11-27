**AInode 0.9.3 for Mac (Apple Silicon), Windows and Linux.** Three fixes. Nothing new to learn.

SHA-256 of each download:
```
cff1cae91cda83b39d542d5d2fcd6223716dd1d8e6c909e5a6c3621e62ae4a42  AInode-0.9.3.dmg
4349977ffe6591a4511fbe1a67a8d284793dd98c92f7d09e9b0ae6b823569d37  AInode_0.9.3_aarch64.app.tar.gz
6e7bc7f49430ef2b671dad65aeecc846d9eccfdae7d2388409a7e7400cc693c3  AInode_0.9.3_x64-setup.exe
b90f4f920f87241b8c9f0146170ffa3053794476b11965ccd88ab7d208e7b9b5  AInode_0.9.3_amd64.deb
0924cbf7d779482eace6cda8da9c5e489e16e406250ac8da6a6c241fd8690b3e  AInode_0.9.3_amd64.AppImage
```

**Updating:** AInode 0.8.2 and newer install 0.9.3 by themselves at the first quiet moment: nothing downloading, no chat and no panel open. Your models and settings stay. The Linux `.deb` asks for your password, as any package does.

## Fixed
- **Quitting AInode a moment after it started could leave it running with no window.** It then would not open again until it was force-quit or the computer restarted, because the copy still running answered for it. Seen on the Mac with 0.9.2. AInode now always ends: it stops its engine as before and waits for that at most 20 seconds.
- **The chip could say "no host answered" for ten minutes although the host was fine.** AInode asks the network one question when its engine starts, for the count on the chip. A host refuses an internet address for up to a minute after a few broken connection attempts from it, and everyone behind one shared address shares that count. A start that fell into that minute kept the amber chip. Now, when no host answered, AInode asks once more about 70 seconds later, and never a third time.
- **Linux inside WSL: after an update, AInode did not come back by itself.** Started from a Windows shortcut with `wslg.exe`, the `.deb` update was installed and AInode stayed closed until it was started by hand. On Linux the restarted AInode now runs in a session of its own. A normal Linux desktop was not affected.

## What leaves your computer
- One thing more than in 0.9.2, and only in one case: when no host answered the question AInode asks at its engine's start, it asks the same question once more about 70 seconds later. It is the same search with no words, to the same hosts. Everything else is as in 0.9.2.

## Good to know
- Everything under "Good to know" in the 0.9.2 and 0.9.1 notes still holds. The engine is the same: upstream BTX `btx-modeld` v0.34.16, on the Mac built by AInode from upstream's source, on Windows and Linux upstream's own file.
- **Linux**, after a restart of AInode by itself (an update): what AInode writes to a terminal or a pipe is no longer shown there, because the restarted AInode no longer belongs to that terminal. A log file or the system journal still gets it.
- AInode only downloads. It never serves, pays or earns, and has no wallet.

Feedback: Send feedback in the app, open an issue here, or write in the BTX chat (reach it from @btx_ecosystem).
