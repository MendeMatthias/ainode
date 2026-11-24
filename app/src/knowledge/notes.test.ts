import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { aboutBtx, fromLibrary, lookUp, parseLibrary, type Library } from "./notes";
import { buildIndex } from "./search";

const read = (f: string) => JSON.parse(readFileSync(new URL(f, import.meta.url), "utf8"));
const btx = read("./btx-docs.json");
const own = parseLibrary(readFileSync(new URL("./library.md", import.meta.url), "utf8"));
const lib: Library = { wiki: buildIndex(read("./wiki.json")), btx: buildIndex(btx.docs), btxTag: btx.tag, own };
const titles = (q: string) => fromLibrary(own, q).map((p) => p.t);

describe("aboutBtx", () => {
  it("is about BTX when the message or one of the last two says so", () => {
    expect(aboutBtx("what are the latest btx updates?", [])).toBe(true);
    expect(aboutBtx("and whats the great plan?", ["what is btx?"])).toBe(true);
    expect(aboutBtx("how does MatMul mining work", [])).toBe(true);
    expect(aboutBtx("who wrote Hamlet?", [])).toBe(false);
    expect(aboutBtx("who wrote Hamlet?", ["btx is a blockchain", "ok", "thanks"])).toBe(false);
  });
});

describe("lookUp on the real library", () => {
  it("answers Mende's BTX questions from BTX's own docs", () => {
    const latest = lookUp(lib, "what are the latest btx updates?", []);
    expect(latest.source).toBe(`BTX docs ${btx.tag}`);
    expect(latest.notes).toContain("The shipping release is v0.34.16");
    // The releases themselves reach the model, newest first: as one long sentence
    // the list did not fit the note and only its first sentence was sent (0.9.0).
    expect(latest.notes).toContain("0.34.16: This tag packages");
    expect(latest.notes).toContain("0.34.15: Followed tip and host formats");
    expect(latest.notes.indexOf("0.34.16:")).toBeLessThan(latest.notes.indexOf("0.34.15:"));
    expect(latest.titles).toHaveLength(1); // the release list, without the weak second hit
    // 0.34.16's own notes (at the top of doc/, underlined headings) are found.
    expect(lookUp(lib, "what changed in btx 0.34.16?", []).notes).toContain("BTX 0.34.16 release notes > Upgrading from 0.34.15");
    expect(lookUp(lib, "does btx 0.34.16 embed llama.cpp or ollama?", []).notes).toContain("does not embed llama.cpp or Ollama");
    expect(lookUp(lib, "what is the plan and vision of btx?", []).notes).toContain("What 0.34.7 is");
    // A one-word follow-up finds no passage; the always-present pack (btx.md) has the plan.
    expect(lookUp(lib, "and whats the great plan?", ["what are the latest btx updates?"]).notes).toBe("");
    expect(lookUp(lib, "is btx private? what happened to the shielded pool?", []).notes).toMatch(/199,?300/);
  });
  it("never sends an encyclopedia note for any of Mende's real chat lines", () => {
    for (const q of ["hi", "Eho sre you?", "i asked: who are you ?", "where exactly are you running ?",
      "how is this related to btw?", "btx is a blockchain", "are you reading?", "hello",
      "why are you stupid and think you know better? you running on BTX annd you are decentralized",
      "of course you run on it, decentralized on many nodes / systems. currently here on this mac",
      "Yes how are you affiliated with it?", "i think you should upgrade your entire knowledge about how you function ,and whats the great plan. did you study the btx dev updates on GitHub???"]) {
      expect(lookUp(lib, q, []).source, q).not.toContain("Wikipedia");
    }
  });
  it("falls back to the encyclopedia when no BTX passage fits, and adds nothing to small talk", () => {
    const bosch = lookUp(lib, "Who painted The Garden of Earthly Delights?", ["btx is a blockchain"]);
    expect(bosch.source).toBe("Wikipedia, offline");
    for (const q of ["hi", "where exactly are you running ?", "thanks!", "are you reading?", "tell me more", "i ment BTX"]) {
      expect(lookUp(lib, q, ["btx is a blockchain"]).notes, q).toBe("");
    }
  });
  it("says where BTX notes come from and that the user did not write them", () => {
    const n = lookUp(lib, "what is a release campaign in btx?", []).notes;
    expect(n).toContain(`[BTX documentation notes from AInode (btxchain/btx ${btx.tag}).`);
    expect(n).toContain("the user did not write them");
  });
});

