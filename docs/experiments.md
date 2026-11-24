# Experiments on upstream's model network, 2026-10-12

Every result here was **run**, not read, against upstream's own release:
`btx-0.34.15-arm64-apple-darwin.tar.gz` (sha256 `6e1ec111…c4a8`, matches GitHub's
asset digest), `btx-modeld` unmodified, on an Apple M5, macOS 27.0.1. Lab code is
in [`lab/`](../lab); every number can be reproduced with it.

**What these experiments cannot prove.** Everything ran on one Mac. The WAN
emulator (`lab/wanproxy.py`) reproduces latency, a shared bandwidth cap, stalls
and drops in userspace; it does not reproduce packet loss, MTU limits or
middleboxes. Phase 0's acceptance test, a fetch from a different machine on a
different network, still needs the VPS.

Models: IBM Granite 4.0 350M Q4_K_M (237 MB) and Granite 4.2 3B Q4_K_M (2.24 GB),
both Apache-2.0, both verified against IBM's published sha256 after every fetch.

## Summary

| # | Experiment | Result |
|---|---|---|
| E1 | Start `btx-modeld` on a stock Mac | **Fails closed, exit 2**: "openssl mldsa44 certificate generation failed". Every Mac user of upstream's release hits this, including btx-qt's Models dock |
| E2 | Same, with an OpenSSL 3.5.9 CLI built from source | Still fails until that CLI finds an `openssl.cnf`. **Setting `OPENSSL_CONF` does not help: the helper clears it** (`pq1_runtime.cpp:496-501`); `BTX_OPENSSL` must point at a wrapper that sets it |
| E3 | Host and fetch on loopback (350M) | Works. 147 MB/s, BYTES_VERIFIED, file sha256 equals IBM's |
| E4 | Import the same two files on a fresh helper, from a differently named folder | **Same `btx://` link**; a CRLF copy of the licence gives a different one |
| E5a | 350M at 130 ms RTT, 20 Mbit/s | 102 s, **93% of the line** for payload (start-up included), first piece 1 s, 0 retries |
| E5b | **3B (2.24 GB) at 130 ms, 20 Mbit/s** | **946 s (15.8 min), 95% of the line**, 0 retries, 58 connections, 4.4% wire overhead |
| E5c | 350M at 300 ms, 2 Mbit/s | 997 s, 95% of the line, 0 retries, but the first model piece lands only at **117 s** |
| E5d | 350M at 300 ms, 0.5 Mbit/s | **~50% of the line wasted**: pieces finish arriving just after the 600 s request deadline and are discarded and re-requested |
| E7 | Five plain TCP port probes, then a real fetch | **The prober's IP is locked out ~60 s**, then the same job recovers by itself |
| E8 | Host with `-modeluploadlimit` (2, 5, 33, 64 MiB/s) | **Every fetch fails permanently** within seconds with HTTP 429 |
| E9 | SIGKILL the fetcher at 37%, restart it | **Resumes by itself**, keeps verified pieces; 19% extra bytes (the in-flight window) |
| E10 | SIGKILL the host at 30%, down 90 s, restart | **Fetcher waits and completes by itself**; host re-serves from its catalog with no re-import |
| E11 | Host's TLS identity regenerated | **Every earlier fetcher fails at once and for good**: "peer cert pin mismatch (TOFU)" |
| E13 | A helper fetches with `-modelbind` set, then a third helper fetches only from it | **Serves the identical artifact at once** (237 MB served): host number two is one fetch |
| E14 | Two fetchers behind one IP at once | Both complete (2.8 s and 6.2 s on loopback), sharing the 8-per-address cap |
| E15 | Budget smaller than the model | Fails at once and clearly: `disk quota` |
| E16 | `getmodel` with no peer configured | `status: not_local`, `peers: 0`, **no job**: nothing waits, nothing retries |
| E12 | btxchain/btx#225 with an alias | **Reproduced**: the call never returns; other RPCs and serving keep working; the helper then **ignores SIGTERM** |
| E18 | Upstream `generatemodel` with current llama.cpp | Works, but returns llama-cli's ASCII banner and menu as the answer text |
| E19 | Disk use of a fetched model | **Two full copies**: 4 MiB pieces in the store plus the runnable checkout (not a hardlink) |
| E20 | Delete a fetched model | **No RPC frees the bytes** in v0.34.15 |
| E21 | Damage the runnable copy, then `exportmodelpath` again | **Repaired**: a new file with the right sha256 (5.8 s for 2.2 GB) |
| E22 | Two live hosts for one model, fetcher told about both | **Fails** 4 of 4 near the end: 403 "grant identity". Patch 0008 fixes it; failover is fine either way; two hosts give no speed-up |
| E22b | Does a second host reach fetchers by gossip? | **Not in 0.34.15** for a host bound to `0.0.0.0` (it never advertises itself) |

