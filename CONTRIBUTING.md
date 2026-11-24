# Contributing to AInode

Thank you for looking. We would like more people to work on this. The most
useful first thing you can do is run it and tell us what went wrong on
hardware we do not own.

[docs/DIRECTION.md](docs/DIRECTION.md) says what the app is, the three lines
it does not cross, and what is deliberately not built yet. It is short, and
reading it first saves you work. If you would rather take the code somewhere
else, [FORKING.md](FORKING.md) is for you: forks are welcome.

## What is most wanted right now

- **Windows and Linux on real machines.** Both are built and released, and
  [docs/platforms.md](docs/platforms.md) lists what was never tried: a real
  GPU through Vulkan, Windows 10, a PC without WSL, most Linux distributions.
  A report that says "it works on X" or "it said Y, my computer is actually
  Z" is a valuable issue (there is a form for it).
- **A public guide for running a model host.** Today's host was set up with
  the maintainer's own tooling, which is not in this repository. A clean guide
  that anyone can follow is open work.
- **More hosts and more models.** One public host serves four models. A model
  joins the catalog with its licence record
  ([docs/model-licence.md](docs/model-licence.md)).
- **Translations.** The interface is English only, and its text lives in the
  code. There is no translation layer yet. Building one is welcome.
- **Plain language.** If a screen confused you, that is a bug. Say so.
- **Fixes that belong upstream.** If the engine misbehaves, the fix goes to
  [btxchain/btx](https://github.com/btxchain/btx) as a tested patch, and
  AInode picks it up with upstream's next release.

## Sign your commits off

We use the Developer Certificate of Origin. Add `-s` to your commit:

```bash
git commit -s -m "your message"
```

That appends a `Signed-off-by` line, which is you saying you have the right to
submit the code under the MIT licence. There is no CLA and you keep your
copyright.

## Before you open a pull request

Run the tests. CI runs exactly these on your pull request, on macOS, Ubuntu
and Windows, and they pass on a fresh clone without staging the engine:

```bash
cd app
npm ci
npx tsc --noEmit
npm test                              # the interface suite, a few seconds
cd src-tauri
cargo test --no-run                   # every test target compiles
cargo test --lib                      # the unit tests, a few seconds
```

To see the interface without building anything native, `cd app && npm run dev`
serves it against a mock engine at http://localhost:1450/?mock=fresh. For the
full app, the README has the build for each platform.

Do not run a development build against the data folder or the socket of an
installed AInode. Use a scratch folder (`AINODE_DATA_DIR`, `AINODE_SOCKET`);
the dev bridge and the live tests refuse the installed app's own.

In the pull request, say:

- **what changed**, in plain words, and why;
- **what you ran**, and on which platform: the commands and what they
  printed. "Not run on Windows" is a fine sentence. An untested claim that
  reads like a tested one is not.

Please keep the diff to one subject. A pull request that fixes a bug and also
reformats a file is much harder to review than two pull requests. The Rust
code is not formatted with rustfmt: please do not reformat files you touch.

If your change alters what the app does on the network, what it stores, or
what it promises its user, change `README.md` and `SECURITY.md` in the same
pull request. If it alters how the app talks to the engine, change
[app/ENGINE-CONTRACT.md](app/ENGINE-CONTRACT.md) with it.

## The rule: follow upstream

AInode uses upstream BTX's ModelNet as upstream built and documents it.

- A bug in the engine goes upstream as a tested patch. AInode does not carry
  a patched engine, and the helper's sources are never changed, on any
  platform.
- A deviation from upstream's default behaviour needs a stated reason, in the
  code and in `docs/DIRECTION.md`. "It was easier" is not one.

The three lines: AInode is **not a node**, **not a wallet**, and **never
talks to hosts the user cannot see in it**.

## Things that will be sent back

**Anything touching the trust anchors.** `pubkey` and `endpoints` in
`app/src-tauri/tauri.conf.json` decide which packages an installed app will
accept and where it looks for them. The engine pins in
`app/scripts/stage-engine*.sh` and `build-engine-macos.sh` decide which
`btx-modeld` and llama.cpp every user runs. `hosts.json` and `catalog.json`
decide which host every user's computer contacts and which models it offers.
These are protected by CODEOWNERS. A pull request that changes an engine pin
says where the new hash was measured and on which toolchain, and changes
nothing else.

**Wallet or payment code** of any kind: keys for money, signing, sending,
funding, payment links the app opens by itself.

**Anything that makes AInode listen on the network or share** what it
downloaded, before a design for it has been accepted in an issue. That
includes passing the engine `-modelbind`, `-modelhost`, `-modelseed=auto`,
`-modelfollowpeers=1` or a chain name.

**Changes to what the chat models are told, without measurements.** The
instruction, the knowledge packs and the search that picks a note are measured
with `lab/replay-chat.py`. Put the results before and after in the pull
request ([app/src/knowledge/README.md](app/src/knowledge/README.md)).

**A new model in the catalog without its licence record**
([docs/model-licence.md](docs/model-licence.md)): only open licences with no
field-of-use limit, from the publisher's own repository, pinned by commit and
sha256.

**Telemetry.** The one thing the app reports is the check-in described in the
README, and the user can turn it off.

**Anything that promises a user a reward.** Hosting and sharing are never
described as earning. They are not.

## Who merges

Only the maintainer, [@MendeMatthias](https://github.com/MendeMatthias).
Everyone else, regular contributors too, works through pull requests from a
fork. A pull request is merged when CI passes on all three platforms and the
maintainer has reviewed it.

## Security problems

Do not open a public issue. See [SECURITY.md](SECURITY.md).

## Where the engine itself is developed

`btx-modeld` is upstream at [btxchain/btx](https://github.com/btxchain/btx).
How the model network behaves, what a host serves, and anything about the
chain belongs there, not here. AInode runs that engine; it does not modify it.

## A note on the name

The MIT licence covers the code. It does not grant the AInode or easyBTX name
or logo. Forks are welcome and should ship under a different name, so that a
user can always tell whose build they are running.
[FORKING.md](FORKING.md) lists what a fork must change so it does not collide
with this app on a user's computer.

---

## For the maintainer: repository settings

These are set by hand on GitHub. Nothing in the tree can set them. They are
the settings of [easyNode](https://github.com/MendeMatthias/easynode), read
from it on 2026-11-24, with AInode's names.

**About**

| | |
|---|---|
| Description | Open AI models over BTX, running on your computer. No wallet, no account. MIT. Built on upstream BTX's ModelNet. |
| Website | https://easybtx.com/ai |
| Topics | `ai`, `btx`, `desktop-app`, `llama-cpp`, `local-ai`, `open-source`, `peer-to-peer`, `post-quantum`, `rust`, `tauri` |

**Branch protection, `main`**

| Setting | Value |
|---|---|
| Require a pull request before merging | on, 1 approving review |
| Require review from Code Owners | on |
| Dismiss stale approvals when new commits are pushed | on |
| Require status checks to pass | `test (macos-latest)`, `test (ubuntu-24.04)`, `test (windows-latest)` |
| Require branches to be up to date before merging | off on easyNode; your choice |
| Require conversation resolution | on |
| Allow force pushes | off |
| Allow deletions | off |
| Do not allow bypassing the above settings (enforce for admins) | off |

`no private key material` (Secret scan) and the two Style jobs run on every
pull request too. easyNode does not list its own as required; they can be
added to the required checks once they have run green here.

What this means for a single maintainer:

- **A pull request from someone else:** you review it, approve it as code
  owner, and merge it. Nobody else can: nobody else has write access.
- **Your own pull request:** GitHub does not let an author approve their own
  pull request, so the "1 approving review" can never be met for it. Because
  the rules are not enforced for admins, you merge it with the admin bypass
  ("merge without waiting for requirements"). The bypass also skips the
  required checks, so look at CI yourself before you use it.
- **Dependabot's pull requests** are not yours, so you approve and merge them
  like anyone else's.

**The rest**

| Where | Setting |
|---|---|
| Collaborators | Only you, as admin. Nobody else with Write, Maintain or Admin |
| Actions, workflow permissions | Read repository contents. "Allow GitHub Actions to create and approve pull requests" off |
| Actions, fork pull requests | Require approval before a workflow runs for first-time contributors (or for all outside collaborators) |
| Actions, allowed actions | The workflows use `actions/checkout`, `actions/setup-node` and `Swatinem/rust-cache`, each pinned by commit |
| Secrets and environments | None. CI needs none. The signing keys and the updater key never go into this repository |
| Code security | Private vulnerability reporting on. Dependabot alerts on. Secret scanning and push protection on |
| Releases | They stay on `MendeMatthias/ainode-releases`: the app's updater reads its feed from there |
