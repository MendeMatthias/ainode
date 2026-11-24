# Model licence record

Checked 2026-10-11 against primary sources only: the Hugging Face API, the
model cards, and the GGUF header's own metadata. The first two entries were
written before any weights were fetched; the later ones as each model was
offered.

## Primary: IBM Granite 4.2 3B, Q4_K_M

| | |
|---|---|
| Repo | `ibm-granite/granite-4.2-3b-GGUF` at commit `c40945d71cd90f249a56985e8155551a9188dc30` |
| File | `granite-4.2-3b-Q4_K_M.gguf` |
| Size | 2,244,011,552 bytes (2.09 GiB) |
| SHA-256 | `e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5` |
| Licence | Apache-2.0 (SPDX `Apache-2.0`) |
| Gated | No |
| Released | IBM, August 2026 (repo created 2026-08-12, last modified 2026-09-02) |
| Signed | `model.sig` in the repo is a Sigstore bundle from IBM covering this file |

**Why Apache-2.0 permits this.** Section 2 grants a perpetual, worldwide,
royalty-free copyright licence to "reproduce ... and distribute" the Work.
Section 4 lets anyone redistribute copies in any medium provided they meet four
conditions. There is no field-of-use clause, no user cap, no acceptable-use
policy and no click-through. Four sources agree on the licence: the card's
`license: apache-2.0` field (verified via the HF API), the base card's licence
row linking apache.org (`ibm-granite/granite-4.2-3b` README line 33), the card's
statement that Granite is "released under the Apache 2.0 license, enabling
unrestricted commercial and academic use" (line 43), and the GGUF header's own
`general.license = apache-2.0`.

**What we must do as a redistributor (Apache-2.0 §4).**

- §4(a): give recipients a copy of the licence. **Neither IBM repo contains a
  LICENSE file**, so we ship the canonical Apache-2.0 text ourselves, as
  `LICENSE` beside the GGUF in the hosted package. The helper keeps a file named
  LICENSE with a licence role (`src/modelnet/catalog.cpp:43`).
- §4(b): mark modified files. We host IBM's bytes unchanged, so there is
  nothing to mark. If we ever re-quantise, that changes.
- §4(c): keep attribution. One line in the package and on any page that links
  it: "Granite-4.2-3B (Q4_K_M), IBM, Apache-2.0,
  huggingface.co/ibm-granite/granite-4.2-3b-GGUF@c40945d".
- §4(d): carry a NOTICE file if one exists. None exists.
- §6: no trademark grant. Never imply IBM endorses BTX, easyNode or this host.

**Do not package `granite_thinking_parser.py`.** IBM ships it in the same repo;
the BTX importer refuses `.py` (`src/modelnet/qualification.cpp:52-58`).

**Content-addressed identity, and why the file name matters.** A BTX model id
hashes each file's relative path, role, size, SHA-384 and piece root
(`src/modelnet/cores.cpp:65-85`). So a second operator who imports the same
bytes under the **same file names** gets the same `btx://` link and seeds the
same model. Renaming the GGUF or adding a sidecar file changes the link. The
runbook therefore fixes the package layout exactly: `granite-4.2-3b-Q4_K_M.gguf`
plus `LICENSE`, nothing else.

## Smoke test: IBM Granite 4.0 350M, Q4_K_M

| | |
|---|---|
| Repo | `ibm-granite/granite-4.0-350m-GGUF` at `b8208a86a58427e1739265318028eb5895b74bf2` |
| File | `granite-4.0-350m-Q4_K_M.gguf`, 236,985,760 bytes |
| SHA-256 | `771c588a49607f274a2bba3185733607ebe6f74b996ab90e2d6bee0d98bcec52` |
| Licence | Apache-2.0, not gated, no LICENSE file in repo (same obligations as above) |

Used only to prove the path end to end in minutes before moving 2 GiB.

## Qwen3 8B (offered from 0.7.0)

| | |
|---|---|
| Repo | `Qwen/Qwen3-8B-GGUF` at `7c41481f57cb95916b40956ab2f0b139b296d974` (official, Alibaba's Qwen team) |
| File | `Qwen3-8B-Q4_K_M.gguf`, 5,027,783,488 bytes |
| SHA-256 | `d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785` (equal to the repo's LFS hash) |
| Licence | Apache-2.0 (model card and the repo's own LICENSE file), not gated. The BTX package carries the Apache-2.0 text as `LICENSE`, like the other models |

Measured before it was offered.

## Rejected, and why

| Candidate | Reason |
|---|---|
| Llama family | Llama community licence: conditions and a user cap, not Apache/MIT |
| Gemma family | Gemma terms of use: use restrictions |
| Phi-4-mini | MIT, but no official GGUF; SafeTensors only, 7.67 GB |
| Mistral 7B v0.3 | Apache-2.0, but no official GGUF, 14.5 GB, May 2024 |
| Qwen3-4B | Apache-2.0 and the only GGUF repo that carries its own LICENSE file. A fine second model. Not first because its GGUF header names it "Qwen3 4B Instruct Awq", which needs explaining |
| Qwen3.5 small models | Apache-2.0, but no official GGUF, and the SafeTensors architecture is not on BTX's generate allowlist (`src/modelnet/generate.cpp:48-75`) |
| `granite-4.0-h-tiny` (the upstream author's own test model) | SafeTensors directory of 13.9 GB that llama.cpp cannot open (`doc/modelnet/howto.md:249`); reusing its link would need byte-identical files |

## Not checked

- The minimum llama.cpp build that runs Granite 4.2. Its architecture
  (`granite`) has been in llama.cpp for a long time, but we have not run it.
- The Sigstore signature itself (not verified locally yet; planned at download).

## OpenAI gpt-oss-20b, MXFP4 (offered)

Checked 2026-10-12. Chosen by measurement over Granite 4.2 8B and Ministral 3
14B.

| | |
|---|---|
| Weights | `openai/gpt-oss-20b` at `6cee5e81ee83917806bbde320786a8fb61efebee` (OpenAI, August 2025) |
| File | `ggml-org/gpt-oss-20b-GGUF` at `ef9b12f2ff56c69cf32153a02784e7a3c88bf524`: `gpt-oss-20b-MXFP4.gguf`, the llama.cpp project's conversion, weights in OpenAI's native MXFP4 |
| Size | 12,109,566,624 bytes |
| SHA-256 | `27cd6c432c7672cb812a92f611cf3ba7bbc35928262bb1e1253ff4ee6ae35901` (matched the repo's LFS record after download) |
| Licence | Apache-2.0 on both repos (HF API `license: apache-2.0`); `openai/gpt-oss-20b` ships a LICENSE file with the Apache-2.0 text |
| Gated | No |
| Usage policy | `USAGE_POLICY` in the OpenAI repo, one sentence: by using the model "you agree to comply with all applicable law". Not a field-of-use restriction |
| Link | `btx://pqcjx57rargtrp5us2cgvfdttffzxrfnwfwyrd3m63u9n8wtjqpvs2zf326d025uwrya6wy8tx69mxuxfntv9`, computed by importing the two-file package (this GGUF + the same apache.org `LICENSE` as the Granite packages) into the app's own btx-modeld v0.34.15 on this Mac, nothing published |

Apache-2.0 §4 duties are as for Granite: ship the licence text (the package's
`LICENSE`), keep attribution ("gpt-oss-20b, OpenAI, Apache-2.0, via
huggingface.co/ggml-org/gpt-oss-20b-GGUF@ef9b12f"), no NOTICE file exists, no
trademark use. AInode is not affiliated with or endorsed by OpenAI.