## Details

### E1, E2: the model plane cannot start on a stock Mac

```
$ btx-0.34.15/bin/btx-modeld -modeldir=… -modelstorage=1GiB -modelfollowpeers=0 -modelrpcsocket=…
model subsystem fail-closed: openssl mldsa44 certificate generation failed
exit=2
```

The helper statically links OpenSSL 3.6.3 (`strings` on `libexec/btx-modeld.real`)
but mints its ML-DSA-44 certificate by shelling out to a separate `openssl` CLI
(`src/modelnet/helper.cpp:9085-9097`, called unconditionally at startup at
`:10561`, before any host or bind check). The CLI search order is `BTX_OPENSSL`,
`/opt/openssl-3.5.8`, Homebrew, then `PATH` (`pq1_runtime.cpp:81-98`). macOS ships
LibreSSL 3.3.6. The release archive copies OpenSSL **libraries** only, never a CLI
(`scripts/release/package_release_archive.py:233`), and the v0.34.15 release notes
say nothing about it; only the build docs mention Homebrew.

With a CLI that has no config file, `openssl req` refuses to run (E2). The
obvious fix, exporting `OPENSSL_CONF`, does not work: the helper deliberately
clears `OPENSSL_CONF`, `OPENSSL_CONF_INCLUDE`, `OPENSSL_MODULES` and
`OPENSSL_ENGINES` before it runs the CLI (`pq1_runtime.cpp:496-501`). Our first lab
runs only passed because `make install_ssldirs` had put a default config where
that build looks for one; the app's staged CLI (`OPENSSLDIR "/usr/local/ssl"`)
has none and made the helper exit 2. What works is a one-line wrapper given to
the helper as `BTX_OPENSSL` (`#!/bin/sh` / `OPENSSL_CONF=… exec openssl "$@"`),
verified with the config-less CLI through `host/fetch-test.sh`. The config itself
can be this small:

```
openssl_conf = openssl_init
[openssl_init]
[req]
distinguished_name = dn
[dn]
```

**Upstream fix candidate:** mint the certificate in-process with the OpenSSL the
helper already links (`EVP_PKEY_Q_keygen(…, "ML-DSA-44")`, `X509_sign` with a
NULL digest), and drop the CLI dependency for every platform.
**AInode's workaround:** bundle OpenSSL 3.5.9 (6.5 MB static) plus the config.

### E5: throughput on emulated long links

The emulator delays every chunk by RTT/2 each way through a FIFO delay line, and
shares one token bucket across all connections in each direction (a home line is
one pipe). Its first version deadlocked on chunks larger than its 50 ms burst,
which looked exactly like a BTX stall; that was found, fixed and unit-tested
(1.88 and 0.49 Mbit/s measured against 2 and 0.5) before these runs.

Payload rate = model bytes / total time from the request, start-up included.

| Run | Link | Size | Time | Payload rate | First model piece | Retries | Connections |
|---|---|---|---|---|---|---|---|
| E5a | 130 ms, 20 Mbit/s | 237 MB | 102 s | 2.31 MB/s (93%) | 10.5 s | 0 | 10 |
| E5b | 130 ms, 20 Mbit/s | 2.24 GB | 946 s | 2.37 MB/s (95%) | 15.3 s | 0 | 58 |
| E5c | 300 ms, 2 Mbit/s | 237 MB | 997 s | 0.238 MB/s (95%) | 117 s | 0 | 59 |
| E5d | 300 ms, 0.5 Mbit/s | 237 MB | see below | ~0.03 MB/s (~50%) | ~500 s | 0 | – |

