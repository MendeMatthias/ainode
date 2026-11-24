// Offline encyclopedia search: BM25 over the opening paragraphs of Wikipedia's
// Vital Articles (lab/knowledge/fetch-vital.py builds the data). Pure: no DOM,
// no app imports, so lab/replay-chat.py runs this same file under node to
// measure what the app does.

export interface Doc { t: string; x: string }
export interface Hit { t: string; x: string; score: number }

const STOP = new Set(
  ("a an and are as at be by did do does for from had has have how i in is it its me my of on or " +
    "that the their them they this to was were what when where which who whom whose why will with you " +
    "your can could would should tell about into than then there these those also just like please").split(" "),
);

export function tokens(s: string): string[] {
  return s
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((w) => w && !STOP.has(w));
}

export interface Index {
  docs: Doc[];
  /** term -> [doc, term frequency, doc, term frequency, ...]: flat and typed, so
   *  ~47,000 articles take tens of MB, not the ~200 MB of one array per pair. */
  post: Map<string, Int32Array>;
  len: Float64Array;
  avg: number;
  titles: string[]; // normalized titles, for the phrase boost
}

const TITLE_WEIGHT = 3; // a title word counts as three body words
const MIN_DOCS = 8; // see search(): the smallest collection IDF is computed for

export function buildIndex(docs: Doc[]): Index {
  const grow = new Map<string, number[]>(); // packed small-integer arrays while building
  const len = new Float64Array(docs.length);
  docs.forEach((d, i) => {
    const tf = new Map<string, number>();
    const title = tokens(d.t);
    for (const w of title) tf.set(w, (tf.get(w) ?? 0) + TITLE_WEIGHT);
    const body = tokens(d.x);
    for (const w of body) tf.set(w, (tf.get(w) ?? 0) + 1);
    len[i] = title.length * TITLE_WEIGHT + body.length;
    for (const [w, n] of tf) {
      let list = grow.get(w);
      if (!list) grow.set(w, (list = []));
      list.push(i, n);
    }
  });
  const post = new Map<string, Int32Array>();
  for (const [w, list] of grow) post.set(w, Int32Array.from(list));
  const avg = docs.length ? len.reduce((a, b) => a + b, 0) / docs.length : 1;
  return { docs, post, len, avg, titles: docs.map((d) => tokens(d.t).join(" ")) };
}

/**
 * The paragraphs most likely to answer `query`, best first. Tuned on the real
 * pack against Mende's chats and the lab quiz (lab/replay-chat.py):
 * - a paragraph must match two different words of the message: one word alone
 *   is how small talk found "Reading" for "are you reading?", "Hello Kitty"
 *   for "hello" and "Running" for "where are you running?";
 * - a question that names an article ("Who wrote War and Peace?", "the
 *   capital of Kazakhstan") gets that article first;
 * - by default only the best paragraph, and only when it leads the next by
 *   `margin`. Near ties mean the search is guessing, and a near-miss note is
 *   worse than none: with three notes the 3B answered "smallest country" with
 *   Nauru (the third smallest), "first woman to win a Nobel" with Selma
 *   Lagerlöf (first in Literature) and the 2016 Literature prize with another
 *   laureate. All three were near ties; every question the notes fixed
 *   (Astana, Bosch, Kurosawa) had a clear leader.
 */
