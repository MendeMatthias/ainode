# AInode app: engine contract

The app is a thin, honest shell around **upstream's own** `btx-modeld`
(btxchain/btx v0.34.16) plus a local llama.cpp runtime for chat. On Linux
and Windows the helper is upstream's own file from upstream's archive. On
the Mac upstream publishes no archive since v0.34.16, so the helper is built
by AInode from upstream's tag with the helper's sources untouched
(`scripts/build-engine-macos.sh`). It never reimplements the model network.
The facts below were observed on 2026-10-12 against v0.34.15
(`docs/experiments.md`) and again on 2026-11-23 against v0.34.16
(`docs/modelnet/1-engine-move.md`); where the two differ, it says so.

## Product rules (non-negotiable)

1. Nothing is written to disk before the user consents to a size, and the size
   shown is the **total** footprint: engine store + runnable copies.
2. `-modelfollowpeers=0`, always. The app never downloads a model the user did
   not ask for.
3. Download only. No `-modelbind`: the app never listens on the network.
4. No earning, reward, income or payment language anywhere.
5. macOS arm64 and Linux x86_64 (glibc 2.38+). Windows runs upstream's Linux
   helper inside WSL2 (`docs/windows.md`): upstream has not ported it (unix
   sockets, POSIX-only code).

## Bundled resources (`src-tauri/resources/engine/`)

| File | Source | Why |
|---|---|---|
| `btx-modeld` | built by `scripts/build-engine-macos.sh` from upstream v0.34.16 (commit `074dac16…`), sha256 `258da6e4…0876` on the toolchain pinned in `stage-engine.sh`. That is the staged file: the release's Developer ID signing changes its signature bytes, so the helper in an installed app has another sha256 | the model network. Statically links OpenSSL 3.5.9; mints its ML-DSA-44 identity in-process, so no OpenSSL tool is bundled (E1 is fixed upstream, btxchain/btx#228) |
| `btx-modeld.BUILD-MANIFEST.json` | written by that script | what the helper was built from: commit, tree, every input's sha256, the toolchain, the flags, and the three deviations outside the helper's sources |
| `LICENSE-btx.txt`, `LICENSE-openssl.txt`, `licenses/` | `scripts/licenses/` (sources and sha256: `PROVENANCE.md` there) | the licence texts of the helper, of OpenSSL, and of the libraries compiled into the helper; on Linux and Windows also of the libraries in `lib/` |
| `llama/` | llama.cpp `b11559` macOS arm64 (MIT): `llama-server` + its dylibs | local chat. Upstream's `generatemodel` is one-shot, reloads the model per call, and returns llama-cli's banner as text with current llama.cpp (E18) |
| `catalog.json` | ours | the models the app offers |
| `hosts.json` | ours | bootstrap `host:port` list. **Empty until the first public host exists.** |

Linux (`scripts/stage-engine-linux.sh`): the helper is upstream's
`libexec/btx-modeld.real` from `btx-0.34.16-x86_64-linux-gnu.tar.gz`
(`9bbfff06…932b`), bytes unmodified, and llama.cpp's Ubuntu Vulkan x64
build, plus `lib/`: the six libraries the helper links from its release
(OpenSSL, SQLite, libgomp, zlib, zstd), passed as `LD_LIBRARY_PATH` to the
helper only. Since v0.34.16 the helper takes libstdc++ and libgcc_s from the
computer (GLIBCXX_3.4.32, CXXABI_1.3.15: Ubuntu 24.04, Debian 13).
`MANIFEST.json` says `"platform": "linux-x86_64"`.

## Paths

- Data: `~/Library/Application Support/AInode/` (`$DATA`); Linux
  `$XDG_DATA_HOME/AInode/` (`~/.local/share/AInode/`)
  - `consent.json`: `{ "total_bytes": u64, "engine_bytes": u64, "accepted_at": unix_s, "app_version": "…" }`
  - `modelnet/`: the helper's `-modeldir`
  - `settings.json`: `{ "theme": "system"|"dark"|"light", "extra_peers": ["host:port"] }`
- Socket: `$TMPDIR/ainode/modeld.sock` (Linux: `$XDG_RUNTIME_DIR/ainode/modeld.sock`,
  else `/tmp/ainode-<uid>/`; the webview's own files go there too, never to
  the home folder before consent), parent dir created `0700`. (macOS caps a
  unix socket path at 104 bytes; `$TMPDIR` is per-user and short. v0.34.16
  sets the socket's parent to `0700` itself and refuses one that is not
  `0700` and ours: the socket must sit in a folder of its own.)

## Starting the engine

