import { describe, expect, it } from "vitest";
import type { StartSearch } from "./api";
import type { Coverage } from "./chip";
import { createStartSearch, NOT_SENT, RETRIES, RETRY_MS, windowShown } from "./startsearch";

const answer = (run: number, answered = 1): StartSearch => ({
  run, at_ms: 1_000 * run,
  answer: { results: [], coverage: { connected_peers_queried: 1, index_peers_queried: 0, responses_received: answered, timed_out: 1 - answered } },
});
const SHOWN = { visible: true, minimized: false };
const tick = () => new Promise((r) => setTimeout(r, 0));

function harness(reply: (n: number) => Promise<StartSearch>) {
  let run = 0;
  const asks: number[] = [];
  const got: [Coverage, number][] = [];
  const errors: unknown[] = [];
  const s = createStartSearch({
    ask: () => { asks.push(run); return reply(asks.length); },
    currentRun: () => run,
    onCoverage: (c, r) => got.push([c, r]),
    onError: (e) => errors.push(e),
  });
  return { s, asks, got, errors, setRun: (r: number) => { run = r; } };
}

describe("the start-up search, as the page runs it", () => {
  it("asks once per engine run, however often the state is polled", async () => {
    const h = harness(async () => answer(1));
    h.setRun(1);
    for (let i = 0; i < 20; i++) h.s.maybe("running", 1, SHOWN);
    await tick();
    expect(h.asks).toEqual([1]);
    expect(h.got.map(([c, r]) => [c.answered, r])).toEqual([[1, 1]]);
  });

  it("asks again after a restart, once", async () => {
    let n = 0;
    const h = harness(async () => answer(++n));
    h.setRun(1);
    h.s.maybe("running", 1, SHOWN);
    h.setRun(2);
    for (let i = 0; i < 5; i++) h.s.maybe("running", 2, SHOWN);
    await tick();
    expect(h.asks).toEqual([1, 2]);
  });

  it("does not ask while the window is hidden, minimized, or not yet known to be shown", async () => {
    const h = harness(async () => answer(1));
    h.setRun(1);
    h.s.maybe("running", 1, { visible: false, minimized: false });
    h.s.maybe("running", 1, { visible: true, minimized: true });
    // A login launch: visible to the page, but the window has not said whether it is minimized.
    h.s.maybe("running", 1, { visible: true, minimized: null });
    await tick();
    expect(h.asks).toEqual([]);
    // Once it is shown: one request.
    h.s.maybe("running", 1, SHOWN);
    h.s.maybe("running", 1, SHOWN);
    await tick();
    expect(h.asks).toEqual([1]);
    expect(windowShown({ visible: true, minimized: null })).toBe(false);
  });

  it("does not ask while the engine is not running", async () => {
    const h = harness(async () => answer(1));
    for (const e of ["stopped", "starting", "error"]) h.s.maybe(e, 1, SHOWN);
    h.s.maybe("running", 0, SHOWN);
    await tick();
    expect(h.asks).toEqual([]);
  });

  it("does not ask again in the run after a failure, and shows no count", async () => {
    const h = harness(async () => { throw new Error("engine busy"); });
    h.setRun(1);
    h.s.maybe("running", 1, SHOWN);
    await tick();
    h.s.maybe("running", 1, SHOWN);
    await tick();
    expect(h.asks).toEqual([1]);
    expect(h.got).toEqual([]);
    expect(h.errors).toHaveLength(1);
  });

  it("drops an answer for a run that is already over", async () => {
    let release: (v: StartSearch) => void = () => {};
    const h = harness(() => new Promise((r) => { release = r; }));
    h.setRun(1);
    h.s.maybe("running", 1, SHOWN);
    h.setRun(2); // the engine restarted while the answer was on its way
    release(answer(1));
    await tick();
    expect(h.got).toEqual([]);
  });

  describe("a refusal in which nothing was sent (second review, finding 4)", () => {
    const setup = (fail: (n: number) => boolean) => {
      let t = 0;
      const asks: number[] = [];
      const got: number[] = [];
      const s = createStartSearch({
        ask: () => {
          asks.push(asks.length + 1);
          return fail(asks.length) ? Promise.reject(`${NOT_SENT}the window is not shown: the start-up search waits until it is`) : Promise.resolve(answer(1));
        },
        currentRun: () => 1,
        onCoverage: (c) => got.push(c.answered),
        now: () => t,
      });
      return { s, asks, got, at: (ms: number) => { t = ms; } };
    };
    const HIDDEN = { visible: false, minimized: false };

    it("does not use up the run: the page asks again once the window is hidden and shown", async () => {
      const h = setup((n) => n === 1);
      h.s.maybe("running", 1, SHOWN);
      await tick();
      // Still shown, polled again and again: no second request yet.
      for (let i = 0; i < 5; i++) h.s.maybe("running", 1, SHOWN);
      await tick();
      expect(h.asks).toHaveLength(1);
      h.s.maybe("running", 1, HIDDEN);
      h.s.maybe("running", 1, SHOWN);
      await tick();
      expect(h.asks).toHaveLength(2);
      expect(h.got).toEqual([1]);
      // Answered: no more requests in the run.
      h.s.maybe("running", 1, HIDDEN);
      h.s.maybe("running", 1, SHOWN);
      await tick();
      expect(h.asks).toHaveLength(2);
    });

    it("also asks again a little later while the window stays shown, a few times at most", async () => {
      const h = setup(() => true);
      h.s.maybe("running", 1, SHOWN);
      await tick();
      for (let i = 1; i <= RETRIES + 3; i++) {
        h.at(i * RETRY_MS);
        h.s.maybe("running", 1, SHOWN);
        await tick();
      }
      expect(h.asks).toHaveLength(1 + RETRIES);
    });

    it("a search that was sent and failed is still not repeated", async () => {
      let n = 0;
      const s = createStartSearch({
        ask: () => { n++; return Promise.reject(new Error("engine did not answer searchmodels in time")); },
        currentRun: () => 1,
        onCoverage: () => {},
        now: () => 1e9,
      });
      s.maybe("running", 1, SHOWN);
      await tick();
      s.maybe("running", 1, HIDDEN);
      s.maybe("running", 1, SHOWN);
      await tick();
      expect(n).toBe(1);
    });
  });
});