Wire overhead is 4.4% (2,341,751,778 bytes on the wire for 2,244,022,910 of
payload), the cost of upstream's 512-byte TLS records (`transport_pq.cpp:62-66`,
added because 16 KiB records stalled a real WAN retrieve).

**E5d, the slow-link cliff.** The fetcher keeps 8 pieces of 4 MiB in flight
(`pq1_runtime.h:28`, `types.h:74`) and gives each request 600 s
(`PQ1_TRANSFER_MS`). At 0.5 Mbit/s, 8 x 4 MiB takes ~537 s, just inside the
deadline, so some pieces complete and some do not. The connection log shows it:
connections that closed at 600.9-603.4 s had **delivered the full 4,383,811-byte
piece** and were then dropped by the client's deadline, with less upload (no
follow-up request) than the ones that finished at 494-584 s. Those pieces are
fetched again. Below roughly 0.45 Mbit/s nearly every piece would be discarded the
moment it arrives. **Upstream fix candidate:** size the in-flight window to the
measured rate (or scale the deadline with it).

### E7: a port probe locks the prober out

Five `nc -z` probes against a fresh host, then a real fetch from the same IP: the
fetch saw `unexpected eof` for ~55 s (6 retries), then completed on its own. The
host bans an address after 4 unauthenticated handshakes in 60 s
(`PQ1_UNAUTH_HANDSHAKE_LIMIT`). Consequences: never monitor a host with a TCP
check; and every user behind one carrier-NAT address shares the ban.

### E8: an upload limit breaks serving completely

| `-modeluploadlimit` | Result |
|---|---|
| 2 MiB/s | failed in 2.3 s: `missing FreeGrant HTTP 429` |
| 5 MiB/s | failed at once: `RESOURCE_GOVERNOR: background upload budget exhausted` |
| 33 MiB/s | failed in 0.5 s after 8.4 MB: `missing FreeGrant HTTP 429` |
| 64 MiB/s | failed in 0.5 s after 21 MB: `missing FreeGrant HTTP 429` |

The host answers 429 with `Retry-After: 1` (`helper.cpp:2299-2303`), so the intent
is "slow down". The client retries a piece 8 times back to back with no backoff and
ignores `Retry-After` (`helper.cpp:9330-9370`), and a 429 on the grant request is
classed as permanent (`pq1_runtime.cpp:316`). The limit therefore never throttles:
it kills the transfer. **Never set `-modeluploadlimit` on a host.**
**Upstream fix candidate:** honour `Retry-After` without spending a retry, and
let the governor delay instead of reject.

### E9, E10: interruption

Fetcher SIGKILLed at 88 of 237 MB, restarted with the same data dir and no new
request: the job reappeared as `queued` (while downloading), kept every verified
piece and finished; 282,742,822 bytes crossed the wire for 236,997,118 of payload
(the in-flight window at the moment of the kill is lost).

Host SIGKILLed at 71 MB with the fetcher mid-transfer, kept down 90 s: the fetcher
retried with backoff (4 to 9 retries) and finished by itself after the host came
back. The host re-served from `catalog.json` without a re-import.

### E11: the host's TLS key is load-bearing

Fetchers pin a host's ML-DSA key on first contact in `<modeldir>/tls/pins.json`
(`{"ip:port": sha384}`). Regenerating the host's `tls/` (a rebuilt VPS, a lost
disk) makes every earlier fetcher fail in 0.25 s with
`peer cert pin mismatch (TOFU)`, no retry. Recovery needs each user to delete that
entry. Content is SHA-384-verified regardless, so the pin protects the transport,
not the model. **The host's `tls/` must be backed up like a key.**

### E12: #225