Environment: the allowlist only (`PATH`, `HOME`, `TMPDIR`; Linux also
`LD_LIBRARY_PATH=<res>/lib`). No `BTX_OPENSSL`: v0.34.16 mints its identity
in-process. (Up to 0.8.4 the app bundled an OpenSSL tool and a wrapper for
v0.34.15, which exited 2 without one.)
Arguments, single dash, `-key=value` (anything else exits 1):

```
btx-modeld -modeldir=$DATA/modelnet
           -modelstorage=<engine_gib>GiB      # integer GiB, FIXED quota
           -modelseed=manual
           -modelfollowpeers=0
           -modelrpcsocket=$TMPDIR/ainode/modeld.sock
           -modelpeer=<host:port>             # one per bootstrap / extra peer
```

`engine_gib = floor(total_bytes / 2 / GiB)`: half the consent goes to the
engine's piece store, half is reserved for runnable copies (E19: the runnable
checkout is a second full copy, not a hardlink, so a model costs ~2x its size).

Ready when the socket exists and `hello` (or `getmodelnetworkinfo`) answers.
Exit code 2 = fail-closed (its TLS identity, or a socket folder it does not
accept); surface stderr's first line.
Stop: RPC `stop`, then SIGTERM after 3 s, SIGKILL after 8 s. Never leave an
orphan helper: on app quit, stop it; on app start, if a stale socket exists and
nothing answers, delete it.

## RPC framing

Unix stream socket, one request per connection:
`{"jsonrpc":"1.0","id":1,"method":M,"params":[...]}\n`, shut down the write
side, read until the first `\n`, close. Error replies carry
`{"error":{"code":"STRING","message":"..."}}`. Reference client: `lab/rpc.py`.
The helper has **2** unix workers: never hold a connection open, never poll
faster than every 1 s, never run two long calls at once.

## Calls the app uses (all observed)

| Need | Call | Notes |
|---|---|---|
| status | `getmodelnetworkinfo []` | `pq1_ready`, `openssl`, `peers_connected`, `quota_bytes`, `used_bytes`, `storage_reclaimable_bytes` |
| configured peers | `getmodelpeers []` | `{"peers":["host:port"]}` |
| add a peer live | `addmodelnode ["host:port"]` | hostnames and `[v6]:port` accepted |
| library | `listmodels []` | `models[]`: `uri, model_id, label, admission, complete, incomplete, bytes, files[{file_size, ranges[{first,count}], piece_size}]` |
| download | `getmodel [uri, "FREE_ONLY"]` | returns `job_id` at once (async). Calling it twice for the same model starts a second worker: don't |
| progress | `getmodeljob []` | `jobs[]`: `job_id, status (queued/running/done/failed), model_id, result.status ("retrieved")`. **`bytes_committed` is the whole store, not the job**; compute per-model progress from `listmodels` ranges, clipping **each range** to the file's end: `sum(min(file_size, (first+count)*piece_size) - first*piece_size)` / `bytes` (clipping only the total overcounts a file split across ranges) |
| runnable path | `exportmodelpath [uri]` | `path` = checkout dir with the real files. Creates a second full copy (E19) |
| forget | `unhostmodel [uri]` | unpin + unseed. **Does not free bytes.** The app deletes the checkout dir it asked for, and the store pieces become evictable when a later download needs room. v0.34.16 has a real delete, `removemodel [uri]` (E20 fixed upstream, measured 2026-11-23: 463 MB to 76 KB); the app does not call it yet |
| cancel | `cancelmodeljob [job_id]` | takes a **job id**, not a uri; an empty id lists jobs. A cancelled job reports `failed` with error `cancelled` |
| shutdown | `stop []` | |

A resumed job (after an engine restart) reports `queued` while it downloads;
treat `queued` with growing ranges as running.

Observed semantics the UI depends on:
- `peers_connected` counts open connections: **0 when idle**, 8 while fetching
  from one host. It is not "hosts reachable"; never gate a download on it.
- `getmodel` with no peer configured returns `status:"not_local"` and **no job**.
- After `unhostmodel` the model still lists `complete:true`; the backend keeps
  `$DATA/removed.json` so a removed model shows as not downloaded.

## Commands the Rust side exposes to the UI