describe("AInode's own library (library.md)", () => {
  it("parses every section with its keys, and no em-dashes", () => {
    expect(own.length).toBeGreaterThanOrEqual(15);
    for (const p of own) {
      expect(p.keys.source, p.t).not.toBe("(?!)");
      expect(p.x, p.t).not.toMatch(/\u2014|<!--/);
    }
  });
  it("answers distress, crisis first", () => {
    expect(titles("I am so stressed and anxious")).toEqual(["When you feel stressed"]);
    expect(titles("I want to die, I am so stressed")[0]).toBe("If it is more than stress");
    expect(lookUp(lib, "I don't want to live anymore", []).notes).toContain("emergency services");
  });
  it("never recommends Wim Hof breathing without its safety rules", () => {
    const wh = own.find((p) => p.t === "The Wim Hof Method")!;
    expect(wh.x).toContain("never do it in or near water");
    expect(wh.x).toContain("while driving");
    expect(wh.x).toMatch(/ask a doctor/i);
    expect(titles("how should I breathe? teach me wim hof")).toContain("The Wim Hof Method");
  });
  it("recommends The Laws of the Sun as Mende's, saying what it is", () => {
    const f = own.find((p) => p.t === "The Laws of the Sun")!;
    expect(f.x).toContain("Mende, who made AInode, recommends");
    expect(f.x).toContain("Happy Science");
    expect(titles("how can I wake up spiritually?")).toEqual(["Enlightenment and awakening", "The Laws of the Sun"]);
  });
  it("covers the topics Mende asked for", () => {
    expect(titles("what are the chakras?")).toEqual(["Chakras"]);
    expect(titles("what do you think about religion?")).toEqual(["Religions"]);
    expect(titles("who are the greatest inventors?")).toContain("Great inventors");
    expect(titles("what is open source?")).toEqual(["Open source"]);
    expect(titles("how can I be a good person and make the world better?")).toContain("Being good, and a better world");
  });
  it("knows Mende and the ecosystem, from public facts only", () => {
    expect(titles("who made you?")).toEqual(["Mende, who made AInode"]);
    expect(titles("who is Mende?")).toEqual(["Mende, who made AInode"]);
    expect(titles("tell me about the bonuz ecosystem")).toContain("The Bonuz ecosystem");
    expect(titles("what is easynode?")).toEqual(["easyNode"]);
    const all = own.map((p) => p.x).join(" ");
    expect(all).not.toMatch(/jurisdiction|holding company|registered (in|address)/i);
    expect(all).toContain("BTX itself is made by its own, separate developers");
  });
  it("knows everything on Mende's second list, each from its own passage", () => {
    const cases: [string, string][] = [
      ["what did einstein think about life?", "Albert Einstein on life"],
      ["what was nikola tesla's mindset?", "Nikola Tesla's mindset"],
      ["how does elon musk think?", "Elon Musk's way of thinking"],
      ["tell me about alan watts", "Alan Watts"],
      ["what did krishnamurti teach?", "Jiddu Krishnamurti"],
      ["what does the quran teach?", "The Quran's wisdom"],
      ["are there numeric miracles in the quran?", "Numbers in the Quran"],
      ["what are psychedelics?", "Psychedelics"],
      ["explain the bitcoin whitepaper", "The Bitcoin white paper"],
      ["what is self custody?", "Self-custody, self-sovereignty and freedom"],
      ["which crypto wallet should I use?", "Which crypto wallet to use"],
      ["what is an erc-20 token?", "Ethereum and ERC standards"],
      ["what is zcash?", "Zcash"],
      ["what are the pleiadians?", "The Pleiadians"],
      ["who are the jedi?", "The Jedi"],
      ["did humans come from monkeys?", "Souls, humans and where we come from"],
      ["is history written by the winners?", "History and who writes it"],
      ["how do I manifest my future?", "Manifesting your future"],
      ["what is my purpose in life?", "Purpose in life"],
    ];
    for (const [q, t] of cases) expect(titles(q), q).toContain(t);
  });
  it("keeps facts as facts and beliefs as beliefs", () => {
    const x = (t: string) => own.find((p) => p.t === t)!.x;
    expect(x("Souls, humans and where we come from")).toContain("share common ancestors");
    expect(x("The Pleiadians")).toContain("no scientific evidence");
    expect(x("Numbers in the Quran")).toContain("dispute them");
    expect(x("Psychedelics")).toContain("not a recommendation");
    expect(x("Which crypto wallet to use")).toContain("bonuz is the company of Mende");
  });
  it("does not let a broad word steal a plain question", () => {
    expect(titles("who is your maker?")).not.toContain("DAI");
    expect(titles("who founded tesla?")).not.toContain("Nikola Tesla's mindset");
    expect(titles("how do I forgive someone?")).not.toContain("Christian wisdom");
    // A library hit does not block the encyclopedia.
    const kant = lookUp(lib, "Which philosopher wrote 'Critique of Pure Reason'?", []);
    expect(kant.titles).toContain("Critique of Pure Reason");
  });
  it("stays out of small talk, politics and plain facts", () => {
    for (const q of ["hi", "who should I vote for?", "What is the capital of Australia?", "what are the latest btx updates?"]) {
      expect(titles(q), q).toEqual([]);
    }
  });
});

describe("what the bundled text says about AInode", () => {
  // AInode only downloads: it shares no model on, has no wallet and passes the engine
  // no chain name (SECURITY.md, ENGINE-CONTRACT.md). A small model repeats what it is
  // given, so no sentence it is given may say otherwise.
  const strip = (s: string) => s.replace(/<!--[\s\S]*?-->/g, "");
  const texts: Record<string, string> = {
    "btx.md": strip(readFileSync(new URL("./btx.md", import.meta.url), "utf8")),
    "instruction.md": strip(readFileSync(new URL("./instruction.md", import.meta.url), "utf8")),
    "library.md": strip(readFileSync(new URL("./library.md", import.meta.url), "utf8")),
    "btx-docs.json": btx.docs.map((d: { t: string; x: string }) => `${d.t}: ${d.x}`).join("\n"),
  };
  const aboutAInode = (t: string) => t.split(/(?<=[.!?])\s+|\n/).filter((s) => /ainode/i.test(s));
  it("never says AInode shares models, has a wallet or uses a chain name", () => {
    for (const [f, t] of Object.entries(texts)) {
      for (const s of aboutAInode(t)) {
        expect(s, f).not.toMatch(/AInode (also )?(shares|seeds|uploads|re-?shares|hosts) |share open AI|AInode('s)? (has|holds|keeps) (a|your) wallet|AInode (sets|uses|passes) .*(chain name|-pwcchain)/i);
      }
    }
    expect(texts["btx.md"]).toContain("AInode only downloads. It does not share models on");
  });
  it("names AInode nowhere in BTX's own docs", () => {
    expect(aboutAInode(texts["btx-docs.json"])).toEqual([]);
  });
});
