// What AInode looks up for a message, on this Mac: BTX's own documentation when the
// conversation is about BTX, else the offline encyclopedia, else nothing. The chat
// and lab/knowledge/search-cli.ts (for lab/replay-chat.py) both call this.
import { notesFor, search, tokens, WIKI_HEADER, type Hit, type Index } from "./search.ts";
import { ABOUT_THE_FILE, fileOf } from "./own.ts";
import { THIS } from "../platform.ts";

/** Words that make a message about BTX. Mende asked the model to know BTX's
 *  technical side, its latest updates and its plan (2026-10-12). */
export const BTX_WORDS =
  /\b(btx|bitcoin|blockchain|matmul|mining|miners?|halving|block rewards?|modelnet|model network|btx-modeld|bount(?:y|ies)|release campaigns?|pay with compute|ml-dsa|post-quantum|shielded|dandelion|exactreplay|agentic state|hosting|host a model|0\.34(?:\.\d+)?)\b/i;

/** Words that make a message about quantum computing or post-quantum cryptography:
 *  postquantum.wiki, Mende's reference, answers those (pq-wiki.json). */
export const QUANTUM_WORDS =
  /\b(quantum|qubits?|post.?quantum|pqc|ml-?kem|ml-?dsa|slh-?dsa|kyber|dilithium|sphincs|falcon|lattice|shor'?s?|grover'?s?|harvest now|q-?day|crqc|superposition|entangle\w*|decoherence|fips ?20[345]|ecdsa|schnorr|cryptograph\w*|encryption)\b/i;

/** About quantum: the message names it, or one of the user's last two did. */
export function aboutQuantum(latest: string, earlier: string[]): boolean {
  return QUANTUM_WORDS.test(latest) || earlier.slice(-2).some((m) => QUANTUM_WORDS.test(m));
}

/** About BTX: the message names it, or one of the user's last two did ("and what's
 *  the great plan?" after a BTX question). */
export function aboutBtx(latest: string, earlier: string[]): boolean {
  return BTX_WORDS.test(latest) || earlier.slice(-2).some((m) => BTX_WORDS.test(m));
}

/** One passage of AInode's own library (library.md) and the words that call it. */
export interface Passage { t: string; x: string; keys: RegExp }

/** library.md: each "## " section a passage, its `keys` comment the words that call it. */
export function parseLibrary(md: string): Passage[] {
  return md.split(/^## /m).slice(1).map((sec) => {
    const [head, ...rest] = sec.split("\n");
    const body = rest.join("\n");
    const keys = /<!--\s*keys:\s*([\s\S]*?)\s*-->/.exec(body)?.[1] ?? "(?!)";
    return { t: head.trim(), x: body.replace(/<!--[\s\S]*?-->/g, "").trim(), keys: new RegExp(keys, "i") };
  });
}

/** The library passages a message calls, crisis first (it is listed first), at most two. */
export function fromLibrary(lib: Passage[], latest: string): Passage[] {
  return lib.filter((p) => p.keys.test(latest)).slice(0, 2);
}

export interface Library {
  wiki: Index | null; btx: Index | null; btxTag: string; own?: Passage[]; pq?: Index | null;
  /** The user's own files (knowledge/own.ts), searched first. */
  mine?: Index | null;
}
export interface Looked { notes: string; source: string; titles: string[] }

export const MINE_HEADER = `[From the user's own files, which they added to AInode on ${THIS}. Answer from them when they help, and say which file. Quote numbers and names exactly as written.]`;

/**
 * The user's own files: up to two passages that match the message. Their files
 * are small next to the encyclopedia, and BM25 scores in a small collection are
 * small too (a rare word in four passages weighs about 1.2, not 10), so the bar
 * is low (1.5, no lead needed); two words must still match, or one when the
 * message has only one. A message about "this file" with no matching words ("summarise
 * it") gets the opening passages of the file added last in this chat.
 */
export function fromMine(ix: Index, latest: string, recent?: string): Hit[] {
  const n = new Set(tokens(latest)).size;
  const hits = n ? search(ix, latest, 2, 1.5, 1, Math.min(2, n)) : [];
  if (hits.length || !recent || !ABOUT_THE_FILE.test(latest)) return hits;
  return ix.docs.filter((d) => fileOf(d.t) === recent).slice(0, 2).map((d) => ({ ...d, score: 0 }));
}