export function search(ix: Index, query: string, k = 1, minScore = 15, margin = 1.2, minTerms = 2, drop: string[] = []): Hit[] {
  const q = [...new Set(tokens(query))].filter((w) => !drop.includes(w));
  if (q.length < minTerms) return [];
  // A word's weight (IDF) compares it with the rest of the collection, and a
  // handful of passages is no collection: in a one-page PDF every word is in
  // "every" passage and weighs log(1.33) = 0.29, so nothing could reach the
  // bar. Weigh as if among at least MIN_DOCS passages; every bundled library
  // is far bigger, so only the user's own small files score differently.
  const N = Math.max(ix.docs.length, MIN_DOCS);
  const k1 = 1.2;
  const b = 0.75;
  const score = new Map<number, number>();
  const matched = new Map<number, number>();
  for (const w of q) {
    const list = ix.post.get(w);
    if (!list) continue;
    const df = list.length / 2;
    const idf = Math.log(1 + (N - df + 0.5) / (df + 0.5));
    for (let j = 0; j < list.length; j += 2) {
      const i = list[j];
      const tf = list[j + 1];
      const s = (idf * tf * (k1 + 1)) / (tf + k1 * (1 - b + (b * ix.len[i]) / ix.avg));
      score.set(i, (score.get(i) ?? 0) + s);
      matched.set(i, (matched.get(i) ?? 0) + 1);
    }
  }
  const qn = ` ${q.join(" ")} `;
  const named = new Set<number>();
  for (const [i, s] of score) {
    const t = ix.titles[i];
    if (t.length >= 4 && qn.includes(` ${t} `)) {
      score.set(i, s + 6);
      // Only a multi-word title is a name ("tale genji"); one word like
      // "country" is named by ordinary questions ("the smallest country").
      if (t.includes(" ")) named.add(i);
    }
  }
  const ranked = [...score].filter(([i]) => (matched.get(i) ?? 0) >= minTerms).sort((a, b) => b[1] - a[1]);
  // Hit j counts only if it leads hit j+1 by `margin`: the leaders, not a tie.
  const out: Hit[] = [];
  for (let j = 0; j < Math.min(k, ranked.length); j++) {
    const [i, s] = ranked[j];
    const next = ranked[j + 1]?.[1] ?? 0;
    // An article the question names ("Who wrote The Tale of Genji?") is trusted
    // without a lead: with Wikipedia's Level 5 its neighbours ("Hikaru Genji")
    // came close, the rule went quiet and the 3B invented an author.
    if (s < minScore || (s < next * margin && !(j === 0 && named.has(i)))) break;
    out.push({ t: ix.docs[i].t, x: ix.docs[i].x, score: Math.round(s * 10) / 10 });
  }
  return out;
}

export const WIKI_HEADER = "[Encyclopedia notes from AInode (Wikipedia). Use them if they help; the user did not write them.]";

/**
 * A long passage as the model sees it: its first sentence (it names the subject),
 * then the sentences that share the most words with the question, in their own
 * order, up to `maxChars`. The encyclopedia keeps whole opening paragraphs, so
 * "who won the 2016 Nobel in Literature" needs the sentence about 2016, not the
 * paragraph's start. With no question words to go by, it reads on in order.
 */
export function bestWindow(text: string, query: string, maxChars: number): string {
  if (text.length <= maxChars) return text;
  const sentences = splitSentences(text);
  const q = new Set(tokens(query));
  const score = sentences.map((x, i) => (i === 0 ? Infinity : tokens(x).filter((w) => q.has(w)).length));
  const keep = new Set<number>();
  let used = 0;
  const take = (i: number) => {
    const len = sentences[i].length + 1;
    if (used + len > maxChars) return false;
    keep.add(i);
    used += len;
    return true;
  };
  // The first sentence and the matching ones, best first; then, if room is left,
  // the rest in reading order until one no longer fits.
  for (const i of sentences.map((_, i) => i).filter((i) => score[i] > 0).sort((a, b) => score[b] - score[a] || a - b)) take(i);
  for (let i = 1; i < sentences.length; i++) if (!keep.has(i) && score[i] === 0 && !take(i)) break;
  if (!keep.size) return `${text.slice(0, maxChars - 1)}…`;
  return [...keep].sort((a, b) => a - b).map((i) => sentences[i]).join(" ");
}

/**
 * Sentences end at ".", "!" or "?" followed by a space, so "v0.34.15" and "3.14"
 * stay whole (a pattern split them into "0. 34. 15"). No regex lookbehind: the
 * WebKit of macOS 13.0 to 13.2 cannot parse it, and the app supports macOS 13.
 */
export function splitSentences(text: string): string[] {
  const out: string[] = [];
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if ((c === "." || c === "!" || c === "?") && (i + 1 === text.length || /\s/.test(text[i + 1]))) {
      out.push(text.slice(start, i + 1).trim());
      start = i + 1;
    }
  }
  if (start < text.length) out.push(text.slice(start).trim());
  return out.filter(Boolean);
}

/** The hits as the model sees them, after the user's message. Empty when none. */
export function notesFor(hits: Hit[], maxChars = 600, header = WIKI_HEADER, query = ""): string {
  if (!hits.length) return "";
  return `\n\n${header}\n` + hits.map((h) => `- ${h.t}: ${bestWindow(h.x, query, maxChars)}`).join("\n");
}