```ts
get_state(): {
  version: string,
  consent: null | { total_bytes, engine_bytes, accepted_at },
  disk: { free_bytes, total_bytes },          // filesystem holding $DATA
  engine: { state: "stopped"|"starting"|"running"|"error", error?: string, openssl?: string, pq1_ready?: bool },
  network: { peers_configured: 0|1, peers_connected: number, active_host?: string, hosts_known: number, switched?: "from -> to" },
  // failover (0.7.0): a download with no piece for 90 s and a transport error, with another host
  // known, restarts the engine onto the next host (E9: downloads resume); at most once per 2 min;
  // a failed host is skipped for 10 min; add_peer clears the automatic choice.
  usage: { engine_used_bytes, runnable_bytes, own_bytes, total_bytes },   // runnable = sum of checkout dirs; own = $DATA/knowledge
  theme: "system"|"dark"|"light",
  autostart: bool,                            // open = start, unless the user pressed stop
  think: bool,                                // the chat's Think switch: the default for models never set
  face: "random"|"swarm"|"glyphs"|"chip",
  data_dir: string,                           // where the data lives: the default, or the folder chosen at the first start
  gpu: bool,                                  // chat on the graphics card (default); false: llama-server --device none -ngl 0
  count_me: bool,                             // counted among the AInodes online (default true)
  community_id: string | null,                // this install's random 32-hex id in that count; null before consent
  keep_chats: bool,                           // saved conversations; false unless the user turns it on
  models: { [id]: { think: bool, length: "short"|"normal"|"long", temperature: 0..1.5 } }, // per-model settings (0.7.0)
  wsl: null | {                               // Windows only (0.8.4, backend/wslheal.rs): a stuck WSL
    restarting: bool,                         //   AInode restarts WSL now (alone, or on the user's click)
    easynode: bool,                           //   waiting while easyNode restarts WSL (its %TEMP% note)
    stuck_others?: string[],                  //   WSL stuck, these other distributions run: the button names them
    node_here: bool,                          //   a BTX node answers on this PC (it stops too)
  },
  keeper: {                                   // Rust's own timers (0.8.4, backend/keeper.rs), page or no page
    restart_pending: bool,                    //   the engine stopped on its own; started again after 20 s, 1 min, 3 min
    checked_in_at: number | null,             //   the last check-in (unix s) and whether easybtx.com took it
    checkin_ok: bool | null,
  },
}
accept_consent(total_gb: number, data_dir?: string): { relaunch: bool }
  // decimal GB; writes consent.json; refuses < 4 or > 80% of free disk. data_dir only at the
  // first start: another drive or folder for the data (validate::data_dir: a full path on a
  // local drive, outside the program folder). Then the consent and settings go there, a
  // pointer (location.json {data_dir}) into the default data dir, and the UI calls relaunch.
places(): Place[]                           // the drives (Windows: fixed ones) or mounted filesystems, each with
  // { root, free_bytes, total_bytes, data_dir (where AInode would live on it), min_gb, max_gb, current }
relaunch(): never                           // the app restarts and reads its paths again (AppHandle::restart)
start_engine(): void
  // Windows: waits while easyNode restarts WSL; a stuck WSL is restarted when AInode's distribution
  // is the only one running, otherwise the error says so and `wsl.stuck_others` names the rest.
restart_wsl(others: string[]): void
  // Windows: the user's click on that button; refused when a distribution not in `others` runs now.
stop_engine(): void
catalog(): CatalogModel[]                   // bundled catalog.json, each joined with local state:
  // { id, name, publisher, licence, licence_url, size_bytes, uri, summary, source_url,
  //   good_at, ram_gb, offered, local: "none"|"downloading"|"ready", progress: 0..1, runnable: bool }
download(uri: string): void                 // getmodel FREE_ONLY, idempotent per uri
cancel(uri: string): void                   // cancelmodeljob
remove(uri: string): void                   // unhostmodel + delete its checkout dir; a model kept once is also forgotten by the engine (store_once.rs)
reveal(uri: string): void                   // runnable path (below), then `open -R <gguf>`
chat_start(uri: string): { port, key }      // runnable path, then llama-server on 127.0.0.1:<free port>
chat_stop(): void
set_theme(t): void
set_pref(name: "autostart"|"think"|"keep_chats"|"gpu"|"count_me", on: bool): void
set_face(face): void
set_model_prefs(id, prefs): void            // a catalog id; prefs checked (length, 0 <= temperature <= 1.5); settings.json after consent only
knowledge_list(): OwnFile[]                  // the user's own files: { id, name, kind, added, chars, count, bytes }
knowledge_add(name, kind, passages): OwnFile // passages [{t, x}] read by the webview; kept as $DATA/knowledge/<id>.json if it fits the consented total; never before consent
knowledge_remove(id): void                  // id: 16 lowercase hex, checked before any path is built
knowledge_passages(): [{f, t, x}]            // every passage, for the chat's search worker
network_bounties(): upstream JSON          // searchbounties [{scope: NETWORK, limit: 50}], read-only
network_campaigns(): upstream JSON         // searchmodels [{text: "", scope: NETWORK, limit: 50, filters: {object_kind: MODEL}}], then getfundablemodels []; read-only. The search's own reply rides along as `refresh` (the models found and the engine's counters of who was asked and who answered; null if it failed). No words leave: the text is empty
network_search(text): upstream JSON        // searchmodels [{text, scope: NETWORK, limit: 30, filters: {object_kind: MODEL}}]; 1..200 chars; peers see the words. Models only: a bounty's record also matches a model search
network_start_search(): { run, at_ms, answer } // searchmodels [{text: "", scope: NETWORK, limit: 50, filters: {object_kind: MODEL}}], once per engine run; a run counts only after the new helper has its index peers (state.network.engine_run is 0 until then); refused unless the calling window is visible and not minimized; a refusal that sent nothing begins "Not sent: " and leaves the run its search (the page asks again later); a search that was sent is the run's answer, success or error; feeds the chip's count
index_peers(): [{ endpoint, builtin }]       // hosts the user added for the engine to ask when it searches (its index peers); empty by default (the model host is asked anyway); settings.json `index_peers`, read leniently, checked, one form each, at most 4 (the engine asks at most 8 hosts a search, index peers first, 1.5 s for each one down); never the model host the user chose (dropped from the file at the next engine start; add_peer drops it too) nor a host the engine never asks (never_asked: loopback, 0/8, unspecified, localhost, 169.254/16, 100.64/10, 198.18/15, fe80::/64, fc00::/7, ::a.b.c.d, node ports); a failover's host is left out for that run only and stays saved
index_peer_add(hostport): [{ endpoint, builtin }]    // validate::host_port in Rust (canonical IPv4/IPv6, no numeric-looking names), not the model host, not a host the engine never asks (the reason in words), at most 4; addmodelindex first if running, saved only if that worked; under the same lock as add_peer; given again after every engine start (the engine forgets them at restart)
index_peer_remove(hostport): [{ endpoint, builtin }] // validate::host_port; removemodelindex first if running, saved only if that worked (else the host stays, and says so)
link_open(uri): { uri, store_id?, card, show } // validate::model_uri first (Bech32m, MODEL kind); getmodelsharecard + showmodel, both local
link_get(uri, title): void                 // store models -> download(); not-offered catalog models refused; else getmodel FREE_ONLY, never pinned; remembered in settings.links
links(): LinkModel[]                        // the fetched links with progress
link_reveal(uri): void                     // exportmodelpath if it fits the consented total, then `open <dir>`
link_remove(uri): void                     // cancel + unhostmodel + delete its checkout dir; never for store models
                                           // never called: getmodelreleaseeconomics, cacheencryptedmodel (btxchain/btx#225)
chats_list(model_id): ChatMeta[]           // kept chats of a model, newest first
chat_load(id): Chat
chat_save(chat): void                      // a no-op unless keep_chats; $DATA/chats/<id>.json if it fits; ids 16 lowercase hex
chat_delete(id): void
add_peer(hostport: string): void            // validates host:port, persists it as the host in use; a running engine restarts onto it
update_check(): { version, notes, refused }  // update.rs: the feed on ainode-releases; never downloads. refused: the feed's
                                           // files are not signed under AInode_<version><platform suffix>, shown as such
update_install(): void                     // download the update the last check found, stop chat + engine (as Quit), install, restart
```

