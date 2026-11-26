**AInode 0.9.2 for Mac (Apple Silicon), Windows and Linux.** A release of fixes: nothing new to learn, and nothing changes in what leaves your computer.

SHA-256 of each download:
```
381f69df806634ba6d190a78caa34f568d39b9e24e616ab7d57ca43f969ae756  AInode-0.9.2.dmg
f7c55363e35e42176d71031e809e2ca3c77daaf2ce70ad606b3558ea6ebbcf51  AInode_0.9.2_aarch64.app.tar.gz
928f822638890a21a48fac98118233b68cd65694b5331994f95ec3e9a12319a5  AInode_0.9.2_x64-setup.exe
74c32bedc3a17de769e17d00b8ecb8fbdf8c215d9678fe3e68f96506afd20c3e  AInode_0.9.2_amd64.deb
83f3d4eec67e696f9738764ded5bac670e335b7b2b94b37c36fc9c652bc24cf1  AInode_0.9.2_amd64.AppImage
```

**Updating:** AInode 0.8.2 and newer install 0.9.2 by themselves at the first quiet moment: nothing downloading, no chat and no panel open. Your models and settings stay. The Linux `.deb` asks for your password, as any package does.

## Fixed
- **A damaged copy of a model is replaced.** AInode keeps one copy of each model. If that copy was damaged on disk, the chat could not start, said only "exportmodelpath: IO: missing piece", and **Get** did nothing. That was so since 0.8.4. Now AInode checks the copy against its published checksum, says in plain words that it is damaged, removes it, and **Get** fetches it again. If a chat is open on that model, or another download is running, it touches nothing and says what to do first.
- **Linux AppImage: opening a chat could make the window go blank** and come back as an empty start screen. The chat's opening scene played a short video, and the AppImage could not play it. That was so in 0.8.4 and 0.9.1. On Linux the scene now shows a still picture.
- **Windows: settings sometimes could not be saved**, with "Access is denied", while another program (an antivirus scanner, for one) was reading the file at that moment. AInode now tries again for about a second and a half.
- **Windows: with Windows' "Text size" above 100 %** the window was too narrow for its page, and lines wrapped. The window now grows by the same factor, as far as the screen has room.
- **A long error from the engine ran past the window's edge.** It wraps now.
- **After a model failed to start**, the message box went on saying "Starting the model…". It now says that the model did not start.

## Bundled libraries
- Three libraries inside the app were updated because advisories were published for the versions 0.9.1 ships: `rustls-webpki` 0.103.17 (was 0.103.15), `url` 2.5.9 (was 2.5.8) and `cookie` 0.18.3 (was 0.18.1). None of the three allowed a forged update or running code: an update is accepted only with AInode's own signature, as before. The updater plugin is 2.14.1 (was 2.14.0).

## Good to know
- Everything under "Good to know" in the 0.9.1 notes still holds. The engine is the same: upstream BTX `btx-modeld` v0.34.16, on the Mac built by AInode from upstream's source, on Windows and Linux upstream's own file.
- **Linux** needs glibc 2.39 and the C++ library of GCC 14, or newer (Ubuntu 24.04, Debian 13, Fedora 40, or newer). That was already so for 0.9.1; the documentation now says it in one place.
- AInode only downloads. It never serves, pays or earns, and has no wallet.

Feedback: Send feedback in the app, open an issue here, or write in the BTX chat (reach it from @btx_ecosystem).
