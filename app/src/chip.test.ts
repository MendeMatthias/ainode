import { describe, expect, it } from "vitest";
import { CARD_LINE, CARD_LINE_TITLE, COVERAGE_FRESH_MS, chipLabel, chipOf, chipTitle, firstLede, hostsAnswering, startSearchDue, type ChipInput, type Coverage } from "./chip";

const NOW = 1_800_000_000_000;
const cov = (asked: number, answered: number, ageMs = 5_000): Coverage => ({ asked, answered, indexPeers: 0, atMs: NOW - ageMs });
const at = (over: Partial<ChipInput>): ChipInput => ({ engine: "running", hosts: 1, fetching: false, waiting: false, coverage: null, nowMs: NOW, ...over });

describe("the ModelNet chip says only what the engine reported", () => {
  it("is the plain grey name while the engine is off, starting or failed", () => {
    for (const engine of ["stopped", "starting", "error"]) {
      const c = chipOf(at({ engine, coverage: cov(1, 1) }));
      expect(c).toEqual({ tone: "off", lead: "", detail: "" });
      expect(chipLabel(c)).toBe("BTX ModelNet");
    }
  });

  it("is the plain name when the engine runs with no host to reach", () => {
    expect(chipOf(at({ hosts: 0 }))).toEqual({ tone: "off", lead: "", detail: "" });
  });

  it("shows no number before a network answer was counted", () => {
    const c = chipOf(at({}));
    expect(chipLabel(c)).toBe("Powered by BTX ModelNet");
    expect(chipLabel(c)).not.toMatch(/\d/);
  });

  it("shows the hosts that answered, as the engine counted them", () => {
    expect(chipLabel(chipOf(at({ coverage: cov(1, 1) })))).toBe("Powered by BTX ModelNet · 1 host answering");
    expect(chipLabel(chipOf(at({ coverage: cov(3, 2) })))).toBe("Powered by BTX ModelNet · 2 hosts answering");
    expect(hostsAnswering(1)).toBe("1 host answering");
  });

  it("drops the number once the answer is old, and never trusts a time from the future", () => {
    expect(chipLabel(chipOf(at({ coverage: cov(1, 1, COVERAGE_FRESH_MS + 1) })))).toBe("Powered by BTX ModelNet");
    expect(chipLabel(chipOf(at({ coverage: cov(1, 1, -60_000) })))).toBe("Powered by BTX ModelNet");
    expect(chipLabel(chipOf(at({ coverage: cov(1, 1, COVERAGE_FRESH_MS) })))).toBe("Powered by BTX ModelNet · 1 host answering");
  });

  it("says so when the engine asked and nobody answered", () => {
    const c = chipOf(at({ coverage: cov(1, 0) }));
    expect(c.tone).toBe("warn");
    expect(chipLabel(c)).toBe("BTX ModelNet · no host answered");
    // Nobody asked is not "no host answered".
    expect(chipLabel(chipOf(at({ coverage: cov(0, 0) })))).toBe("Powered by BTX ModelNet");
  });

  it("says a download is checked piece by piece, with no number", () => {
    const c = chipOf(at({ fetching: true, coverage: cov(1, 1) }));
    expect(c.tone).toBe("busy");
    expect(chipLabel(c)).toBe("Fetching over BTX ModelNet · each piece checked");
    expect(chipLabel(c)).not.toMatch(/\d/);
  });

  it("warns when a running download's host stopped answering", () => {
    const c = chipOf(at({ fetching: true, waiting: true, coverage: cov(1, 1) }));
    expect(chipLabel(c)).toBe("BTX ModelNet · no host answered");
    // A stalled flag without a download means nothing.
    expect(chipLabel(chipOf(at({ waiting: true })))).toBe("Powered by BTX ModelNet");
  });

  it("has no state beyond the five that were built, for any input", () => {
    // Every combination of what the chip is given. A new label, such as one of
    // the two later states ("through your BTX node", "sharing ..."), fails here.
    const allowed = [
      /^BTX ModelNet$/,
      /^Powered by BTX ModelNet$/,
      /^Powered by BTX ModelNet · [1-9]\d* hosts? answering$/,
      /^Fetching over BTX ModelNet · each piece checked$/,
      /^BTX ModelNet · no host answered$/,
    ];
    const seen = new Set<string>();
    for (const engine of ["stopped", "starting", "running", "error", "something-new"]) {
      for (const hosts of [0, 1, 3]) {
        for (const fetching of [false, true]) {
          for (const waiting of [false, true]) {
            for (const coverage of [null, cov(0, 0), cov(1, 0), cov(1, 1), cov(4, 3), cov(1, 1, COVERAGE_FRESH_MS + 1), cov(2, 0, COVERAGE_FRESH_MS + 1), cov(1, 1, -5)]) {
              const label = chipLabel(chipOf({ engine, hosts, fetching, waiting, coverage, nowMs: NOW }));
              expect(allowed.some((a) => a.test(label)), label).toBe(true);
              seen.add(label.replace(/\d+ hosts?/, "N hosts"));
            }
          }
        }
      }
    }
    // And all five are reachable: the list above is not wider than the code.
    expect([...seen].sort()).toEqual([
      "BTX ModelNet",
      "BTX ModelNet · no host answered",
      "Fetching over BTX ModelNet · each piece checked",
      "Powered by BTX ModelNet",
      "Powered by BTX ModelNet · N hosts answering",
    ]);
  });

  it("tells in its tooltip whose count it is", () => {
    expect(chipTitle(chipOf(at({ coverage: cov(1, 1) })))).toMatch(/not a count of the whole network/);
    expect(chipTitle(chipOf(at({ coverage: cov(1, 0) })))).toMatch(/No host answered/);
    expect(chipTitle(chipOf(at({ engine: "stopped" })))).toBe("Open the BTX ModelNet view");
  });

  it("words the card line about bytes, not about quality", () => {
    expect(CARD_LINE).toBe("Bytes verified · over BTX ModelNet");
    expect(`${CARD_LINE} ${CARD_LINE_TITLE}`).not.toMatch(/safe to|trusted|approved|audited|secure\b/i);
    expect(CARD_LINE_TITLE).toMatch(/intact, not safe or correct/);
  });

  it("asks for the start-up search once per engine run, only while the engine runs and the window is shown", () => {
    // The first run, window shown: due.
    expect(startSearchDue("running", 1, 0, true)).toBe(true);
    // Already asked for this run: not again, however often the page polls.
    expect(startSearchDue("running", 1, 1, true)).toBe(false);
    // A restart (the user's or the keeper's): a new run, due once more.
    expect(startSearchDue("running", 2, 1, true)).toBe(true);
    // Window hidden or minimized: not now; it is asked once the window shows.
    expect(startSearchDue("running", 2, 1, false)).toBe(false);
    // No engine, no run (and before consent there is no engine).
    for (const engine of ["stopped", "starting", "error"]) expect(startSearchDue(engine, 3, 0, true)).toBe(false);
    expect(startSearchDue("running", 0, 0, true)).toBe(false);
    // Polled many times across two runs: two requests in all.
    let asked = 0;
    let requests = 0;
    for (const run of [1, 1, 1, 1, 2, 2, 2]) {
      if (startSearchDue("running", run, asked, true)) { asked = run; requests += 1; }
    }
    expect(requests).toBe(2);
  });
});

describe("BTX ModelNet by name outside the chip (plan section 6)", () => {
  it("the first-run sentence names it and keeps to what AInode does", () => {
    expect(firstLede("this Mac")).toBe("Open AI models over BTX ModelNet. Checked piece by piece, run on this Mac. No account.");
    expect(firstLede("this PC")).toContain("run on this PC.");
  });
});
