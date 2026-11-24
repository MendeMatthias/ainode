# The real window, clicked through

`click-through.py` drives the **installed** AInode through its own window, the
way a person would, against the real engine and the public host, and saves a
screenshot of every step:

consent, Start, Store, Get the 3B (2.2 GB over BTX), Chat (the loading scene,
an answer), Clear, a PDF added with +, the settings sheet, Network, Status,
kept chats under History, Remove (the space comes back).

It talks WebDriver to `tauri-driver`, which drives the app's webview:
WebKitWebDriver on Linux. `webdriver.py` is a stdlib-only client.

## Linux (run 2026-10-13 in WSL2, Ubuntu 24.04, Xvfb)

```bash
sudo apt install xvfb webkit2gtk-driver dbus-x11
cargo install tauri-driver --locked
sudo apt install ./AInode_<version>_amd64.deb        # from scripts/release-linux.sh
Xvfb :99 -screen 0 1280x900x24 -nolisten tcp &
export DISPLAY=:99 HOME=$(mktemp -d) XDG_RUNTIME_DIR=$(mktemp -d)   # a fresh user
dbus-run-session -- sh -c 'tauri-driver --port 4444 --native-port 4445 & sleep 2;
  python3 lab/window/click-through.py /usr/bin/ainode shots/ --pdf some-text.pdf'
```

Xvfb keeps the window off the screen of whoever uses the computer.

Two things the app does for this, both only when `tauri-driver` launched it
(`TAURI_WEBVIEW_AUTOMATION=true`, Tauri's own switch): the window is not
incognito, because WebKitWebDriver can only attach to the web context Tauri
makes, not the ephemeral one wry makes for an incognito webview
(`lib.rs` `open_main_window`). Its data still goes to the private runtime
folder, never the home folder.

## Nothing written before consent

Checked separately, without WebDriver: launch the installed app with an empty
`HOME`, leave it on the consent screen, quit, list `HOME`. Since 2026-10-13 it
stays empty (before: WebKitGTK salts and an HSTS database under
`~/.local/share/com.m2promende.ainode/`, and Mesa's shader cache).
