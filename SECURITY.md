# Security

## Reporting a vulnerability

**Do not open a public issue for a security problem.**

Use GitHub's private vulnerability reporting on this repository: the
**Security** tab, then **Report a vulnerability**. That opens a private thread
visible only to the maintainer, and it works without either of us publishing
an email address.

If that button is not there for you, open a normal issue saying only that you
have something to report and asking for a private channel, with no details in
it. A missing button is our bug, not your problem, and you should never be left
choosing between silence and a public disclosure.

## What to expect

AInode is maintained by one person. These are deliberately numbers that can be
met rather than numbers that sound good:

- **Acknowledgement within 5 days.** Often much sooner.
- **An assessment within 14 days** of the acknowledgement: whether we agree it
  is a vulnerability, and roughly what we intend to do.
- **No fixed fix deadline.** A single maintainer cannot honestly promise one.
  We will tell you what we are doing and when we expect it.

There is no bug bounty. We will credit you in the release notes unless you
would rather we did not.

## What is in scope

- The AInode app in this repository.
- The update path: anything that would let a package the release key did not
  sign reach a user, or point the updater at a host we do not control.
- The chat: anything that lets a web page, another program or a model's own
  output reach the local model server, the chat key or the user's files.
- Anything that makes AInode listen on the network, share what it downloaded,
  or contact a host the user cannot see in it.
- Anything written before the user's consent, or outside AInode's own folder.
- The engine staging: a way to get a file that does not match its pinned
  sha256 into a build.

## What is out of scope

