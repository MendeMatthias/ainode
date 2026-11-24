# AInode on Windows: findings and the route

**In short.** AInode can run on Windows today without waiting for
BTX's developers: the Windows app runs upstream's own Linux engine inside a
small private Linux it brings along (WSL2, built into Windows 10 and 11).
Measured on this PC: it costs nothing a user would notice, the engine fetched
the 12 GB model byte-identical, and models load in about 2 s when they sit on
the Windows drive. Two choices were open: **this route** (recommended), and
**where models live** (recommended: on the Windows drive, a one-time wait
per model instead of a slower start on every chat).

**Decided 2026-10-14:** route 1, and models on the Windows drive.

Measured 2026-10-13 on a Windows PC (Windows 11 Pro 26200, WSL 2.7.10,
i9-12900K, 32 GB, NVMe C:, HDD D:). Everything below was **run** on this PC
unless it says otherwise. Raw output: `lab/evidence/2026-10-13/windows/`.

## The blocker, unchanged

Upstream's model helper is POSIX-only (`docs/platforms.md`): unix-socket
control surface, `fork`/`posix_spawn`, 22 files with POSIX headers. BTX
publishes no Windows build of it, and easyNode's Windows `btxd` is built with
`-DWITH_MODELNET=OFF` for that reason. So the question is not "build it for
Windows" but "where does upstream's Linux helper run, and how does a Windows
app reach it".

## Route 1, measured: the Windows app, the engine inside WSL2

```
AInode.exe (Tauri, WebView2)  ──  llama-server.exe (llama.cpp Windows build, 127.0.0.1, key as on the Mac)
   │ one pipe, JSON lines
   └─ wsl.exe -d AInode -- <engine>/ainode-wsl          (one process for the engine's whole life)
         ├─ btx-modeld   upstream's Linux binary, unmodified, LD_LIBRARY_PATH=<engine>/lib
         └─ relays each request line to the helper's unix socket, one connection per request
```

### What was measured

