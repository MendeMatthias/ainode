import { describe, expect, it } from "vitest";
import { buildIndex, notesFor, search, splitSentences, tokens } from "./search";

// A handful of real-style openings: enough to check ranking, not a benchmark
// (lab/replay-chat.py "+kb" measures the real pack against real models).
const docs = [
  { t: "Bob Dylan", x: "Bob Dylan is an American singer-songwriter. He was awarded the 2016 Nobel Prize in Literature for having created new poetic expressions within the great American song tradition." },
  { t: "Nobel Prize in Literature", x: "The Nobel Prize in Literature is a Swedish literature prize awarded annually since 1901." },
  { t: "Hieronymus Bosch", x: "Hieronymus Bosch was a Dutch painter from Brabant. His best-known work is the triptych The Garden of Earthly Delights." },
  { t: "Astana", x: "Astana, previously known as Akmolinsk, Tselinograd, Akmola and Nur-Sultan, is the capital city of Kazakhstan." },
  { t: "Kazakhstan", x: "Kazakhstan is a landlocked country in Central Asia. Its capital is Astana, and its largest city is Almaty." },
  { t: "War and Peace", x: "War and Peace is a novel by the Russian author Leo Tolstoy, published in 1869." },
  { t: "Peace", x: "Peace is a state of harmony characterized by a lack of violence and conflict." },
  { t: "Sky", x: "The sky is everything that lies above the surface of the Earth. During the day it appears blue because of Rayleigh scattering." },
];
const ix = buildIndex(docs);
// No score floor on a toy set: its word-rarity numbers are not the real pack's.
// The floor is tested against the real pack below.
const titles = (q: string) => search(ix, q, 3, 0, 1).map((h) => h.t);

describe("search", () => {
  it("finds the paragraph that answers a factual question", () => {
    expect(titles("Who painted The Garden of Earthly Delights?")[0]).toBe("Hieronymus Bosch");
    expect(titles("What is the capital of Kazakhstan?")).toContain("Astana");
    expect(search(ix, "Bob Dylan Nobel Prize 2016", 3, 0, 1).map((h) => h.t)[0]).toBe("Bob Dylan");
  });
  it("needs two words of the message to match: one word is how small talk found junk", () => {
    // On the real pack: "are you reading?" found "Reading", "hello" found "Hello Kitty".
    expect(titles("tell me about peace")).toEqual([]);
    expect(search(ix, "peace", 3, 0, 1)).toEqual([]);
  });
  it("gives an article named in the question its place", () => {
    expect(titles("Who wrote War and Peace?")[0]).toBe("War and Peace");
  });
  it("sends nothing when the best two nearly tie: the search is guessing", () => {
    const tie = buildIndex([
      { t: "Nauru", x: "Nauru is an island country, the third-smallest country by area in the world." },
      { t: "Tuvalu", x: "Tuvalu is an island country, the fourth-smallest country by area in the world." },
    ]);
    expect(search(tie, "smallest country by area", 1, 0)).toEqual([]);
  });
  it("returns at most k hits, best first", () => {
    const hits = search(ix, "Kazakhstan capital Astana Almaty", 2, 0, 1);
    expect(hits.length).toBeLessThanOrEqual(2);
    expect(hits[0].score).toBeGreaterThanOrEqual(hits[hits.length - 1].score);
  });
  it("reads words without accents, case or punctuation", () => {
    expect(tokens("Schrödinger's équation, 1926!")).toEqual(["schrodinger", "s", "equation", "1926"]);
  });
});

describe("notesFor", () => {
  it("is empty without hits, and says where the notes come from otherwise", () => {
    expect(notesFor([])).toBe("");
    const n = notesFor(search(ix, "Bob Dylan Nobel Prize 2016", 3, 0, 1));
    expect(n).toContain("[Encyclopedia notes from AInode (Wikipedia).");
    expect(n).toContain("the user did not write them");
    expect(n).toContain("- Bob Dylan: Bob Dylan is an American singer-songwriter.");
  });
  it("keeps a long passage's first sentence and the sentences that match the question", () => {
    const x = "Bob Dylan is an American singer-songwriter. He was born in Duluth in 1941. " +
      "He wrote many songs in the 1960s. He was awarded the 2016 Nobel Prize in Literature. He still tours.";
    const n = notesFor([{ t: "Bob Dylan", x, score: 9 }], 100, "[h]", "Who won the Nobel Prize in Literature in 2016?");
    expect(n).toContain("Bob Dylan is an American singer-songwriter.");
    expect(n).toContain("He was awarded the 2016 Nobel Prize in Literature.");
    expect(n).not.toContain("Duluth");
  });
  it("splits sentences without breaking version numbers or decimals", () => {
    expect(splitSentences("The release is v0.34.15. Pi is about 3.14! Really? Yes")).toEqual(
      ["The release is v0.34.15.", "Pi is about 3.14!", "Really?", "Yes"]);
  });
  it("reads on in order when the question gives nothing to go by", () => {
    const long = { t: "Long", x: `${"First sentence here. ".repeat(40)}End.`, score: 9 };
    const n = notesFor([long], 100);
    expect(n.endsWith("here.")).toBe(true);
    expect(n.length).toBeLessThan(250);
  });
});

describe("the real pack (wiki.json)", async () => {
  const { readFileSync } = await import("node:fs");
  const real = buildIndex(JSON.parse(readFileSync(new URL("./wiki.json", import.meta.url), "utf8")));
  it("finds nothing in the encyclopedia for Mende's real chat lines", () => {
    // (Raw encyclopedia search. In the app, lines that name BTX go to BTX's docs
    // first: notes.test.ts checks what the app sends.)
    for (const q of ["hi", "Eho sre you?", "i asked: who are you ?", "where exactly are you running ?",
      "how is this related to btw?", "btx is a blockchain", "are you reading?", "hello",
      "of course you run on it, decentralized on many nodes / systems. currently here on this mac",
      "What is 17 times 3?", "What is 15% of 240?"]) {
      expect(search(real, q), q).toEqual([]);
    }
  });
  it("finds the article holding the answer to the facts the 3B made up", () => {
    // "Hiero Vermee", "Akira Koreya", "Almaty": its answers without the pack.
    expect(search(real, "Who painted 'The Garden of Earthly Delights'?")[0]?.x).toContain("Bosch");
    expect(search(real, "Who directed the film 'Seven Samurai'?")[0]?.x).toContain("Kurosawa");
    expect(search(real, "What is the capital of Kazakhstan?")[0]?.x).toContain("Astana");
  });
  it("trusts an article the question names, even with close neighbours", () => {
    expect(search(real, "Who wrote 'The Tale of Genji'?")[0]?.t).toBe("The Tale of Genji");
  });
  it("stays quiet when the best two nearly tie, as for these near-miss traps", () => {
    for (const q of ["What is the smallest country in the world by area?", "Who was the first woman to win a Nobel Prize?",
      "Who won the Nobel Prize in Literature in 2016?"]) {
      expect(search(real, q), q).toEqual([]);
    }
  });
});