export const LIBRARY_HEADER = "[Notes from AInode's own library. Use them if they help; the user did not write them.]";

/** Passages from one source, before they become notes. */
interface Found { source: string; header: string; items: { t: string; x: string; label: string }[]; maxChars: number }

/**
 * At most three passages, in this order: the user's own files, AInode's own library (what Mende wants
 * every model to know and stand for; its questions are short), BTX's docs when
 * the conversation is about BTX, postquantum.wiki when it is about quantum, and
 * the encyclopedia when neither of those two answered.
 */
export function lookUp(lib: Library, latest: string, earlier: string[], recent?: string): Looked {
  const found: Found[] = [];
  const mine = lib.mine ? fromMine(lib.mine, latest, recent) : [];
  if (mine.length) {
    found.push({ source: "your files", header: MINE_HEADER, maxChars: 900, items: mine.map((h) => ({ t: h.t, x: h.x, label: fileOf(h.t) })) });
  }
  const own = lib.own ? fromLibrary(lib.own, latest) : [];
  if (own.length) found.push({ source: "AInode library", header: LIBRARY_HEADER, maxChars: 4000, items: own.map((p) => ({ t: p.t, x: p.x, label: p.t })) });
  if (lib.btx && aboutBtx(latest, earlier)) {
    const hits = topTwo(lib.btx, latest, ["btx"]);
    if (hits.length) found.push({ source: `BTX docs ${lib.btxTag}`, maxChars: 700, items: hits.map((h) => ({ t: h.t, x: h.x, label: short(h.t) })),
      header: `[BTX documentation notes from AInode (btxchain/btx ${lib.btxTag}). Use them if they help; the user did not write them.]` });
  }
  if (lib.pq && aboutQuantum(latest, earlier)) {
    const hits = topTwo(lib.pq, latest, ["quantum", "post"]);
    if (hits.length) found.push({ source: "postquantum.wiki", maxChars: 700, items: hits.map((h) => ({ t: h.t, x: h.x, label: short(h.t) })),
      header: "[Notes from postquantum.wiki, Mende's post-quantum reference, via AInode. Use them if they help; the user did not write them.]" });
  }
  // The encyclopedia unless BTX's docs or postquantum.wiki answered: a library hit
  // alone must not block it ("Which philosopher wrote Critique of Pure Reason?"
  // calls "Wise teachers", and still needs the article on Kant's book).
  if (!found.some((f) => f.source !== "AInode library") && lib.wiki) {
    const hits = search(lib.wiki, latest);
    if (hits.length) found.push({ source: "Wikipedia, offline", header: WIKI_HEADER, maxChars: 600, items: hits.map((h) => ({ t: h.t, x: h.x, label: h.t })) });
  }
  // Three passages at most, own library first.
  let left = 3;
  const kept = found.map((f) => {
    const items = f.items.slice(0, left);
    left -= items.length;
    return { ...f, items };
  }).filter((f) => f.items.length);
  return {
    notes: kept.map((f) => notesFor(f.items.map((i) => ({ ...i, score: 0 })), f.maxChars, f.header, latest)).join(""),
    source: kept.map((f) => f.source).join(" and "),
    titles: [...new Set(kept.flatMap((f) => f.items.map((i) => i.label)))],
  };
}

/**
 * BTX's docs and postquantum.wiki: every passage is about their subject, so two may
 * come together (no lead needed, unlike the encyclopedia's near misses). The
 * subject's own word ("btx", "quantum") is in nearly every passage, so it is not
 * counted: two other words must match, or small talk after a BTX question found
 * passages ("running" in "where are you running?"). A second passage only if
 * nearly as good: "latest updates" found the release list (14.1) and then
 * "Mirror policy" (6.9).
 */
function topTwo(ix: Index, latest: string, drop: string[]): Hit[] {
  return search(ix, latest, 2, 4.5, 1, 2, drop).filter((h, i, all) => i === 0 || h.score >= all[0].score * 0.6);
}

/** "HUMANS.md > What 0.34.7 is (the plan...)" as a chip label: the section, short. */
function short(t: string): string {
  const s = t.replace(/\s*\([^)]*\)$/, "").split(" > ").pop() ?? t;
  return s.length > 42 ? `${s.slice(0, 41)}…` : s;
}
