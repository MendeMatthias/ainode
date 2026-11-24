// The offline library in a background thread. Loading and indexing ~58 MB of
// encyclopedia takes seconds (2.98 s measured for the index alone); on the page's
// own thread that would freeze the window. The chat warms this up when it opens
// and then only sends questions: {id, q, earlier} -> {id, looked}.
/// <reference lib="webworker" />
import libraryText from "./library.md?raw";
import btxUrl from "./btx-docs.json?url";
import pqUrl from "./pq-wiki.json?url";
import wikiUrl from "./wiki.json?url";
import { lookUp, parseLibrary, type Library } from "./notes";
import { buildIndex, type Doc, type Index } from "./search";

const json = (url: string) => fetch(url).then((r) => r.json()).catch(() => null);

let library: Promise<Library> | null = null;
function load(): Promise<Library> {
  library ??= Promise.all([json(wikiUrl), json(btxUrl), json(pqUrl)]).then(([wiki, btx, pq]) => ({
    wiki: wiki ? buildIndex(wiki as Doc[]) : null,
    btx: btx ? buildIndex(btx.docs as Doc[]) : null,
    btxTag: (btx?.tag as string) ?? "",
    pq: pq ? buildIndex(pq.docs as Doc[]) : null,
    own: parseLibrary(libraryText),
  }));
  return library;
}

// The user's own files (knowledge/own.ts): replaced whole whenever the chat
// sends them (on opening, after an add or a remove). Small next to the
// encyclopedia, so rebuilding the index is quick.
let mine: Index | null = null;

self.onmessage = async (e: MessageEvent<{ id: number; q?: string; earlier?: string[]; recent?: string; mine?: Doc[] }>) => {
  const { id, q, earlier, recent } = e.data;
  if (e.data.mine) mine = e.data.mine.length ? buildIndex(e.data.mine) : null;
  const lib = await load();
  self.postMessage({ id, looked: q === undefined ? null : lookUp({ ...lib, mine }, q, earlier ?? [], recent) });
};