`setmodelalias <uri> smoke`, then `getmodelreleaseeconomics ["smoke"]`: no answer
in 20 s, ever. `getmodelnetworkinfo` still answered in 0.00 s and a full fetch from
the host still completed at 84 MB/s, so the blast radius is one of the two unix
workers. But the helper then ignored SIGTERM for 3+ s and needed SIGKILL. AInode
never sends aliases and always escalates to SIGKILL on stop.

### E18: local generation

`generatemodel` execs `$BTX_LLAMA_CLI -m … -p … -n … --no-display-prompt`
(`helper.cpp:528-530`). With current llama.cpp (b11559), `llama-cli` is an
interactive chat program, so the RPC's `text` is its ASCII banner, spinner control
codes and command menu around the answer. Pointing `BTX_LLAMA_CLI` at
`llama-completion` gives clean text plus a trailing `> EOF by user`; adding
`-no-cnv` to the argv would remove that. It is also one-shot (model reloaded per
call). AInode uses llama.cpp's `llama-server` on 127.0.0.1 for chat instead:
Granite 4.2 3B streams at ~50 tokens/s on the M5.

### E19, E20: disk

After a fetch, the store holds the model as 4 MiB `.piece` files, and
`exportmodelpath` writes a second, runnable copy under `checkout/` (different
inodes; `du` 226 MB + 226 MB for a 237 MB model). The `-modelstorage` quota covers
the store only. `unhostmodel`, `unseedmodel` and `unpinmodel` free nothing; the
`*erasure*` RPCs are erasure coding, not deletion; bytes are reclaimed only by
eviction when a later download needs room (`catalog.cpp:375-425`). AInode therefore
counts every model twice against the user's budget and gives the engine half.

### E21: the runnable copy is self-healing

One byte flipped in the middle of the 2.2 GB checkout gguf (sha256 then
`274ad043…`). The next `exportmodelpath` took 5.8 s and left a file with a new
inode and IBM's sha256 (`e0406663…`). So the checkout is checked against the
store on export and rebuilt when it differs. An intact export is quick (a whole
chat start, export plus model load, took 2.7 to 3.2 s). AInode still hashes the
gguf once per copy before loading it (0.75 s, ARMv8 SHA instructions), which
now only covers a change made between the export and the load.

### E22: two hosts for one model

Two release hosts import the same 237 MB package (same link, E4), each behind
its own emulated 10 Mbit/s, 60 ms line (`lab/exp-multihost.sh`). The fetcher is
told about one or both with `-modelpeer`.

| fetcher | hosts | result | time | retries | via host 1 / host 2 |
|---|---|---|---|---|---|
| 0.34.15 release | one | retrieved | 199.6 s | 0 | 247 / 0 MB |
| 0.34.15 release | both, 3 runs | **failed** 3 of 3 | 257.5 s | 54 | 240 / 13.6 MB |
| 0.34.15 release | both, host 2 over IPv6 | **failed** | 265.7 s | 55 | 245 / 9.9 MB |
| patches 0001-0007 | both | **failed** | 257.7 s | 54 | 240 / 13.6 MB |
| + 0008 | both, 2 runs | retrieved | 196.8 s | 0 | 243 / 4.4 MB |
| + 0008 | both, host 2 over IPv6 | retrieved | 197.6 s | 0 | 4.5 / 243 MB |
| 0.34.15 release | both, host 1 (primary) SIGKILLed at 31% | retrieved | 217.8 s | 17 | 73 / 178 MB |
| + 0008 | both, host 1 (primary) SIGKILLed at 30% | retrieved | 198.3 s | 0 | 101 / 147 MB |

Every failed run stopped at 232,802,814 of 236,997,118 bytes with
`piece HTTP 403 {"error_code":"ENTITLEMENT","message":"grant identity"}`.