| Question | Result |
|---|---|
| Reaching the helper's unix socket from Windows, a `wsl.exe` per call | median **92 ms**, p90 111 ms (20 calls) |
| The same through **one persistent relay** (`wsl.exe` kept open, JSON lines over stdin/stdout) | 79 ms once to start, then median **0.2 ms**, p90 0.6 ms (400 calls: hello, getmodelnetworkinfo, listmodels) |
| AInode's own Linux, imported from Canonical's **Ubuntu Base 24.04.5** (30 MB tarball, sha256 from cdimage.ubuntu.com) with `wsl --import` | **1.1 s**, 172 MB on disk, glibc 2.39; first command 5.9 s cold |
| Upstream's Linux helper run **from the Windows install folder** through `/mnt/c` inside that stock Ubuntu Base | starts, mints its ML-DSA-44 cert, "PQ1 ready"; nothing installed with apt |
| Fetch the 350M from the public host, store on ext4 inside WSL | 55.6 s (4.3 MB/s, the line), **sha256 matches** |
| The same, store on NTFS (`/mnt/c/...`) | 63.0 s (3.8 MB/s), **sha256 matches** |
| `exportmodelpath` (the runnable copy), 237 MB | ext4 **0.52 s**; NTFS **5.25 s**; ext4 store with the copy on NTFS 6.86 s |
| Raw writes inside WSL, 2 GiB then fsync | ext4 960 MB/s; NTFS via `/mnt/c` 190 MB/s |
| 4 MiB pieces, fsync each (how a download writes) | ext4 278 MB/s; NTFS 231 MB/s (both far above any line) |
| Windows reading a 2 GiB file inside WSL (`\\wsl.localhost\...`) | **~380 MiB/s**; from NTFS 2.5 GiB/s (partly cached) |
| Windows `llama-server.exe` (b11559, CPU build) loading the 350M | from `\\wsl.localhost`: ready in **1.6 to 1.9 s**; from NTFS: **0.6 s**; it answers either way (mmap works over the WSL share) |
| Fetch the 3B (2.24 GB) from the public host into ext4 | 393 s (5.7 MB/s, this PC's line to UAE North) |
| `exportmodelpath`, the 3B | ext4 **7.6 s** (296 MB/s); store on NTFS **26.3 s** (85 MB/s) |
| Windows `llama-server.exe` (CPU build) loading the 3B, start to ready | from `\\wsl.localhost`: **14.2 s** cold, 11.2 s again; from NTFS: **2.1 to 2.3 s** |
| **A full 12 GB fetch** (gpt-oss 20B) from the public host into WSL | 12,109,577,982 bytes in 2,335 s (5.2 MB/s, 39 min), sha256 `27cd6c43…5901`: **identical** to the catalog; `exportmodelpath` on ext4 50.1 s (242 MB/s) |
| AInode killed (TerminateProcess of its `wsl.exe`) | the Linux side gets **SIGHUP** and `btx-modeld` **exits**: a crashed app leaves no engine running inside WSL |

### What that means

- **The bridge costs nothing** once it is one long-lived process: 0.2 ms a
  call, the same as a local socket. A `wsl.exe` per call (92 ms, three calls
  a poll) would not do. No TCP port is opened; the helper still never
  listens on the network (rule 3).
- **AInode can bring its own Linux in a second, offline**: a 30 MB tarball
  imported as a private distro named `AInode` under
  `%LOCALAPPDATA%\AInode\data\wsl`. It never touches the user's own distros, needs
  no Microsoft Store, no account and no apt, and is deleted with the app
  (`wsl --unregister AInode`). Only after consent (rule 1): the import is the
  first write.
- **Upstream's helper runs as shipped**, from the install folder. The app
  keeps its whole engine contract; only the transport to the socket changes.
- **The engine dies with the app.** WSL hangs the helper up when its
  `wsl.exe` goes, so the orphan logic the Mac needs is simpler here.
- **Where the models live** is the one real choice:
  - **A. Inside AInode's WSL disk (ext4).** Fastest to fetch and prepare
    (the 3B's runnable copy in 7.6 s). Every chat start then loads the model
    across the WSL share: the 3B took 11 to 14 s to be ready (2 s from NTFS),
    so the 20B would take about a minute (extrapolated), every time.
    The disk image grows with the store; WSL's sparse mode gives the space
    back after Remove (`wsl --manage AInode --set-sparse true`).
  - **B. On the Windows drive (NTFS), `%LOCALAPPDATA%\AInode\data\modelnet`.**
    Plain Windows files: the space AInode shows is the space Explorer shows,
    Remove frees it at once, Show file opens Explorer, and every chat starts
    fast (native reads: the 3B ready in 2.2 s). The cost is once per model:
    preparing the runnable copy through `/mnt/c` took 26 s for the 3B (about
    2.5 min for the 20B), after which the pieces are deleted as on the Mac.
  - **Recommendation: B.** A one-time wait per model is easier to explain
    than a slow start on every chat, and it keeps the "space used" honest
    without a virtual disk in between.

### What the app has to do (route 1)

1. **WSL present?** `wsl.exe --status`. If not: one plain screen, "AInode
   needs Windows' built-in Linux support (WSL). Press Install: Windows asks
   for permission, then restart." It runs `wsl --install --no-distribution`
   elevated. Virtualization off in the BIOS (`0x80370102`) gets its own
   sentence. (Not tested: WSL is installed on this PC.)
2. **First Start after consent:** import Ubuntu Base as `AInode` (pinned
   sha256, shipped in the installer), add a user `ainode`, write its
   `/etc/wsl.conf` (that user, interop off, no Windows PATH, no drive
   mounted whole, no WSLg, no GPU libraries) and an `/etc/fstab` that mounts only the engine folder
   (read-only) and the models folder (never executable), set it sparse.
   Checked before every start; a distribution made by 0.8.2 or earlier is
   switched over once (two restarts of `AInode`, a few seconds).
3. **Engine:** start `wsl.exe -d AInode -- <engine>/ainode-wsl ...`, a small
   Linux program shipped with the engine (`app/wsl-host`, standard library
   Rust, static, 496 KB): it starts `btx-modeld` with the same argv and
   environment as on Linux, relays JSON lines to its socket, and stops it over
   RPC when the pipe closes. Before it starts the helper it clears what a
   crashed run left in `/tmp/ainode` the way engine.rs does on Linux: a helper
   still answering is asked to stop, one alive but silent is found through a
   pid file and ended only when that pid runs our btx-modeld, a dead socket is
   unlinked. Built and measured: ready in 0.24 s, 0.23 ms a
   call, a clean stop 0.5 s after the pipe closes, paths with spaces fine.
   `rpc.rs` gets a second transport; the gate, timeouts and polling stay.
4. **Paths** map one way: `C:\Users\u\...` is `/mnt/c/Users/u/...` for the
   helper. **Disk** from `GetDiskFreeSpaceExW`, **memory** from
   `GlobalMemoryStatusEx`. **Show file**: `explorer /select,`. **Links**:
   the default browser. **Chat**: `llama-server.exe` with
   `CREATE_NO_WINDOW`, stopped with TerminateProcess after its HTTP stop.
5. **Quit:** RPC stop, then `wsl --terminate AInode` (only ours), so the VM
   memory goes back.
6. **Chat runtime:** llama.cpp's Windows Vulkan build (b11559, 32 MB): NVIDIA,
   AMD and Intel GPUs through their own Vulkan drivers, the CPU otherwise. The
   model never crosses into WSL; only the engine does.

**Built and tested (#44):** `app/scripts/release-windows.ps1` stages the engine inside a WSL
build distribution (`stage-engine-windows.sh`: upstream's helper as on Linux,
`ainode-wsl`, llama.cpp b11559's Windows Vulkan build with the Visual C++
runtime app-local, the pinned Ubuntu Base rootfs), builds the NSIS installer
(78.6 MiB, per-user, no admin prompt) and proves the packed engine equals its
manifest. The program goes to `%LOCALAPPDATA%\AInode`, the data to its own
`%LOCALAPPDATA%\AInode\data`, the window's WebView2 files to
`%TEMP%inode\webview`.

- 54 Rust tests and clippy with MSVC; 137 UI tests on Windows.
- Nothing written before consent outside `%TEMP%inode`.
- **The real window, clicked through from a first launch** (WebDriver,
  `lab/evidence/2026-10-13/windows-app/`): consent ("this PC"), Start (AInode's
  Linux imported, the engine up, "Online · 1 host" in 2.9 s), the 3B over BTX,
  chat answers, Clear, a PDF answered from, settings, Network, Status, kept
  chats, Remove (2.2 GB back). PASS in 949 s, on the CPU (this PC's GPU runs
  the BTX node).
- The installer: installs in 11 s, Start menu entry, the installed engine
  identical to the manifest, the app runs; uninstall removes the program,
  keeps the data and removes the `AInode` distribution.
- **The live engine test runs on Windows** (`tests/engine_live.rs`, one flow
  for every platform; `lab/evidence/2026-10-14/windows-live/`): the
  distribution imported and the engine up in 2.6 s, the 350M fetched from the
  public host byte-identical, pinned survives quota enforcement, kept once,
  llama-server.exe ready in 2.2 s and answering with the key only, Remove
  frees the space and the engine forgets the model, a helper killed inside
  WSL reads "stopped by signal 9" in the app, a clean stop leaves no socket
  and no helper inside WSL. Leftovers of a crashed run: a helper still
  answering was stopped in 0.8 s by the next Start; one frozen with SIGSTOP
  was found through its pid file and ended, the next Start up in 7.5 s
  (before this, the probe had no timeout: a silent leftover would have held
  the start until its 30 s limit and stayed running beside the new helper).
  PASS in 278 s, on the CPU.
- **A PC without WSL.** Windows 11 (and Windows 10 since 22H2) ships a stub
  `wsl.exe` that knows only `--install`, `--update`, `--status` and `--help`
  until WSL is installed; its whole message table (`System32\en-US\wsl.exe.mui`,
  read 2026-10-14) is its usage text and "Press any key to install Windows
  Subsystem for Linux ... This prompt will time out in 60 seconds". It never
  says "not installed", which is what the app looked for, and a bare call
  waits a minute for a key. The app now asks first whether WSL is installed
  the way the stub itself finds it (the MSI package under `%ProgramFiles%\WSL`,
  the Store package's `wsl.exe` alias under `%LOCALAPPDATA%\Microsoft\WindowsApps`,
  or the old inbox component `System32\lxss\LxssManager.dll`), and reads the
  stub's usage text or prompt as "no WSL" too. Reasoned from the stub's
  binary and message table and unit-tested with those texts; not run on a PC
  without WSL (none here).
- **Audited 2026-10-14** (an independent read of the Windows and Linux code,
  then each finding checked and fixed where real):
  - the engine is started with `wsl --exec`, not `wsl --`: `--` hands the
    line to the user's shell, where a `'`, `$` or `&` in the profile path
    (`C:\Users\O'Brien`) is syntax and the engine would never start;
  - a distribution that is registered but not set up (a run killed between
    the import and the user setup, a deleted disk image) is noticed (`id -u`
    as the engine's user) and imported again; it holds nothing of the user's;
  - every `wsl.exe` call has a ceiling and dies with its timeout, prints
    UTF-8 (`WSL_UTF8=1`), and the codes a user can act on (virtualization
    off, WSL missing) are turned into plain words at the engine start too;
  - the app's `wsl.exe` is in the job object that already ended
    `llama-server.exe` with the app. **Measured**
    (`lab/evidence/2026-10-14/windows-live/kill-wsl-exe.txt`): when
    `wsl.exe` is terminated, WSL hangs the session up and both `ainode-wsl`
    and the helper are gone within 0.5 s; the helper also carries
    `PR_SET_PDEATHSIG` now, and `ainode-wsl` no longer panics (and aborts) on
    a log line it cannot write once the app is gone;
  - each request over the pipe carries its own id and a line that is not its
    reply is skipped, so one stray line on stdout cannot shift every later
    reply by one; an engine's watcher clears only its own pipe;
  - the helper's and llama-server's output is read as bytes: a line that is
    not UTF-8 is shown lossily instead of ending the reader, which would have
    closed the pipe and killed the child with SIGPIPE on its next write;
  - the unix socket-path length check is no longer applied to the Windows
    `%TEMP%` path (a long profile name could stop the app from launching);
  - the release script refuses files beside the manifest's, and the staging
    script accepts the Visual C++ runtime DLLs only with a valid Microsoft
    Authenticode signature.
  Then (the second round, same day) what that list had left: a Stop or Quit
  during a Start no longer waits it out (the start sees the stop at its next
  check and ends, `Engine::cancel`); `ainode-wsl clear` runs before the peers
  are pinned, so a leftover helper cannot save `catalog.json` over them on
  its way out; the release script installs the packed installer itself into
  a scratch folder, checks the engine in there and uninstalls it again
  (skipped on a machine where AInode is installed); `ainode-wsl` is built
  `--locked` from its committed `Cargo.lock`; and a failure before the window
  exists (a bad path, a refused runtime folder) shows a native message box
  (`os::fatal`: MessageBoxW, zenity or kdialog or xmessage, osascript)
  instead of an app that never appears. The live engine test passed again on
  Windows (232 s) and Linux (184 s) with all of it.
- **The first release build (2026-10-14):** `AInode_0.7.1_x64-setup.exe`
  (sha256 `773c7e4f…fa0404`), checked once more on this PC (installer, the
  real window from a first launch, PASS in 846 s).
- **Where the models live is the user's choice** (2026-10-14: let the
  user pick the drive with room). The first screen shows "Where:
  `%LOCALAPPDATA%\AInode\data`" with Change: the fixed drives, each with its
  free space; picking `D:\` makes the folder `D:\AInode`, re-offers the sizes
  for that drive (`state::consent_limits` on its free space), and START
  writes the consent and the settings there, a pointer `location.json` into
  the default folder, and the app relaunches to read its paths again. The
  engine then gets `-modeldir=/mnt/d/AInode/modelnet`, and AInode's own WSL
  disk lives beside the models (`D:\AInode\wsl`). Only at the first start:
  afterwards the folder holds models, and the choice is gone from the sheet.
  The same on Linux (mounted filesystems) and the Mac (the startup disk and
  `/Volumes`).
- **Public since 2026-10-14**: `AInode_0.7.1_x64-setup.exe`
  on v0.7.1 of `ainode-releases`, and the button on
  easybtx.com/ai. The packed-installer probe in `release-windows.ps1` runs
  its uninstaller in place (`_?=<dir>`: NSIS uninstallers otherwise copy
  themselves away and return at once) and clears
  `HKCU\Software\m2promende\AInode`, which otherwise remembers the scratch
  folder as the place to install next (seen: the following silent install on
  the build PC went there).
- **The graphics card is the user's choice** (2026-10-14): Status >
  Graphics card. Left free, `llama-server.exe` gets `--device none -ngl 0`.
  Measured here with `ggml-vulkan.dll` present (`llama-server --list-devices`:
  `Vulkan0: NVIDIA GeForce RTX 3060`, `Vulkan1: Intel UHD Graphics 770`):
  no GPU memory and no process from llama-server, chat on the CPU. This is
  what replaces taking `ggml-vulkan.dll` out of the test copy on a PC whose
  GPU runs a node.
- **Installed and running on the Windows PC for real** (2026-10-14): the
  public setup.exe, its data on D:, the graphics card left free, started at
  login. The tests before it were started from a program packaged as MSIX and
  so ran inside its package container, where writes under `%LOCALAPPDATA%`
  land in the package's own store: the same app, only its paths differed.
- Not yet: Vulkan on a Windows GPU, a real PC without WSL, Windows 10, signing.

## Route 2: port the helper upstream

A real change (Windows sockets or named pipes, process spawning, file APIs,
`statvfs`) across 22 files, the maintainers' call. The numbers above show a
Windows user can have AInode without it, so it is no longer urgent. It has
not been proposed upstream.

## Route 3: the whole Linux app through WSLg

Works today as a tester's path: the Linux `.deb` runs in any Ubuntu 24.04 WSL
distribution and shows its window through WSLg (this is how the Linux build
was driven here, off screen). Not a product: the user installs Linux first.

## Signing and distribution (proposal, nothing bought)

- Unsigned `.exe`/`.msi`: SmartScreen warns ("More info", "Run anyway") until
  the file has reputation. An EV certificate no longer buys SmartScreen
  trust.
- **Microsoft Store (MSIX):** Microsoft signs it for free; it worked for
  PQ Wallet (Store ID 9NNFJBPH98C6). To check before choosing it: MSIX
  virtualizes `%LOCALAPPDATA%` writes, and `wsl.exe` runs outside the package,
  so the WSL disk and the model folder must sit at a path both see (the
  package's real `LocalCache` folder, or a folder the user picks); and the
  Store's policy on apps that set up a WSL distribution.
- Proposal: build route 1 as an NSIS installer first (what easyNode ships for
  Windows), hand it to testers unsigned with the SmartScreen steps written
  down, and package the same build as MSIX for the Store later.
