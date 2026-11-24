// The user's own files, read on this Mac and split into passages the chat can
// search like BTX's docs (notes.ts). Pure: no DOM, no pdf.js (pdf.ts reads PDFs),
// so it is unit-tested and the lab can run it under node.
import type { Doc } from "./search.ts";

export type OwnKind = "pdf" | "md" | "txt";

/** What AInode can read, by extension. */
export function kindOf(name: string): OwnKind | null {
  const ext = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase();
  if (ext === "pdf") return "pdf";
  if (ext === "md" || ext === "markdown" || ext === "mdx") return "md";
  if (ext === "txt" || ext === "text") return "txt";
  return null;
}

/** About a page of text: long enough to answer from, short enough that the
 *  matching sentences fit the notes (bestWindow keeps the best ones). */
export const PASSAGE_CHARS = 1200;

/** Paragraphs packed into passages of at most `max` characters; a paragraph
 *  longer than that is cut at sentence ends, then hard. */
export function pack(text: string, max = PASSAGE_CHARS): string[] {
  const paras = text.split(/\n\s*\n/).map((p) => p.replace(/[ \t]+/g, " ").replace(/\s*\n\s*/g, " ").trim()).filter(Boolean);
  const out: string[] = [];
  let cur = "";
  const flush = () => { if (cur) out.push(cur); cur = ""; };
  for (const p of paras) {
    for (const piece of p.length <= max ? [p] : cut(p, max)) {
      if (cur && cur.length + 2 + piece.length > max) flush();
      cur = cur ? `${cur}\n\n${piece}` : piece;
    }
  }
  flush();
  return out;
}

function cut(p: string, max: number): string[] {
  const out: string[] = [];
  let rest = p;
  while (rest.length > max) {
    // The last sentence end inside the limit, else the last space, else hard.
    let at = -1;
    for (let i = max - 1; i > max / 2; i--) if (".!?".includes(rest[i]) && rest[i + 1] === " ") { at = i + 1; break; }
    if (at < 0) at = rest.lastIndexOf(" ", max);
    if (at <= 0) at = max;
    out.push(rest.slice(0, at).trim());
    rest = rest.slice(at).trim();
  }
  if (rest) out.push(rest);
  return out;
}

/** Markdown: one section per heading, titled "name > heading", each packed. */
export function splitMarkdown(name: string, text: string): Doc[] {
  const out: Doc[] = [];
  let head = "";
  let body: string[] = [];
  let fence = false;
  const flush = () => {
    for (const x of pack(body.join("\n"))) out.push({ t: head ? `${name} > ${head}` : name, x });
    body = [];
  };
  for (const line of text.replace(/\r\n?/g, "\n").split("\n")) {
    if (/^\s*(```|~~~)/.test(line)) fence = !fence;
    const h = fence ? null : /^#{1,6}\s+(.+?)\s*#*\s*$/.exec(line);
    if (h) { flush(); head = h[1].slice(0, 120); continue; }
    body.push(line);
  }
  flush();
  return out;
}

export function splitText(name: string, text: string): Doc[] {
  return pack(text.replace(/\r\n?/g, "\n")).map((x) => ({ t: name, x }));
}

/** A PDF's pages (their text, in order): passages titled "name, page N". */
export function splitPdf(name: string, pages: string[]): Doc[] {
  const out: Doc[] = [];
  pages.forEach((text, i) => {
    for (const x of pack(text)) out.push({ t: `${name}, page ${i + 1}`, x });
  });
  return out;
}

/**
 * A PDF with almost no text is a scan (pictures of pages): say so rather than
 * add an empty file. Under 40 characters a page on average, or none at all.
 */
export function looksScanned(pages: string[]): boolean {
  const chars = pages.reduce((a, p) => a + p.replace(/\s+/g, "").length, 0);
  return pages.length === 0 || chars < 40 * pages.length;
}

/** Which file a passage came from: its title up to " > " or ", page ". */
export function fileOf(title: string): string {
  const a = title.indexOf(" > ");
  const b = title.lastIndexOf(", page ");
  const cuts = [a, b].filter((i) => i > 0);
  return cuts.length ? title.slice(0, Math.min(...cuts)) : title;
}

/** A question about "this file" or the document in general, not about words in it. */
export const ABOUT_THE_FILE = /\b(summari[sz]e|summary|tl;?dr|this (file|doc(ument)?|pdf|paper|text|book|article)|the (file|doc(ument)?|pdf|paper|text|book|article)|what does it say|key points|main points)\b/i;