Cause (`helper.cpp`, read): the primary host's pooled sessions send a shared
`grant_headers` (9314-9318); `issue_grant` overwrites it with the grant of
whichever host answered last (9248-9252), and the swarm asks every extra host
for one (9645, 9844). From then on new piece requests to the primary carry
another host's grant, which the primary refuses (`free_grant.cpp:409-414`), and
that 403 is not one the fetcher renews (9334-9337). Patch 0008 keeps each
host's grant on its own session and renews on "grant identity".

Also measured: two hosts give **no speed-up**. One host carries almost all
bytes; the other gets about one piece per round. Which one is primary depends
on the order the helper lists them, and IPv6 (a second netgroup) did not
change the split. Failover works: a dead primary costs 18 s on the release and
nothing with 0008.

### E22b: does a second host spread by gossip?

Host 2 became a host by fetching from host 1 with `-modelbind` (the route the
#227 draft suggests); then a fresh fetcher was told only about host 1
(`lab/exp-gossip.sh`). It never connected to host 2 and finished normally
(199.7 s). Host 1 received one gossip record and accepted none. In the source, a
helper advertises itself only if it is bound to a specific address or has a
NAT mapping (`LocalPexEndpoint`, `helper.cpp:4403-4409`); one bound to
`0.0.0.0`, as `host/btx-modeld.service` is, never does. So in 0.34.15 a fetcher
meets a second host only when told about it. Not tested: a host bound to a
specific public address, which would advertise.

What AInode does about it: the engine is given exactly one host (the one the
user added last, else the first bundled), and before each start the helper's
remembered list in `catalog.json` is set to that one, because the helper keeps
every host it was ever given and has no RPC to forget one
(`catalog.cpp:1172-1182`). Checked through the dev bridge: two remembered
hosts became one; "Use a host" restarts the engine onto the new host in 0.5 s
and the helper then knows only it.

## Fixes, built and tested

The two worst findings are fixed in a source build of v0.34.15 (the commits:
[`MendeMatthias/btx-upstream` `ainode/modelnet-fixes-v0.34.15`](https://github.com/MendeMatthias/btx-upstream/commits/ainode/modelnet-fixes-v0.34.15)):

- **E1:** the patched helper mints its ML-DSA-44 identity in-process and starts
  on a stock Mac with no OpenSSL CLI; it interoperates with the unmodified
  release in both directions.
- **E5d:** the retrieve window adapts to the measured rate. At 0.25 Mbit/s
  the release keeps 0 MB in 20 minutes (all 8 complete pieces dropped at the
  deadline, forever); the patch keeps 33.6 MB (90% of the line); 20 Mbit/s is unchanged (102 s).
- **E20:** `removemodel` frees a model's pieces and checkout (452 MB to 64 KB),
  refuses during a download, which then completes intact.
- **E8:** the upload governor paces instead of refusing. A 2 MiB/s cap now
  delivers 2.07 MB/s with 0 retries, **also to the unmodified release fetcher**;
  33 MiB/s delivers 31.9 MB/s; no cap is unchanged (153 MB/s).
- **E22:** each host's FreeGrant stays on its own session (0008). Two live
  hosts: release fails 4 of 4, patched retrieves 3 of 3 in the one-host time
  with 0 retries, against unmodified hosts; a killed primary costs nothing.

## Upstream candidates, in order of impact

1. In-process ML-DSA certificate minting (E1): the model plane does not start for
   any Mac user of the release today.
2. Keep each host's grant on its own session (E22): today a second host for a
   model makes downloads of it fail for every client told about both. Must
   ship before the network has two hosts for anything.
3. Honour `Retry-After` and backpressure instead of 429 (E8).
4. Adaptive in-flight window for slow links (E5d).
5. A real delete for local models (E20), and an optional single-copy checkout (E19).
6. `generatemodel` argv for current llama.cpp (E18).
7. Doc fixes: `-modelhost` binds `127.0.0.1` (architecture.md says unix-only);
   `NoteReport` is never called, so PUBLIC_DIRECT is unreachable
   (reachability.md describes dial-backs that do not happen).

This list is as written on 2026-10-12. Items 1 to 5 went upstream afterwards
as btxchain/btx#228 to #232, and upstream took the fixes into v0.34.16.
