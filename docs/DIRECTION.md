# Direction

What AInode is, how it follows upstream BTX's ModelNet, and what it will not
become.

AInode is open, and it is meant as a baseline. We would like more developers
to work on it and more people to fork it. This page is here so that you know
where the project is going before you put work in, and where a fork may want
to go another way.

## What AInode is

BTX's model network (ModelNet) lets anyone fetch a public AI model as
content-addressed pieces from hosts that carry it, and check every piece.
Upstream's product is the node: one `btxd` owns the money and the model
network, and upstream's own desktop app is an interface to a running node.

AInode is the door for people without a node. It runs upstream's standalone
model helper (`btx-modeld`), fetches a model the user picked, checks it, and
runs it locally with llama.cpp. No account, no wallet, nothing to pay and
nothing to earn. Upstream's documents call these shapes an explorer and a
local runtime on a standalone helper, and say that fetching a public model
needs no wallet and no balance.

## The rule: follow upstream

AInode follows upstream BTX's ModelNet. It uses what upstream built, the way
upstream documents it, and it runs upstream's engine with the engine's
sources untouched.

- A problem in the engine is fixed upstream, as a tested patch with what
  happens, where, what the change does and which tests ran. Six went that
  way and are in v0.34.16 (btxchain/btx#228 to #233).
- AInode builds something of its own only where upstream has no surface to
  extend.
- A deviation from upstream's default needs a stated reason, in the code or
  in the document that describes the behaviour. The current ones are listed
  below.

## Three lines AInode does not cross

1. **Not a node.** AInode runs no `btxd`, validates no chain and mines
   nothing.
2. **Not a wallet.** It holds no keys for money, signs no transaction and
   moves no funds. Where the network shows something that costs money, the
   app says what a node with a wallet would do and stops there.
3. **It never talks to hosts the user cannot see in it.** The engine is
   given the model host shown in the app and the hosts the user added. The
   one exception is the engine's own: during a download, and for up to 15
   minutes after it, the engine may contact other sources of that model the
   host named ([`SECURITY.md`](../SECURITY.md) says exactly what each host
   can see).

AInode is also never a list of its users' addresses. The count of open
AInodes on easybtx.com is a number, self-reported, and can be turned off.

## Where AInode deviates from upstream's default today, and why

| Upstream's default | AInode | Why |
|---|---|---|
| A client that holds a model shares it within limits its owner sets | Never listens, never shares | Sharing changes what the app promises its user. It waits for a design accepted first (see below) |
| The engine is given a chain name (`-pwcchain=main`) and records what hosts say about themselves | No chain name | With a chain name on both sides, the engine asks its host for other addresses at every 5-second visit and contacts what it is given. That would break line 3. Reported upstream as [btxchain/btx#946](https://github.com/btxchain/btx/issues/946); the flag stays out until upstream offers observation without it ([`app/ENGINE-CONTRACT.md`](../app/ENGINE-CONTRACT.md)) |
| Every known host is handed to the engine | One host at a time | A rule kept from the v0.34.15 engine, where two hosts for one model made downloads fail. The fix is in v0.34.16; handing over every host is a change of its own |
| On the Mac, upstream's own release file | Built from upstream's tag by a pinned script | Upstream publishes no Mac file since v0.34.16. When it does again, AInode goes back to upstream's bytes |

## What is deliberately not built

- **Sharing.** "Never listens on the network and never shares what it
  downloads" is a promise in the README. Turning a download into a source
  needs governors a desktop must have (battery, metered lines, memory), a
  way for a computer behind a home router to be reached, and the user's
  explicit word. Upstream names some of these as still missing on its side.
  This is the largest open piece of work, and help is welcome: start with an
  issue that proposes a design, so the promise and the code change together.
- **A wallet, payments, funding.** Line 2. Funding in ModelNet happens in
  the node's wallet, the way upstream wants.
- **A published list of hosts or of users.** Upstream says it will not add a
  model-plane seed list or a published host list. AInode does not add one of
  its own.
- **Swarm work, idle compute, settlement.** Upstream describes these for
  later releases and has not enabled them. AInode waits for upstream.

## What comes next, in upstream's order

Done so far: the v0.34.16 engine on the Mac (0.9.0), and the BTX ModelNet
view, which shows what this computer's engine knows and says it is one
computer's view, not a census (0.9.0 and 0.9.1).

1. Windows and Linux on the v0.34.16 engine.
2. "Use my BTX node": when a node runs on the same computer, AInode uses it.
   Sharing, confirmed funding numbers and model-family membership arrive
   through the node, the way upstream wants.
3. Sharing without a node, on the home network first, when the conditions
   above are met.

Each step that changes what the app promises its user changes the README and
`SECURITY.md` in the same pull request.

## How to take part

- Work on what is wanted now: the list is in the
  [README](../README.md#fork-it-build-on-it) and in
  [CONTRIBUTING.md](../CONTRIBUTING.md).
- Propose something larger: open an issue with the idea and how it follows
  upstream. The three lines above are the only fixed points.
- Go another way: fork it. [FORKING.md](../FORKING.md) lists what to change.
  A fork that crosses one of the three lines is a different app, and that is
  fine: the licence is there for it.
