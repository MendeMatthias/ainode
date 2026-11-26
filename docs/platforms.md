# Platforms: Mac, Linux and Windows

The order was Mac, then Windows and Linux. All three are built. Windows
is not a plain build: upstream's model helper has never run there, so the
Windows app runs upstream's Linux helper inside WSL2 (`docs/windows.md`).

## macOS (done)

Apple Silicon, macOS 13+. The app bundles `btx-modeld` and llama.cpp's
`llama-server`. Up to 0.8.4 the helper was upstream's own v0.34.15 file, byte
for byte, with an OpenSSL 3.5.9 CLI beside it (E1). From the v0.34.16 engine
on, upstream publishes no Mac file: the helper is built by
`app/scripts/build-engine-macos.sh` from upstream's tag and staged only with
its pinned sha256, and no OpenSSL tool is bundled. Signed and notarized
since 0.5.0 (`app/scripts/release-macos.sh`, `docs/SIGNING.md`): every bundled
Mach-O is Developer ID signed with the hardened runtime, which changes
`btx-modeld`'s signature bytes, so the installed helper does not have the
pinned sha256; `check-bundle.sh` runs before signing and compares the staged
bytes. The release script has run with the new engine since 0.9.0.

## Linux (built 2026-10-13)

x86_64, glibc 2.39 or newer and, from the v0.34.16 engine on, a libstdc++
from GCC 14 or newer: Ubuntu 24.04+ and Debian 13+ have both (run on the
Windows PC on 2026-11-24: the 0.9.1 `.deb` on Debian 13, the AppImage on
Ubuntu 24.04; and on 2026-11-26: the 0.9.2 `.deb` on Debian 13, the AppImage
on Ubuntu 24.04 and Debian 13), Fedora 40+ should (glibc 2.39, GCC 14; not run). The app
itself is built on Ubuntu 24.04 and needs GLIBC_2.39, and so did 0.8.4's:
on Fedora 39 (glibc 2.38, GCC 13) both AppImages stop at once with
"GLIBC_2.39 not found", before any window (measured). So no system with
glibc 2.38 ever ran AInode for Linux, and none can update itself into the
new engine's rule. The engine alone needs glibc 2.38 and CXXABI_1.3.15. The
only systems where the app starts and the engine cannot are those with glibc
2.39 or newer and a libstdc++ older than GCC 14; there the app says so in
plain words (shown on the PC in the real window: Fedora 39's GCC 13 libstdc++
given to the engine on Debian 13).
`app/scripts/release-linux.sh` makes a `.deb` and an AppImage from the same
code as the Mac app.

- **Engine:** upstream's `btx-0.34.16-x86_64-linux-gnu.tar.gz` (pinned
  `9bbfff06…932b`): `libexec/btx-modeld.real`, bytes unmodified, plus the
  six libraries it needs from the release's `lib/` (OpenSSL, SQLite, libgomp,
  zlib, zstd). The app starts it with `LD_LIBRARY_PATH=<engine>/lib`, exactly
  as upstream's own `bin/btx-modeld` wrapper does (`engine.rs` `helper_env`).
  New with v0.34.16: upstream no longer ships libstdc++ and libgcc_s, so the
  helper takes them from the computer and needs GLIBCXX_3.4.32 and
  CXXABI_1.3.15 (read from the archive on 2026-11-23). The `.deb` depends on
  `libstdc++6 (>= 14)` and `libgcc-s1 (>= 14)`. Until 0.8.4 the engine was
  v0.34.15 with seven libraries, libstdc++ among them.
- **No OpenSSL CLI any more.** v0.34.15 needed one on first start (E1) and
  AInode built it static. v0.34.16 mints its identity in-process;
  `stage-engine-linux.sh` proves the staged helper reaches "PQ1 ready" with
  no OpenSSL tool and opens no listening socket.
- **Chat:** llama.cpp `b11559`, Ubuntu Vulkan x64 build: the CPU backends (one
  per instruction set, picked at run time) plus a Vulkan backend loaded only
  if the computer has a Vulkan driver; without one it chats on the CPU. Its
  libraries carry `RUNPATH $ORIGIN`. It needs the system's OpenSSL 3 (only
  3.0.0 symbols, checked) and libstdc++; OpenMP (`libgomp`) is bundled next to
  it from the BTX release, since a minimal install may lack it.
- **Paths:** `$XDG_DATA_HOME/AInode` (`~/.local/share/AInode`) for data; the
  socket, pid files, log and the webview's own files in
  `$XDG_RUNTIME_DIR/ainode/` (0700, a per-user tmpfs; without it
  `/tmp/ainode-<uid>/`). Show file uses the file manager's
  `org.freedesktop.FileManager1.ShowItems` over D-Bus, else `xdg-open` on
  the folder; links use `xdg-open`. Inside an AppImage those children get the
  user's environment without the AppImage's own library paths (`os.rs`).
- **Memory:** the kernel's online memory blocks, rounded up to whole GiB, so
  a 16 GB PC counts as 16 like a 16 GB Mac (MemTotal alone says 15.5).
- **A crashed app leaves nothing running** (2026-10-14): the helper and
  llama-server carry `PR_SET_PDEATHSIG`, so a SIGKILLed app ends them with
  SIGTERM instead of leaving llama-server holding gigabytes until the next
  launch; the next launch's cleanup stays as the second line.
