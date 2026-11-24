import { describe, expect, it } from "vitest";
import { fileOf, kindOf, looksScanned, pack, splitMarkdown, splitPdf, splitText, PASSAGE_CHARS } from "./own";
import { buildIndex } from "./search";
import { fromMine, lookUp, MINE_HEADER } from "./notes";

describe("reading the user's own files", () => {
  it("knows what it can read", () => {
    expect(kindOf("Report.PDF")).toBe("pdf");
    expect(kindOf("notes.md")).toBe("md");
    expect(kindOf("a.markdown")).toBe("md");
    expect(kindOf("todo.txt")).toBe("txt");
    expect(kindOf("deck.pptx")).toBeNull();
    expect(kindOf("README")).toBeNull();
  });

  it("packs paragraphs into passages and cuts a long one at sentence ends", () => {
    const long = Array.from({ length: 60 }, (_, i) => `Sentence number ${i} is here.`).join(" ");
    const parts = pack(`Short one.\n\n${long}\n\nLast.`);
    expect(parts.every((p) => p.length <= PASSAGE_CHARS)).toBe(true);
    expect(parts.join(" ")).toContain("Sentence number 59 is here.");
    expect(parts.some((p) => /is here\.$/.test(p))).toBe(true);
    expect(pack("")).toEqual([]);
  });

  it("titles Markdown passages by their heading, and ignores # inside code", () => {
    const docs = splitMarkdown("plan.md", "Intro text.\n\n# Setup\nInstall it.\n```\n# not a heading\n```\n## Run\nRun it.");
    expect(docs.map((d) => d.t)).toEqual(["plan.md", "plan.md > Setup", "plan.md > Run"]);
    expect(docs[1].x).toContain("# not a heading");
  });

  it("titles PDF passages by page, and tells a scan from text", () => {
    const docs = splitPdf("paper.pdf", ["Abstract. We study lattices.", "", "Results."]);
    expect(docs.map((d) => d.t)).toEqual(["paper.pdf, page 1", "paper.pdf, page 3"]);
    expect(looksScanned(["", " ", "3"])).toBe(true);
    expect(looksScanned([])).toBe(true);
    expect(looksScanned(["A page of real text that is long enough to read, more than forty characters."])).toBe(false);
    expect(splitText("a.txt", "one\n\ntwo")).toEqual([{ t: "a.txt", x: "one\n\ntwo" }]);
  });

  it("finds the file a passage came from", () => {
    expect(fileOf("plan.md > Setup > more")).toBe("plan.md");
    expect(fileOf("my, page file.pdf, page 12")).toBe("my, page file.pdf");
    expect(fileOf("a.txt")).toBe("a.txt");
  });
});

describe("searching the user's own files", () => {
  const ix = buildIndex([
    ...splitMarkdown("invoice-rules.md", "# Payment terms\nInvoices are due within 45 days of delivery. Late invoices pay 2% interest a month.\n\n# Contacts\nAccounts payable is run by Leila Haddad."),
    ...splitPdf("garden.pdf", ["Tomatoes need six hours of sun and water at the roots.", "Basil grows well next to tomatoes."]),
  ]);

  it("answers from the right passage, ahead of the encyclopedia", () => {
    const hits = fromMine(ix, "When are invoices due?");
    expect(hits[0].t).toBe("invoice-rules.md > Payment terms");
    const looked = lookUp({ wiki: null, btx: null, btxTag: "", mine: ix }, "Who runs accounts payable?", []);
    expect(looked.source).toBe("your files");
    expect(looked.titles).toEqual(["invoice-rules.md"]);
    expect(looked.notes).toContain(MINE_HEADER);
    expect(looked.notes).toContain("Leila Haddad");
  });

  it("stays quiet when the files do not answer", () => {
    expect(fromMine(ix, "What is the capital of Peru?")).toEqual([]);
    expect(lookUp({ wiki: null, btx: null, btxTag: "", mine: ix }, "hello there", []).notes).toBe("");
  });

  it("finds a file that is one passage, like a one-page PDF", () => {
    // BM25 alone weighed every word of a one-passage collection at log(1.33),
    // so no question could reach the bar: the Linux window check of
    // 2026-10-13 added a one-page PDF and asked for the code in it, and the
    // file was never consulted.
    const one = buildIndex(splitPdf("lab-notes.pdf", ["AInode test document. The access code for the lab is ORCHID-4417. This file was added to check that AInode reads PDFs."]));
    expect(fromMine(one, "What is the access code in my file?").map((h) => fileOf(h.t))).toEqual(["lab-notes.pdf"]);
    expect(fromMine(one, "What is the capital of Peru?")).toEqual([]);
    expect(lookUp({ wiki: null, btx: null, btxTag: "", mine: one }, "hello there", []).notes).toBe("");
  });

  it("'summarise it' after adding a file gets that file's opening", () => {
    const hits = fromMine(ix, "Can you summarise this file?", "garden.pdf");
    expect(hits.map((h) => h.t)).toEqual(["garden.pdf, page 1", "garden.pdf, page 2"]);
    expect(fromMine(ix, "Can you summarise this file?")).toEqual([]);
  });
});
