# Third-party components

What the app bundles or downloads, at which version, under which licence. Each
engine file is pinned by sha256 in `app/scripts/stage-engine.sh` (Linux:
`stage-engine-linux.sh`, Windows: `stage-engine-windows.sh`) and recorded in
the bundle's `resources/engine/MANIFEST.json`.

| Component | Version | Licence | How it is used |
|---|---|---|---|
| BTX `btx-modeld` ([btxchain/btx](https://github.com/btxchain/btx)) | v0.34.16 (`074dac16`). Linux and Windows: upstream's own file from `btx-0.34.16-x86_64-linux-gnu.tar.gz`, bytes unmodified, with the libraries that archive ships for it. Mac: built by AInode from that tag with `app/scripts/build-engine-macos.sh`, the helper's sources untouched (upstream publishes no Mac archive); what went in is in `resources/engine/btx-modeld.BUILD-MANIFEST.json` | MIT (`resources/engine/LICENSE-btx.txt` on the Mac) | The model network engine |
| [OpenSSL](https://www.openssl.org/) | 3.5.9, built from the release tarball, linked statically into the Mac helper. No OpenSSL tool is bundled any more: since v0.34.16 the engine mints its ML-DSA-44 identity itself (btxchain/btx#228) | Apache-2.0 (`resources/engine/LICENSE-openssl.txt`) | The engine's post-quantum TLS |
| [SQLite](https://sqlite.org/) | 3.46.1, built by upstream's `depends`, linked statically into the Mac helper | Public domain | The engine's local security store |
| [Boost](https://www.boost.org/) | 1.81.0 headers, built by upstream's `depends` | BSL-1.0 | Needed to configure and compile upstream's tree. No Boost symbol is left in the Mac helper (measured with `nm`) |
| Libraries inside BTX's own source tree at that tag: [secp256k1](https://github.com/bitcoin-core/secp256k1) (`src/secp256k1`), [LevelDB](https://github.com/google/leveldb) (`src/leveldb`), UniValue (`src/univalue`), libbitcoinpqc (`src/libbitcoinpqc`) | as vendored by upstream at v0.34.16 | Each under its own terms in upstream's tree: `src/secp256k1/COPYING` (MIT), `src/leveldb/LICENSE` (BSD-3-Clause), `src/libbitcoinpqc/LICENSE` (MIT) with the licence files of the Dilithium and SPHINCS+ code inside it; UniValue has no file of its own and falls under BTX's `COPYING`. The texts ship in `resources/engine/licenses/` on every platform | Compiled into the helper, as in upstream's own builds (Mac helper: measured with `nm`; upstream's Linux file: not inspected) |
| [LLVM OpenMP](https://openmp.llvm.org/) | 23.1.6, built from the LLVM source release | Apache-2.0 with LLVM exception | Needed to configure a Mac build of upstream's tree. No OpenMP code ends up in the helper (measured: no `__kmpc` symbol) |
| libevent | 2.1.12, built by upstream's `depends` | BSD-3-Clause | Needed to configure upstream's tree. Not linked into the helper |
| [llama.cpp](https://github.com/ggml-org/llama.cpp) `llama-server` | b11559, macOS arm64 release | MIT (`resources/engine/llama/LICENSE`) | Runs the model locally for chat |
| [Tauri](https://tauri.app/) | 2.12 | MIT or Apache-2.0 | The app shell |
| [pdf.js](https://github.com/mozilla/pdf.js) (`pdfjs-dist`, legacy build) | 6.3.289 | Apache-2.0 | Reads the text of a PDF the user drops into the chat, in the app, offline; loaded only when a PDF is added |
| [JetBrains Mono](https://www.jetbrains.com/lp/mono/) | woff2, Regular and Bold | SIL Open Font License 1.1 | The interface font, bundled, never fetched |
| BTX documentation ([btxchain/btx](https://github.com/btxchain/btx) at v0.34.16, `074dac16`): README, HUMANS.md, the 0.34 release notes, the design essays, the model-network docs | 815 passages, `app/src/knowledge/btx-docs.json`, built by `lab/knowledge/build-btx-docs.py` | MIT | What the chat hands a model when the conversation is about BTX |
| postquantum.wiki (Mende's post-quantum reference, btx-apps `apps/wiki/content` at `9f91ac07`) | 155 articles, 970 passages, `app/src/knowledge/pq-wiki.json` | All rights reserved; included with Mende's permission (12 October 2026). Not MIT | What the chat hands a model when the conversation is about quantum or post-quantum |
| Wikipedia, opening paragraphs of the [Vital Articles, Levels 4 and 5](https://en.wikipedia.org/wiki/Wikipedia:Vital_articles/Level/5) | 46,941 articles, fetched 12 October 2026 (`app/src/knowledge/wiki-attribution.json` lists each article and revision) | [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/), by Wikipedia contributors; adapted (opening paragraph only, pronunciations removed, cut near 600 characters) | The offline encyclopedia the chat consults; `app/src/knowledge/wiki.json` is under CC BY-SA 4.0, not MIT |

## What each platform's engine folder ships

The engine folder is `resources/engine` inside the app. Every file in it is
named in its `MANIFEST.json` with its source and sha256.

| | Mac | Linux (`.deb`, AppImage) | Windows |
|---|---|---|---|
| `btx-modeld` | built by AInode from upstream's tag | upstream's file, unmodified | upstream's Linux file, unmodified (it runs inside AInode's WSL distribution) |
| Inside the helper | OpenSSL 3.5.9, SQLite 3.46.1, and from BTX's tree secp256k1, LevelDB, UniValue, libbitcoinpqc (measured, `nm`) | the libraries from BTX's tree (same source; not inspected) | as Linux |
| `lib/`, beside the helper | nothing | six files from upstream's archive, unmodified: `libssl.so.3` and `libcrypto.so.3` (OpenSSL 3.5.5), `libsqlite3.so.0` (SQLite 3.46.1), `libgomp.so.1` (GNU OpenMP), `libz.so.1` (zlib), `libzstd.so.1` (zstd 1.5.7) | the same six |
| From the system, not shipped | macOS's own libraries | glibc 2.38 or newer, `libstdc++` and `libgcc_s` from GCC 14 or newer | the same, from the Ubuntu Base 24.04.5 image AInode ships |
| `LICENSE-btx.txt`, `LICENSE-openssl.txt` | yes | yes | yes |
| `licenses/`: secp256k1, LevelDB, libbitcoinpqc (with its Dilithium and SPHINCS+ code), a `README.txt` | yes | yes | yes |
| `licenses/`: zlib, zstd, GNU OpenMP (GPL-3 with the GCC Runtime Library Exception), `GPL-3.txt` | no (none of these is shipped) | yes | yes |
| `btx-modeld.BUILD-MANIFEST.json` | yes | no (the helper is not built by AInode) | no |
| `llama/` | llama.cpp's Mac files and its `LICENSE` | llama.cpp's Linux files, its `LICENSE`, and a copy of `libgomp.so.1` from the BTX archive | llama.cpp's Windows files and its licences |
| `ainode-wsl`, `wsl/rootfs.tar.gz` | no | no | yes: AInode's own relay, and Canonical's Ubuntu Base 24.04.5 |

SQLite is in the public domain and has no licence text. Where each licence
text comes from, and its sha256: `app/scripts/licenses/PROVENANCE.md`. The
staging scripts stop if a text is missing or changed. Staged and checked on
the Mac; the Linux and Windows staging were not run (no Linux here).

## Rust and JavaScript dependencies

The app's own dependency graphs resolve to permissive licences. Counted on 24
November 2026: the 483 crates in `app/src-tauri/Cargo.lock` (every platform and
feature) are under MIT, Apache-2.0 (some with the LLVM exception),
BSD-3-Clause, ISC, Zlib, Unicode-3.0, the
Unlicense, 0BSD, CC0-1.0 or CDLA-Permissive-2.0, alone or as a choice, and
five are MPL-2.0. The 110 packages in `app/package-lock.json` are under MIT,
Apache-2.0, ISC or BSD-3-Clause. No crate or package is under GPL, AGPL or
SSPL only.

The MPL-2.0 crates are file-level copyleft. AInode depends on them without
modification, which places no condition on this project's licence. Run
`cargo tree` in `app/src-tauri` or `npm ls` in `app` for the current list.

This is about the app's code only. The engine folder ships GNU OpenMP on
Linux and Windows (GPL-3 with the GCC Runtime Library Exception), as the table
above says.

Models the app offers, fetched over BTX, never bundled:

| Model | Source | Licence |
|---|---|---|
| IBM Granite 4.2 3B, Q4_K_M | `ibm-granite/granite-4.2-3b-GGUF` at `c40945d7` | Apache-2.0 |
| IBM Granite 4.0 350M, Q4_K_M (not offered since 0.4; kept on the host) | `ibm-granite/granite-4.0-350m-GGUF` at `b8208a86` | Apache-2.0 |
| Qwen3 8B, Q4_K_M | `Qwen/Qwen3-8B-GGUF` at `7c41481f` | Apache-2.0 |
| OpenAI gpt-oss-20b, MXFP4 | `ggml-org/gpt-oss-20b-GGUF` at `ef9b12f2` | Apache-2.0 |

Details and hashes: `docs/model-licence.md`. AInode is not affiliated with or
endorsed by IBM, OpenAI, Alibaba, Mozilla, the BTX developers, or any project above.
