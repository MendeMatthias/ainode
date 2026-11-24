# What every model in AInode knows

| File | What it is | Licence |
|---|---|---|
| `instruction.md` | The instruction every chat starts with: who the model is, `{knowledge}`, how to answer | MIT, like the app |
| `btx.md` | What BTX is and where the model runs, each fact sourced to the btxchain/btx repo | MIT, like the app |
| `library.md` | AInode's own library: what Mende wants every model to know and stand for (wellbeing and breathing with safety, crisis help, spirituality as the traditions teach it, faiths, wise minds, Bitcoin and crypto, self-custody, formulas, Mende and the Bonuz ecosystem from public facts). Each section has trigger words | MIT, like the app |
| `pq-wiki.json` | postquantum.wiki, Mende's post-quantum reference (btx-apps `apps/wiki/content`), 155 articles in 970 passages | **All rights reserved** (the source is unlicensed); included with Mende's permission (2026-10-12). Not covered by the app's MIT licence |
| `worker.ts` | Loads and indexes everything in a background thread (the encyclopedia is ~58 MB), so the window never freezes | MIT, like the app |
| `btx-docs.json` | BTX's own documentation at v0.34.16 (commit 074dac16, the engine AInode ships) in 815 passages: README, HUMANS.md, every 0.34 release note (0.34.16's is at `doc/release-notes-0.34.16.md`), the design essays (the plan), the model-network docs | MIT, the BTX developers |
| `wiki.json` | The offline encyclopedia: whole opening paragraphs of Wikipedia's Vital Articles, Levels 4 and 5 (46,941 articles, 58 MB; three political lists of Level 5 left out on Mende's word) | **CC BY-SA 4.0**, Wikipedia contributors |
| `wiki-attribution.json` | Every article title and the revision fetched, for that licence | CC BY-SA 4.0 |
| `search.ts` | Finds the one paragraph that answers a question, or none | MIT, like the app |
| `own.ts` | The user's own files (0.7.0): which files it reads, how PDF pages, Markdown sections and text are split into passages; searched first (`notes.ts` `fromMine`). The text is kept by `src-tauri/src/own.rs` under `$DATA/knowledge/`, never the file, never anywhere else | MIT, like the app |
| `pdf.ts` | A PDF's text, page by page, with pdf.js (Apache-2.0), bundled and loaded only when a PDF is added | MIT, like the app |
| `notes.ts` | Decides where to look: BTX's docs when the conversation is about BTX, else the encyclopedia, else nowhere | MIT, like the app |

The encyclopedia never leaves the Mac and nothing is looked up online: the
chat adds at most one paragraph, and only when one article clearly answers the
question (`search.ts` explains the rules and the mistakes that set them).

Rebuild the encyclopedia:

```bash
python3 lab/knowledge/fetch-vital.py /tmp/vital && python3 lab/knowledge/fetch-vital.py /tmp/vital 5
python3 lab/knowledge/build-pack.py /tmp/vital/vital4.json /tmp/vital/vital5.json app/src/knowledge
python3 lab/knowledge/build-pq-wiki.py ~/repos/btx-apps app/src/knowledge
python3 lab/knowledge/build-btx-docs.py ~/repos/btx app/src/knowledge v0.34.16   # the BTX release to describe; read with git show, nothing checked out
```

Measure any change to these files with `lab/replay-chat.py` (`refusals` must
stay at 0; compare `quiz`, `reason` and the conversations with `app` and
`app+kb`).
