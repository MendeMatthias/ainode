## What changed, and why

<!-- Plain words. One thing per pull request. -->

## What I ran

<!-- The commands and what they printed, and on which platform. "Not run on Windows" is a fine sentence. -->

- [ ] My commits are signed off (`git commit -s`, CONTRIBUTING.md).
- [ ] `npx tsc --noEmit`
- [ ] `npm test`
- [ ] `cargo test --lib`
- Platform(s):

## Checks

- [ ] It adds no wallet or payment code, and nothing that makes AInode listen on the network or share.
- [ ] If it changes what the app does on the network, what it stores or what it promises: `README.md` and `SECURITY.md` are changed with it.
- [ ] If it changes how the app talks to the engine: `app/ENGINE-CONTRACT.md` is changed with it.
- [ ] If it deviates from upstream BTX's ModelNet: the reason is stated, here and in `docs/DIRECTION.md`.
- [ ] If it changes the chat instruction, a knowledge pack or the note search: the `lab/replay-chat.py` results before and after are below.

<!-- Not a vulnerability report? Good. A vulnerability goes to "Report a vulnerability" on the Security tab, never into a pull request. -->