- **Other distributions** (2026-10-14, `lab/evidence/2026-10-14/distros/`):
  on a fresh **Debian 13** the `.deb` installs with its own dependencies
  (`libssl3t64 | libssl3, libwebkit2gtk-4.1-0, libgtk-3-0`), the engine
  fetches the 350M from the public host byte-identical, llama-server
  answers, the app runs under WSLg. Found there: fontconfig wrote its
  per-user cache into `~/.cache` at the first launch when the system's cache
  is missing or stale (Ubuntu's was not); the app now sets `XDG_CACHE_HOME`
  to its private runtime folder before GTK starts (`lib.rs`), and a fresh
  HOME stays empty. On **Fedora 44** the AppImage runs the same flow; from
  the host it needs what every desktop Fedora has (fontconfig, freetype,
  fribidi, harfbuzz, libX11, libxcb, libdrm, libglvnd EGL/GLX/GLES,
  libwayland-client, Mesa's gbm and DRI drivers), found by `ldd` on a minimal
  container image.
- **Rule 1 holds:** a fresh launch writes nothing to the home folder before
  consent. WebKitGTK wrote salts and an HSTS database there even for an
  incognito window, and Mesa its shader cache: the window's data directory
  is now the runtime folder and the drivers' disk caches are off
  (`lib.rs`).
- **Packaging:** Tauri builds both; `release-linux.sh` then puts upstream's
  engine back into the AppImage (linuxdeploy had patched a RUNPATH into
  `btx-modeld`, its OpenSSL and our CLI), names the Debian package `ainode`
  (Tauri says `a-inode`), and proves every engine file in both outputs equals
  `MANIFEST.json`.
- **Proven:** `tests/engine_live.rs` on Linux against the public host (the
  350M byte-identical, kept once, chat, Remove frees the space, a re-fetch,
  crash and orphan recovery), and the installed `.deb` clicked through in its
  real window (`lab/window/`). Evidence: `lab/evidence/2026-10-13/linux/`.
- **Not yet:** a real desktop GPU through Vulkan (tested on the CPU, inside
  WSL2), ARM. A distribution with glibc below 2.39 does not run the app at
  all (Fedora 39, measured). Unsigned, like most Linux downloads.

## Windows (built 2026-10-14; `docs/windows.md`)

x64, Windows 11 (Windows 10 untested), WSL2. Upstream's model helper is POSIX
code with no Windows build (22 files in `src/modelnet/` with POSIX headers, a
unix-socket control surface, `fork`/`posix_spawn`; easyNode's Windows `btxd`
is built with `-DWITH_MODELNET=OFF` for this reason). So the Windows app runs
upstream's **unmodified Linux helper inside AInode's own WSL2 distribution**
(route 1 of `docs/windows.md`), and chats natively.

- **Engine:** the Linux engine above, byte for byte, inside a distribution
  named `AInode` that the app imports from a shipped, pinned Ubuntu Base
  tarball (30 MB) on the first Start after consent (1.1 s). `ainode-wsl`
  (`app/wsl-host`, static musl, standard library only) starts the helper in
  there and relays the app's JSON-RPC over one `wsl.exe` pipe (0.2 ms a
  call); it clears a crashed run's leftovers before starting (pid file and
  socket in `/tmp/ainode`), as the Linux app does at launch.
- **Chat:** llama.cpp's Windows Vulkan build (`llama-server.exe`, CPU
  backends picked at run time, Vulkan when a driver is there), with the
  Visual C++ runtime app-local. The model never crosses into WSL.
- **Paths:** program `%LOCALAPPDATA%\AInode` (per-user NSIS installer, no
  admin prompt), data `%LOCALAPPDATA%\AInode\data` (the models on the
  Windows drive, `/mnt/c/...` for the helper, so Explorer and the app agree
  on the space), the chat's pid and key and the webview's files under
  `%TEMP%inode`. Show file is Explorer `/select,`; links open the default
  browser. Disk from `GetDiskFreeSpaceExW`, memory from
  `GlobalMemoryStatusEx` rounded up to whole GiB.
- **No WSL:** the app checks the installed files first (the inbox `wsl.exe`
  is a stub that never says "not installed") and shows one plain sentence
  with `wsl --install --no-distribution`.
- **Packaging:** `app/scripts/release-windows.ps1` stages the engine inside a
  WSL build distribution (`stage-engine-windows.sh`), builds the NSIS
  installer with the Windows toolchain, and proves every engine file equals
  `MANIFEST.json`. Uninstall keeps the data and removes the distribution.
- **Proven:** the real window clicked through from a first launch (12 steps,
  `lab/evidence/2026-10-13/windows-app/`), the installer installed, run and
  uninstalled, and `tests/engine_live.rs` on Windows against the public host
  (`lab/evidence/2026-10-14/windows-live/`). Rule 1 holds: nothing outside
  `%TEMP%inode` before consent.
- **Not yet:** Vulkan on a Windows GPU (tested on the CPU: this PC's GPU is
  busy), a real PC without WSL, Windows 10, signing (unsigned: SmartScreen
  warns once; `docs/windows.md` has the proposal), the updater feed entry.
