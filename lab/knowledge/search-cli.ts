// lab/replay-chat.py runs this to get exactly the notes the app would add:
//   node lab/knowledge/search-cli.ts <wiki.json> <btx-docs.json>
// Reads one JSON line per message: {"q": latest, "earlier": [earlier user messages]}.
import { readFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { buildIndex } from "../../app/src/knowledge/search.ts";
import { lookUp, parseLibrary, type Library } from "../../app/src/knowledge/notes.ts";

const btx = JSON.parse(readFileSync(process.argv[3], "utf8"));
const own = parseLibrary(readFileSync(new URL("../../app/src/knowledge/library.md", import.meta.url), "utf8"));
const pq = JSON.parse(readFileSync(new URL("../../app/src/knowledge/pq-wiki.json", import.meta.url), "utf8"));
const lib: Library = { wiki: buildIndex(JSON.parse(readFileSync(process.argv[2], "utf8"))), btx: buildIndex(btx.docs), btxTag: btx.tag, own, pq: buildIndex(pq.docs) };
for await (const line of createInterface({ input: process.stdin })) {
  const { q, earlier } = JSON.parse(line);
  console.log(JSON.stringify(lookUp(lib, q, earlier ?? [])));
}