- BTX's own engine. `btx-modeld` and the model network are upstream: report a
  vulnerability in them privately there, by the routes in
  [btxchain/btx's SECURITY.md](https://github.com/btxchain/btx/blob/main/SECURITY.md),
  not here and not in a public issue. An ordinary bug in the engine goes to
  [btxchain/btx's issues](https://github.com/btxchain/btx/issues). If you are
  unsure which side a problem is on, report it here and we will route it.
- llama.cpp, which is bundled unmodified: report there, and tell us so we can
  move the pin.
- The easybtx.com website and the public model host's server.
- Anything requiring physical access to an already unlocked machine.

## The trust anchors, so you know what to look at

The in-app updater installs only packages signed by the AInode release key and
signed under their exact release name and version. The public half of that key
and the update endpoint are in `app/src-tauri/tauri.conf.json`; the rule is in
`app/src-tauri/src/update.rs`. The engine files are pinned by sha256 in
`app/scripts/stage-engine*.sh` and `build-engine-macos.sh`. All of these are
protected by CODEOWNERS. If you find a way to make the app accept an unsigned
or differently signed package, or to run an engine file that is not the pinned
one, that is the most serious class of bug in this repository and we want to
hear about it first.

## What the app does, and does not do

- **It never listens on the network.** The engine runs without `-modelbind`, so
  it serves nothing and accepts no connections. Its control socket is a unix
  socket in a 0700 directory owned by the user.
- **The model host sees that an AInode is open.** While the app is open, its
  engine (upstream v0.34.16) contacts the one host it knows every 5 seconds,
  also when nothing is being downloaded: a post-quantum TLS handshake and one
  request, about 12 KB down and 8 KB up each time. Measured 2026-11-23, two
  ways: through the app, 24 visits in 120 s, 297,360 bytes from the host and
  197,184 to it; helper alone through a counting forwarder, 12,366 and 8,266
  bytes in each of 20 visits. Over a day that is roughly 350 MB of traffic.
  What the host learns from a visit:
  - this computer's IP address, for as long as AInode is open and not only
    during a download (measured: the connections);
  - the engine's identity, a public key made on this computer at the first
    start. The handshake is mutual, so the host sees the same key at every
    visit and every download, also from another network, until the
    `modelnet/tls` folder is deleted. It is a number, not a name; it lets a
    host tell one AInode from another. That was already so for downloads on
    the v0.34.15 engine (read in upstream's source, `transport_pq.cpp:68`;
    not measured on a host);
  - nothing about your chats, which never leave the machine, and no list of
    your models: the request is an empty `availability` question (read,
    `helper.cpp:12759`; the forwarder does not decrypt, so the content was
    not measured).

  The v0.34.15 engine made no such visits with AInode's settings (measured:
  0 in 120 s). No engine setting stops them in v0.34.16 while the engine
  knows a host (read: the loop runs for every helper). In AInode 0.9.0 the
  visits record nothing: the engine keeps what a host says about itself only
  when both sides name the same chain, and AInode does not name one (next
  point). Reported upstream as
  [btxchain/btx#946](https://github.com/btxchain/btx/issues/946).
- **Other hosts can see an AInode during and shortly after a download.**
  While a model downloads, the engine asks the host it fetches from which
  other addresses have the model, and may fetch pieces from them. The engine
  has done that since v0.34.15, so AInode 0.8.4 does it too (read in
  upstream's source). Every piece is checked by its hash, so such a source
  cannot hand over wrong bytes; but it sees this computer's IP address and
  the engine's public key, like the model host. New with the v0.34.16
  engine: it keeps contacting those addresses for up to 15 minutes after the
  download (read). Outside that and searches, an idle AInode contacts only the one host
  it was given. That is why AInode does not pass the engine a chain name
  (`-pwcchain=main`): with one on both sides, the engine asks its host for
  other addresses at every 5-second visit, all the time, and contacts what
  it is given (read, and the exchange measured: 8 a minute while idle with
  the flag, none without).
  Measured on 2026-11-23: the model host named no other address, and every
  connection of a test engine, idle and downloading, went to the model host.
- **Searches go to the model host, and to hosts you add yourself.** A search
  runs once when the engine starts (for the count on the home screen's chip,
  only while the window is shown, checked by the app and by the window
  itself, and never twice in the same engine run), when the BTX ModelNet
  view opens or refreshes, and when you search. Each host asked sees this
  computer's IP address, the engine's public key and the words searched for
  (none in the automatic ones). The engine always asks the model host it was
  given (measured 2026-11-24: with no other host configured, the start-up
  search asked 1 host and it answered), so the list in the view starts
  empty and holds only hosts you add; the view shows the model host above
  it. It holds at most 4, and refuses what the engine would never ask: a
  host on this computer, link-local and a few other special ranges, and a
  BTX node's own ports. A host you add is asked only when a search runs: measured with a test
  host, 0 connections in 300 s idle, one per search, none during two
  downloads. AInode fetches models
  only from its model host, not from the hosts you add. One more group is
  asked by a search, by the engine's own rule: the other sources the model
  host named during a download in the last 15 minutes (the point above).
  They would see the search words too (read in upstream's source,
  `helper.cpp:7795-7806`; the model host has named none in any measurement,
  so this was not seen happening).
- **Every byte is checked.** Models are content-addressed; each 4 MiB piece is
  verified by SHA-384 before it is kept, and the runnable file is checked
  against the publisher's sha256 before it is loaded.
- **Chat stays on the machine.** llama.cpp's server binds 127.0.0.1 and
  requires a random per-session key, handed over in a 0600 file that is
  deleted once read (never argv, never the environment). Only the health check
  answers without it. CORS allows exactly the app's own origin.
- **AInode writes nothing before consent.** The webview runs incognito; the
  app writes its data folder only after the user chooses a size. One thing
  macOS does by itself for any app with a web view, as soon as the window
  opens: three empty per-process folders in the system's temporary folder,
  and a graphics (Metal) shader cache of about 70 KB in the system's per-user
  cache folder (`/var/folders/…/C/com.apple.WebKit.GPU+<app id>`). Neither is
  in your home folder and neither holds anything of yours (measured
  2026-11-23 on a test build before consent; the installed app has the same
  folders).
- **The webview is locked down.** No Tauri shell, filesystem or HTTP plugins;
  of Tauri's own commands only the four the interface uses (window events,
  minimized, dragging); `script-src 'self'`; model output is rendered as text;
  only plain `https://` links leave the app, never one with a user name.
- **Your data is yours alone.** On Mac and Linux AInode's files are 0600 in
  0700 folders. On Windows its data folder gets a private ACL (you, SYSTEM,
  Administrators) when it is made, also on another drive, whose root would
  otherwise let every account in; it is checked at every start and before
  the engine starts. A folder found open is taken back whole: its owner, and
  every file in it another account owns or may use (reset, or deleted, links
  never followed). The engine does not start while it stays open.
- **Windows: the engine's Linux sees two folders.** AInode's own WSL
  distribution mounts only the engine folder (read-only) and the models folder
  (never executable), none of your drives, and cannot start Windows programs
  or reach your screen, clipboard or microphone (no WSLg) or WSL's GPU
  libraries.
  It is made again from each release's Ubuntu Base.
- **Updates are signed and bound.** Every update is checked against AInode's
  own key and must be signed under its exact release name and version; an
  older release is refused. A `.deb` asks for your password to update, like
  any other package: never add a sudo rule for it (a wildcard rule for
  `dpkg -i` in `/tmp` gives root to any program you run).
- **Bundled engine files are pinned** by sha256 at staging and checked in the
  finished bundle (`app/scripts/check-bundle.sh`). On Linux and Windows the
  model helper is upstream's own file from upstream's release. On the Mac
  it is built by AInode from upstream's tag, the helper's sources untouched,
  because upstream publishes no Mac file since v0.34.16; the build is
  scripted (`app/scripts/build-engine-macos.sh`), gave the same bytes in
  every run on the toolchain it is pinned for (five runs in five folders on
  2026-11-23; another Xcode is expected to give other bytes, and staging
  stops on any helper that does not have the pinned sha256), and
  its manifest ships inside the app
  (`resources/engine/btx-modeld.BUILD-MANIFEST.json`). Three things outside
  the helper's sources differ from an upstream build and are listed in that
  manifest: one cmake line that includes SwapNet is commented out, the Metal
  kernels compile at run time, and OpenMP comes from LLVM's source.
  The sha256 in the manifests is that of the file before the release signs
  it. Developer ID signing changes the file's signature bytes, so the helper
  in an installed AInode has another sha256 (as it had with upstream's file
  up to 0.8.4); `check-bundle.sh` compares before signing.

## Known limits

- The Windows files are not Authenticode-signed yet (`docs/SIGNING.md`,
  Windows); the Mac's are signed and notarized.
- The single-instance lock uses a socket in `/tmp`. Another local user could
  pre-create it and stop AInode from starting (availability only).
- A host is trusted on first use (TOFU): its post-quantum identity is pinned on
  first contact, so a host that rebuilds without restoring its identity breaks
  until its pin is cleared.

## Before each release

The first audit (2026-10-18) left these checks; the second (2026-10-19) ran
them on release builds:

- `cargo audit` in `app/src-tauri` (exceptions only in `.cargo/audit.toml`,
  each with its reason) and `npm audit` in `app`: no new advisory.
- The newest Ubuntu Base point release in `stage-engine-windows.sh` (a new
  pin makes every Windows AInode build its distribution again from it).
- Windows: signed (`release-windows.ps1` says `signed:`, not `UNSIGNED`) once
  a certificate exists.
- Click through the release build's own window: the home screen, the BTX
  ModelNet view, Status. No automated check reads those screens: the first
  build of 0.9.0 said "BTX v0.34.15, unmodified" in Status while its engine
  was v0.34.16, built by AInode. That line now comes from the engine folder's
  manifest (`engine.rs` `bundled`), and `check-bundle.sh` holds the manifest
  to the pinned tag; the click-through is for whatever is next.
- Nothing listens: with the engine running and a chat open, no AInode process
  holds a TCP socket outside 127.0.0.1 (`Get-NetTCPConnection -State Listen`,
  `ss -ltnp`, `lsof -iTCP -sTCP:LISTEN`).