Updates (0.8.0, update.rs): the UI checks after consent, at start and every 6 hours; the
restart is offered only while no model downloads and no chat is open. An offer is taken
only when every entry of `latest.json` is signed under its release name for the feed's
version, and never below the highest version this install has run (`$DATA/update.json`,
written only after consent). An update replaces the app, never the data folder.

Kept once (0.7.1, store_once.rs): after the runnable path verifies a copy and no download runs,
the artifact's `.piece` files are deleted, `stored-once.json` records it, the engine restarts.
Remove and a Get whose copy is gone make the engine forget it (stopped, its `catalog.json`
entry dropped, the artifact folder deleted). Evidence: lab/evidence/2026-10-13/store-once.

The runnable path (reveal, chat_start): a NEW copy is made only if store plus
existing copies plus this gguf fit the consented total; the returned dir must be
strictly inside `$DATA/modelnet/checkout`; the gguf must match the catalog's size
and, once per copy (path + mtime), its sha256. A mismatch deletes the copy.

Preferences live in `settings.json` (in memory until consent), never in the
webview: the window is `incognito`, so WebKit writes no `~/Library/WebKit` or
`~/Library/Caches` data at all (checked 2026-10-12: a fresh release launch
leaves nothing under `~/Library`). The one pre-consent file is the
single-instance plugin's socket, `/tmp/com_m2promende_ainode_si.sock`, which
exists only while the app runs. Another local user could pre-create that path
and stop AInode from starting; accepted for now, since it needs a second
account on the Mac and costs availability only.

