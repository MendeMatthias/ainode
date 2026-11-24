import { describe, expect, it } from "vitest";
import { GLYPHS, latticeTiming, mulberry32, nearest, noise2, scrambleAt, stepAlong, traceEdges } from "./fx";

describe("scrambleAt", () => {
  const rnd = mulberry32(7);
  it("is the text at the end and glyphs at the start; spaces never scramble", () => {
    const text = "Granite is ready. It runs here.";
    expect(scrambleAt(text, 1, rnd)).toBe(text);
    const start = scrambleAt(text, 0, rnd);
    expect(start.length).toBe(text.length);
    for (let i = 0; i < text.length; i++) {
      if (text[i] === " ") expect(start[i]).toBe(" ");
      else expect(GLYPHS.includes(start[i])).toBe(true);
    }
  });
  it("settles from left to right and never un-settles", () => {
    // Lower case only: no letter here is also a glyph, so a match means settled.
    const text = "loading granite on this mac";
    let settled = new Set<number>();
    for (let t = 0; t <= 1.0001; t += 0.05) {
      const now = scrambleAt(text, t, rnd);
      const here = new Set<number>();
      for (let i = 0; i < text.length; i++) if (now[i] === text[i] && text[i] !== " ") here.add(i);
      for (const i of settled) expect(here.has(i)).toBe(true);
      settled = here;
    }
    expect(settled.size).toBe(text.replace(/ /g, "").length);
    // Half way, the left half has more settled letters than the right half.
    const mid = scrambleAt("ABCDEFGHIJKLMNOPQRST", 0.5, rnd);
    const left = [...mid.slice(0, 10)].filter((c, i) => c === "ABCDEFGHIJ"[i]).length;
    const right = [...mid.slice(10)].filter((c, i) => c === "KLMNOPQRST"[i]).length;
    expect(left).toBeGreaterThan(right);
  });
  it("keeps multi-byte characters whole", () => {
    expect(scrambleAt("Mende → ∞", 1, rnd)).toBe("Mende → ∞");
    expect([...scrambleAt("Mende → ∞", 0, rnd)].length).toBe(9);
  });
});

describe("noise and random", () => {
  it("mulberry32 is deterministic and in [0, 1)", () => {
    const a = mulberry32(42), b = mulberry32(42);
    for (let i = 0; i < 100; i++) {
      const x = a();
      expect(x).toBe(b());
      expect(x).toBeGreaterThanOrEqual(0);
      expect(x).toBeLessThan(1);
    }
  });
  it("noise2 is continuous and bounded", () => {
    for (let i = 0; i < 200; i++) {
      const x = i * 0.37, y = i * 0.11;
      const v = noise2(x, y);
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(1);
      expect(Math.abs(noise2(x + 0.01, y) - v)).toBeLessThan(0.05);
    }
  });
});

describe("latticeTiming", () => {
  it("gives a delay per cell, holes as null, and a cycle that covers the loop", () => {
    const t = latticeTiming("orbit", 90);
    expect(t.n).toBe(3);
    expect(t.delays.length).toBe(9);
    expect(t.delays[4]).toBeNull();
    expect(t.delays[0]).toBe(0);
    expect(Math.max(...t.delays.filter((d): d is number => d != null))).toBeLessThan(t.cycleMs);
    const s = latticeTiming("sweep");
    expect(s.n).toBe(4);
    expect(s.delays.length).toBe(16);
  });
});

describe("traceEdges", () => {
  // A filled 20x12 rectangle inside a 30x20 raster.
  const w = 30, h = 20;
  const cov = new Float32Array(w * h);
  for (let y = 4; y < 16; y++) for (let x = 5; x < 25; x++) cov[y * w + x] = 1;
  const s = traceEdges(cov, w, h, 10_000);
  it("finds exactly the boundary pixels with outward normals", () => {
    const n = s.pts.length / 2;
    expect(n).toBe(2 * 20 + 2 * 12 - 4);
    for (let i = 0; i < n; i++) {
      const x = s.pts[i * 2], y = s.pts[i * 2 + 1];
      const onLeft = x === 5.5, onRight = x === 24.5, onTop = y === 4.5, onBottom = y === 15.5;
      expect(onLeft || onRight || onTop || onBottom).toBe(true);
      if (onLeft && !onTop && !onBottom) expect(s.nrm[i * 2]).toBeLessThan(0);
      if (onRight && !onTop && !onBottom) expect(s.nrm[i * 2]).toBeGreaterThan(0);
      if (onTop && !onLeft && !onRight) expect(s.nrm[i * 2 + 1]).toBeLessThan(0);
      if (onBottom && !onLeft && !onRight) expect(s.nrm[i * 2 + 1]).toBeGreaterThan(0);
    }
  });
  it("thins to a limit", () => {
    expect(traceEdges(cov, w, h, 20).pts.length / 2).toBeLessThanOrEqual(20);
  });
  it("nearest finds the closest edge point from anywhere", () => {
    const i = nearest(s, 0, 0);
    expect([s.pts[i * 2], s.pts[i * 2 + 1]]).toEqual([5.5, 4.5]);
    const j = nearest(s, 15, 10); // the middle: the nearest edge is the top or bottom row
    expect([4.5, 15.5]).toContain(s.pts[j * 2 + 1]);
  });
  it("stepAlong walks the outline in the asked direction", () => {
    const start = nearest(s, 5.5, 10.5); // a left-edge point
    const next = stepAlong(s, start, 0, -1, 2, mulberry32(1));
    expect(next).toBeGreaterThanOrEqual(0);
    expect(s.pts[next * 2 + 1]).toBeLessThan(10.5); // moved up
    expect(s.pts[next * 2]).toBe(5.5); // and stayed on the edge
  });
});

describe("coverageFromLuma", () => {
  it("marks the bright pixels of an RGBA buffer, nothing else", async () => {
    const { coverageFromLuma } = await import("./fx");
    // 3x2 pixels: dark, mid, bright / black, green, white
    const px = new Uint8ClampedArray([10, 10, 10, 255, 80, 80, 80, 255, 240, 240, 240, 255, 0, 0, 0, 255, 0, 200, 0, 255, 255, 255, 255, 255]);
    const cov = coverageFromLuma(px, 3, 2, 110);
    expect(Array.from(cov)).toEqual([0, 0, 1, 0, 1, 1]);
  });
});