The chat key reaches llama-server as a 0600 file in the 0700 socket dir
(`--api-key-file`), deleted once the server is up: argv and the environment
are both readable with `ps`.

`AINODE_DATA_DIR`, `AINODE_ENGINE_DIR`, `AINODE_SOCKET` and `AINODE_PEERS` are
honoured by debug builds only. The dev bridge refuses to start without
`AINODE_SOCKET` and `AINODE_DATA_DIR`, and with the installed app's own
socket or folder: on the default socket it would stop a running AInode's
engine.

**One host at a time.** 0.34.15 failed a retrieve near the end with 403
"grant identity" when it knew two live hosts for the same model (E22).
v0.34.16 has the fix (measured 2026-11-23: two hosts, retrieved, 0 retries),
but the rule stays for this release: it was not broken by the new engine,
only one host exists, and handing over every host is a change of its own.
So the engine gets exactly one `-modelpeer`: the host
the user added last, else the first in hosts.json. The helper remembers every
peer it was ever given in `<modeldir>/catalog.json` and has no RPC to forget
one, so before each start the app sets that file's `peers` to the one host
(only that key; an unreadable file is left alone). `network.peers_configured`
is therefore 0 or 1 and `network.active_host` names it. Follow-up, one line
each, not in this release: hand over every host again (failover becomes the
helper's); call `addmodelindex` for our host after each start
(`docs/modelnet/0-plan.md`, stage A and C).

**`-pwcchain=main`: not added, and why.** It was the third one-line
follow-up. With a chain name on the engine and on the host, every 5-second
visit also runs peer exchange (`ext/pex`), and the engine's idle loop visits
every address in its peer-exchange cache, not only the configured host
(read in upstream's source at v0.34.16; the exchange measured on 2026-11-23:
8 a minute while idle with the flag, 0 without). An idle AInode would then
ask its host for other addresses all the time and contact what it is given.
Rule: AInode's engine talks only to hosts the user can see in the app, and
to the sources of a download while it runs (the engine keeps contacting
those for up to 15 minutes afterwards). So the flag stays out until upstream
gives a way to have observation without that. The cost: the visits record
nothing (btxchain/btx#946). Evidence:
`lab/evidence/2026-11-23/modelnet-engine/chain-name/`.

**A change in v0.34.16 to know about.** On a model kept once (pieces
deleted, runnable copy kept) `getmodel` no longer answers `local`: it
fetches the whole model again. The app never calls `getmodel` for a kept-once
model whose copy is there (`backend.rs` `download`), measured through the
backend on 2026-11-23. And the idle helper now contacts its host every 5
seconds, with a mutual handshake that shows the host the engine's public key
each time (`SECURITY.md`; reported upstream as btxchain/btx#946).

Polling: the UI calls `get_state` + `catalog` every 1.5 s while visible.

## The AInodes online (easybtx.com)

The engine cannot tell one AInode about another (an app never listens; in
0.34.15 a helper advertises itself only when bound to a specific address with
peer-follow on). So, like easyNode's apps with `/api/node-checkin`, the app
checks in over HTTPS every 5 minutes while it is open, after consent only,
from Rust since 0.8.4 (backend/keeper.rs, the first 30 s after launch; the
page's timers may stop when no one looks): `POST https://easybtx.com/api/ainode-checkin` (header `x-ainode:
ainode-checkin-v1`) with `{schema: 1, node_id: community_id, agent:
"ainode/<version>", platform: "macos"|"windows"|"linux", engine: <state>,
models: <ready>, hosts: <peers_configured>}`, and gets back `{online,
platforms, enginesRunning, withModels, seenToday, at}`: AInode apps that
checked in during the last 15 minutes, self-reported. Between check-ins, and
always with `count_me` off, it reads `GET https://easybtx.com/api/ainode-online`
every minute (cached 60 s at the edge), so a newly opened AInode shows up
within about a minute; a reading with an older `at` never replaces a newer
one (`countDue`, `newerCommunity` in `logic.ts`). The page only reads; Rust
posts. The site's side is
`site/src/lib/ainodeCheckin.mjs` in MendeMatthias/EasyBTX. CSP: `connect-src`
includes `https://easybtx.com`.
