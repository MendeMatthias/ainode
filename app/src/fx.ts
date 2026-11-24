// Visual effects for the chat: the loading scene (a field of glitching letters
// with an electric BTX mark that morphs into a face), the lattice loader that
// shows while the model thinks, and scrambled text. Canvas 2D and CSS only, no
// dependencies, nothing leaves this Mac. The pure parts (scramble timing, edge
// tracing, lattice patterns, noise) have tests in fx.test.ts; the rest needs a
// window. Reduced motion turns every animation off (calm()).
//
// Inspired by react-bits' Letter Glitch, Electric Logo, Lattice Loader and
// Scrambled Text (MIT), written again here for a plain WebKit page.

export const GLYPHS = "░▒▓01<>/\\|#%&$@*+=:;.-_ABCDEFXYZ";
export const calm = (): boolean =>
  typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// ── Random and noise (pure) ──────────────────────────────────────────────────

/** A small seedable generator, so tests can pin a frame. */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const fract = (x: number) => x - Math.floor(x);
function hash2(x: number, y: number): number {
  const h = Math.sin(x * 127.1 + y * 311.7) * 43758.5453;
  return fract(h);
}
/** Value noise in [0, 1], smooth over about one unit. */
export function noise2(x: number, y: number): number {
  const ix = Math.floor(x), iy = Math.floor(y);
  const fx = x - ix, fy = y - iy;
  const ux = fx * fx * (3 - 2 * fx), uy = fy * fy * (3 - 2 * fy);
  const a = hash2(ix, iy), b = hash2(ix + 1, iy), c = hash2(ix, iy + 1), d = hash2(ix + 1, iy + 1);
  return a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy;
}

// ── Scrambled text (pure timing, DOM reveal) ─────────────────────────────────

/**
 * The text as it reads when a reveal is `t` of the way (0..1): characters
 * settle from left to right, each at its own moment, and until then show a
 * glyph that changes every frame. Spaces and line breaks never scramble.
 * `settle` is how much of the reveal the settling overlaps (0 = strict order).
 */
export function scrambleAt(text: string, t: number, rnd: () => number = Math.random, settle = 0.55): string {
  const chars = [...text];
  const n = Math.max(1, chars.length);
  let out = "";
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i];
    if (!c.trim()) { out += c; continue; }
    // Golden-ratio stagger: neighbours settle at unrelated moments.
    const at = (i / n) * (1 - settle) + settle * (0.3 + 0.7 * fract(i * 0.6180339887));
    out += t >= at ? c : GLYPHS[Math.floor(rnd() * GLYPHS.length)];
  }
  return out;
}

/** Reveal `text` in `el` with a scramble over `ms`. Returns a cancel function. */
export function scrambleIn(el: HTMLElement, text: string, ms = 700): () => void {
  el.dataset.text = text;
  if (calm() || ms <= 0) { el.textContent = text; return () => {}; }
  const t0 = performance.now();
  let raf = 0;
  const frame = (now: number) => {
    const t = Math.min(1, (now - t0) / ms);
    el.textContent = scrambleAt(text, t);
    if (t < 1) raf = requestAnimationFrame(frame);
  };
  el.textContent = scrambleAt(text, 0);
  raf = requestAnimationFrame(frame);
  return () => { cancelAnimationFrame(raf); el.textContent = text; };
}

/**
 * Letters near the pointer scramble and settle again, like react-bits'
 * Scrambled Text: the closer the pointer, the longer a letter takes to settle.
 * The element's text is split into one span per character once.
 */
export function hoverScramble(el: HTMLElement, radius = 70, maxMs = 900): () => void {
  if (calm()) return () => {};
  const text = el.textContent ?? "";
  const spans: HTMLSpanElement[] = [];
  const frag = document.createDocumentFragment();
  for (const c of text) {
    if (c === "\n") { frag.appendChild(document.createElement("br")); continue; }
    const s = document.createElement("span");
    s.textContent = c;
    s.dataset.c = c;
    frag.appendChild(s);
    spans.push(s);
  }
  el.replaceChildren(frag);
  const until = new Float64Array(spans.length);
  let raf = 0;
  const tick = () => {
    const now = performance.now();
    let live = false;
    for (let i = 0; i < spans.length; i++) {
      const s = spans[i];
      const c = s.dataset.c ?? "";
      if (until[i] > now && c.trim()) { s.textContent = GLYPHS[Math.floor(Math.random() * GLYPHS.length)]; live = true; }
      else if (s.textContent !== c) s.textContent = c;
    }
    raf = live ? requestAnimationFrame(tick) : 0;
  };
  const move = (e: PointerEvent) => {
    const now = performance.now();
    for (let i = 0; i < spans.length; i++) {
      const r = spans[i].getBoundingClientRect();
      const d = Math.hypot(e.clientX - (r.left + r.width / 2), e.clientY - (r.top + r.height / 2));
      if (d < radius) until[i] = Math.max(until[i], now + maxMs * (1 - d / radius));
    }
    if (!raf) raf = requestAnimationFrame(tick);
  };
  el.addEventListener("pointermove", move);
  return () => { el.removeEventListener("pointermove", move); cancelAnimationFrame(raf); el.textContent = text; };
}

// ── Lattice loader (pure patterns, DOM element) ──────────────────────────────

export type LatticeName = "orbit" | "sweep" | "pulse" | "snake";
export interface LatticePattern { n: 3 | 4; cells: (number | null)[]; loop: number; scale: number }
const LATTICE: Record<LatticeName, LatticePattern> = {
  orbit: { n: 3, cells: [0, 1, 2, 7, null, 3, 6, 5, 4], loop: 8, scale: 1.2 },
  sweep: { n: 4, cells: [0, 1, 2, 3, 1, 2, 3, 4, 2, 3, 4, 5, 3, 4, 5, 6], loop: 5, scale: 1 },
  pulse: { n: 4, cells: [2, 1, 1, 2, 1, 0, 0, 1, 1, 0, 0, 1, 2, 1, 1, 2], loop: 2.4, scale: 2.5 },
  snake: { n: 4, cells: [0, 1, 2, 3, 7, 6, 5, 4, 8, 9, 10, 11, 15, 14, 13, 12], loop: 16, scale: 1 },
};
/** Per-cell animation delays (ms) and the cycle length for a pattern at `step` ms a unit. */
export function latticeTiming(name: LatticeName, step = 90): { n: number; delays: (number | null)[]; cycleMs: number } {
  const p = LATTICE[name];
  const d = step * p.scale;
  return { n: p.n, delays: p.cells.map((u) => (u == null ? null : Math.round(u * d))), cycleMs: Math.round(p.loop * d) };
}

/** A lattice of cells that brighten in a wave, with a label and a timer. */
export function lattice(name: LatticeName, label: string): { el: HTMLElement; label: HTMLElement; timer: HTMLElement; done(): void } {
  const t = latticeTiming(name);
  const el = document.createElement("span");
  el.className = "lattice";
  el.setAttribute("role", "status");
  el.style.setProperty("--ll-n", String(t.n));
  el.style.setProperty("--ll-cycle", `${t.cycleMs}ms`);
  const grid = document.createElement("span");
  grid.className = "lattice-grid";
  grid.setAttribute("aria-hidden", "true");
  for (const d of t.delays) {
    const c = document.createElement("span");
    c.className = "lattice-cell";
    if (d == null) c.dataset.hole = "";
    else c.style.animationDelay = `${d}ms`;
    grid.appendChild(c);
  }
  const lab = document.createElement("span");
  lab.className = "lattice-label";
  lab.textContent = label;
  const timer = document.createElement("span");
  timer.className = "lattice-timer";
  timer.textContent = "0.0s";
  el.append(grid, lab, timer);
  return { el, label: lab, timer, done: () => { el.dataset.status = "done"; } };
}

// ── Shapes: coverage rasters and their edges (pure tracing) ──────────────────

export interface Shape {
  /** Edge points, x0 y0 x1 y1 ... in raster pixels. */
  pts: Float32Array;
  /** Outward unit normals, one pair per point. */
  nrm: Float32Array;
  w: number;
  h: number;
  /** Grid for neighbour lookups: cell size and the point indices per cell. */
  cell: number;
  cols: number;
  rows: number;
  bins: Int32Array[];
  /** Where strands start (raster x, y pairs): spread over every contour, so a
   *  small one like an eye is not left dark. Optional; evenly picked otherwise. */
  anchors?: number[];
}

/**
 * The outline of a coverage raster (0..1 per pixel): every covered pixel with an
 * uncovered 4-neighbour is an edge point, with its normal from the coverage
 * gradient, pointing out. `limit` thins the points evenly when there are more.
 */
export function traceEdges(cov: Float32Array, w: number, h: number, limit = 2400): Shape {
  const xs: number[] = [];
  const ys: number[] = [];
  const nx: number[] = [];
  const ny: number[] = [];
  const at = (x: number, y: number) => (x < 0 || y < 0 || x >= w || y >= h ? 0 : cov[y * w + x]);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (cov[y * w + x] <= 0.5) continue;
      if (at(x - 1, y) > 0.5 && at(x + 1, y) > 0.5 && at(x, y - 1) > 0.5 && at(x, y + 1) > 0.5) continue;
      let gx = at(x + 1, y) - at(x - 1, y);
      let gy = at(x, y + 1) - at(x, y - 1);
      const len = Math.hypot(gx, gy) || 1;
      gx = -gx / len; gy = -gy / len; // from high coverage to low: outward
      xs.push(x + 0.5); ys.push(y + 0.5); nx.push(gx); ny.push(gy);
    }
  }
  const n = xs.length;
  const stride = Math.max(1, Math.ceil(n / limit));
  const m = Math.ceil(n / stride);
  const pts = new Float32Array(m * 2);
  const nrm = new Float32Array(m * 2);
  for (let i = 0, j = 0; i < n; i += stride, j++) {
    pts[j * 2] = xs[i]; pts[j * 2 + 1] = ys[i];
    nrm[j * 2] = nx[i]; nrm[j * 2 + 1] = ny[i];
  }
  const cell = Math.max(4, Math.round(Math.max(w, h) / 40));
  const cols = Math.ceil(w / cell) + 1;
  const rows = Math.ceil(h / cell) + 1;
  const lists: number[][] = Array.from({ length: cols * rows }, () => []);
  for (let i = 0; i < m; i++) {
    const c = Math.min(cols - 1, Math.floor(pts[i * 2] / cell));
    const r = Math.min(rows - 1, Math.floor(pts[i * 2 + 1] / cell));
    lists[r * cols + c].push(i);
  }
  return { pts, nrm, w, h, cell, cols, rows, bins: lists.map((l) => Int32Array.from(l)) };
}

/** Index of the edge point nearest (x, y), searching outward ring by ring. */
export function nearest(s: Shape, x: number, y: number): number {
  const c0 = Math.min(s.cols - 1, Math.max(0, Math.floor(x / s.cell)));
  const r0 = Math.min(s.rows - 1, Math.max(0, Math.floor(y / s.cell)));
  let best = -1;
  let bd = Infinity;
  for (let ring = 0; ring < Math.max(s.cols, s.rows); ring++) {
    for (let r = r0 - ring; r <= r0 + ring; r++) {
      if (r < 0 || r >= s.rows) continue;
      for (let c = c0 - ring; c <= c0 + ring; c++) {
        if (c < 0 || c >= s.cols) continue;
        if (Math.abs(r - r0) !== ring && Math.abs(c - c0) !== ring) continue;
        for (const i of s.bins[r * s.cols + c]) {
          const d = (s.pts[i * 2] - x) ** 2 + (s.pts[i * 2 + 1] - y) ** 2;
          if (d < bd) { bd = d; best = i; }
        }
      }
    }
    // Everything nearer than the ring's inner edge has been seen.
    if (best >= 0 && Math.sqrt(bd) <= ring * s.cell) break;
  }
  return best;
}

/** The next edge point to walk to from point `i` heading (dx, dy): a neighbour
 *  within `reach` that keeps the direction best, with a little chance. */
export function stepAlong(s: Shape, i: number, dx: number, dy: number, reach: number, rnd: () => number): number {
  const x = s.pts[i * 2], y = s.pts[i * 2 + 1];
  const c0 = Math.floor(x / s.cell), r0 = Math.floor(y / s.cell);
  const span = Math.ceil(reach / s.cell);
  let best = -1;
  let bs = -Infinity;
  for (let r = r0 - span; r <= r0 + span; r++) {
    if (r < 0 || r >= s.rows) continue;
    for (let c = c0 - span; c <= c0 + span; c++) {
      if (c < 0 || c >= s.cols) continue;
      for (const j of s.bins[r * s.cols + c]) {
        if (j === i) continue;
        const ex = s.pts[j * 2] - x, ey = s.pts[j * 2 + 1] - y;
        const d = Math.hypot(ex, ey);
        if (d < reach * 0.35 || d > reach) continue;
        const score = (ex * dx + ey * dy) / d + rnd() * 0.6;
        if (score > bs) { bs = score; best = j; }
      }
    }
  }
  return best;
}

// Rasters are made at a working resolution; the stage scales them to fit.
export const RASTER = 420;

function rasterOf(draw: (ctx: CanvasRenderingContext2D, w: number, h: number) => void, w: number, h: number): Float32Array {
  const c = document.createElement("canvas");
  c.width = w; c.height = h;
  const ctx = c.getContext("2d", { willReadFrequently: true })!;
  ctx.fillStyle = "#fff";
  draw(ctx, w, h);
  const d = ctx.getImageData(0, 0, w, h).data;
  const cov = new Float32Array(w * h);
  for (let i = 0; i < w * h; i++) cov[i] = d[i * 4 + 3] / 255;
  return cov;
}

/** The BTX mark: three bold monospace letters. There is no official BTX logo. */
export function shapeBTX(font: string): Shape {
  const w = RASTER, h = Math.round(RASTER * 0.46);
  const s = traceEdges(rasterOf((ctx) => {
    ctx.font = `700 ${Math.round(h * 0.78)}px ${font}`;
    ctx.textAlign = "center"; ctx.textBaseline = "middle";
    ctx.fillText("BTX", w / 2, h / 2 + h * 0.02);
  }, w, h), w, h);
  s.anchors = [0.2, 0.2, 0.5, 0.5, 0.8, 0.8, 0.35, 0.65].flatMap((x, i) => [w * x, h * (i % 2 ? 0.25 : 0.75)]);
  return s;
}

/** The face: a head with two eyes, the silhouette Mende drew. Eyes are holes,
 *  so their outlines carry strands too. */
export function shapeFace(): Shape {
  const w = Math.round(RASTER * 0.78), h = RASTER;
  const s = traceEdges(rasterOf((ctx) => {
    // Each ellipse is its own subpath: without the moveTo, the canvas joins
    // them with hairlines, and the tracer found those as edges (a straight
    // line from eye to eye and on to the cheek).
    ctx.beginPath();
    ctx.moveTo(w / 2 + w * 0.47, h / 2);
    ctx.ellipse(w / 2, h / 2, w * 0.47, h * 0.47, 0, 0, Math.PI * 2);
    ctx.moveTo(w * 0.3 + w * 0.165, h * 0.38);
    ctx.ellipse(w * 0.3, h * 0.38, w * 0.165, h * 0.092, 0, 0, Math.PI * 2);
    ctx.moveTo(w * 0.7 + w * 0.165, h * 0.38);
    ctx.ellipse(w * 0.7, h * 0.38, w * 0.165, h * 0.092, 0, 0, Math.PI * 2);
    ctx.fill("evenodd");
  }, w, h), w, h);
  // One strand on each eye (its outline is short, so one lights it whole), the
  // rest around the head.
  s.anchors = [w * 0.3, h * 0.288, w * 0.7, h * 0.288, w * 0.5, h * 0.03, w * 0.5, h * 0.97, w * 0.03, h * 0.5, w * 0.97, h * 0.5, w * 0.15, h * 0.2, w * 0.85, h * 0.8];
  return s;
}

// ── The letter field (Canvas 2D) ─────────────────────────────────────────────

export type FieldMode = "storm" | "calm";
interface FieldSettings { fontSize: number; density: number; baseOpacity: number; glitch: number; speed: number; centerFade: number; walkers: number }
const FIELD: Record<FieldMode, FieldSettings> = {
  storm: { fontSize: 13, density: 0.92, baseOpacity: 0.26, glitch: 1, speed: 1.5, centerFade: 0.35, walkers: 18 },
  calm: { fontSize: 13, density: 0.9, baseOpacity: 0.09, glitch: 0.12, speed: 0.5, centerFade: 0.9, walkers: 2 },
};
interface Walker { x: number; y: number; a: number; life: number; v: number; tint: number }

/**
 * A field of monospace glyphs that flicker and glitch, torn now and then:
 * react-bits' Letter Glitch in Canvas 2D. `storm` while the model loads,
 * `calm` once it is ready (the middle clears for the face).
 */
export class LetterField {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private mode: FieldMode = "storm";
  private cur: FieldSettings = { ...FIELD.storm };
  private atlases: HTMLCanvasElement[] = [];
  private cols = 0; private rows = 0; private cw = 0; private ch = 0; private dpr = 1;
  private glyph = new Uint8Array(0); private level = new Float32Array(0); private luck = new Float32Array(0);
  private heat = new Float32Array(0); private hold = new Float32Array(0); private tint = new Uint8Array(0);
  private born = new Float32Array(0); private mask = new Float32Array(0);
  private walkers: Walker[] = [];
  private raf = 0; private last = 0; private time = 0; private tearUntil = 0; private tearSeed = 0;
  private pool: number[] = [];
  private ro: ResizeObserver | null = null;
  private colors: string[];
  private font: string;
  private alive = true;

  constructor(private host: HTMLElement, colors: { base: string; hot: string; mid: string }, font: string) {
    this.colors = [colors.base, colors.hot, colors.mid];
    this.font = font;
    this.canvas = document.createElement("canvas");
    this.canvas.className = "fx-field";
    this.ctx = this.canvas.getContext("2d", { alpha: true })!;
    host.appendChild(this.canvas);
    this.ro = new ResizeObserver(() => this.layout());
    this.ro.observe(host);
    this.layout();
    this.raf = requestAnimationFrame((t) => this.frame(t));
  }

  setMode(m: FieldMode) { this.mode = m; }
  /** Horizontal bands slip sideways for `ms`: the "danger" tear. */
  tear(ms = 180) { this.tearUntil = this.time + ms / 1000; this.tearSeed = Math.random() * 1000; }

  destroy() {
    this.alive = false;
    cancelAnimationFrame(this.raf);
    this.ro?.disconnect();
    this.canvas.remove();
  }

  private buildAtlases() {
    const s = this.cur;
    this.dpr = Math.min(2, window.devicePixelRatio || 1);
    const size = s.fontSize * this.dpr;
    const probe = document.createElement("canvas").getContext("2d")!;
    probe.font = `${size}px ${this.font}`;
    let adv = 1;
    for (const c of GLYPHS) adv = Math.max(adv, probe.measureText(c).width);
    this.cw = Math.ceil(adv + size * 0.55);
    this.ch = Math.ceil(size * 1.5);
    this.atlases = this.colors.map((color) => {
      const a = document.createElement("canvas");
      a.width = this.cw * GLYPHS.length; a.height = this.ch;
      const ctx = a.getContext("2d")!;
      ctx.font = `${size}px ${this.font}`;
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.fillStyle = color;
      [...GLYPHS].forEach((g, i) => ctx.fillText(g, i * this.cw + this.cw / 2, this.ch / 2));
      return a;
    });
    this.pool = [...GLYPHS].map((_, i) => i);
  }

  private layout() {
    if (!this.alive) return;
    const r = this.host.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return;
    this.buildAtlases();
    this.canvas.width = Math.round(r.width * this.dpr);
    this.canvas.height = Math.round(r.height * this.dpr);
    this.canvas.style.width = `${r.width}px`;
    this.canvas.style.height = `${r.height}px`;
    this.cols = Math.ceil(this.canvas.width / this.cw) + 1;
    this.rows = Math.ceil(this.canvas.height / this.ch) + 1;
    const n = this.cols * this.rows;
    this.glyph = new Uint8Array(n); this.level = new Float32Array(n); this.luck = new Float32Array(n);
    this.heat = new Float32Array(n); this.hold = new Float32Array(n); this.tint = new Uint8Array(n);
    this.born = new Float32Array(n); this.mask = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      this.glyph[i] = this.pool[(Math.random() * this.pool.length) | 0];
      this.level[i] = 0.55 + 0.45 * Math.random();
      this.luck[i] = Math.random();
      const c = i % this.cols, rr = (i / this.cols) | 0;
      const dx = (c + 0.5) / this.cols - 0.5, dy = (rr + 0.5) / this.rows - 0.5;
      // The intro: letters appear from the middle out over 1.4 s.
      this.born[i] = this.time + 1.4 * (0.3 * Math.min(1, Math.hypot(dx, dy * 0.8) * 1.6) + 0.7 * Math.random() ** 1.3);
      // Edge vignette: the field fades at the frame.
      const ex = Math.min(c + 0.5, this.cols - c - 0.5) / (this.cols * 0.12);
      const ey = Math.min(rr + 0.5, this.rows - rr - 0.5) / (this.rows * 0.12);
      this.mask[i] = Math.min(1, ex) * Math.min(1, ey);
    }
    this.walkers = [];
  }

  private centerMask(c: number, r: number, amount: number): number {
    if (amount <= 0) return 1;
    const dx = Math.abs((c + 0.5) / this.cols - 0.5) / 0.34;
    const dy = Math.abs((r + 0.5) / this.rows - 0.5) / 0.4;
    const d = Math.hypot(dx, dy);
    const inside = d < 1 ? 1 - d * d : 0;
    return 1 - amount * inside;
  }

  private ignite(i: number, strength: number, hold: number, tint: number) {
    this.heat[i] = Math.max(this.heat[i], strength);
    this.hold[i] = Math.max(this.hold[i], hold);
    this.tint[i] = tint;
  }

  private frame(now: number) {
    if (!this.alive) return;
    this.raf = requestAnimationFrame((t) => this.frame(t));
    const dt = Math.min(0.05, this.last ? (now - this.last) / 1000 : 0.016);
    this.last = now;
    this.time += dt;
    // Settings glide between modes, so "ready" is a change of weather, not a cut.
    const want = FIELD[this.mode];
    const k = Math.min(1, dt * 1.6);
    for (const key of Object.keys(want) as (keyof FieldSettings)[]) this.cur[key] += (want[key] - this.cur[key]) * k;
    const s = this.cur;
    const n = this.cols * this.rows;
    if (!n) return;
    // Rest changes: a share of cells swap their glyph.
    let swaps = n * 0.22 * s.speed * dt;
    while (swaps > 0) { if (Math.random() < swaps) { const i = (Math.random() * n) | 0; this.glyph[i] = this.pool[(Math.random() * this.pool.length) | 0]; this.level[i] = 0.55 + 0.45 * Math.random(); } swaps -= 1; }
    // Glitches: cells that flare in a tint and cool down.
    let flares = n * 0.12 * s.glitch * s.speed * dt;
    while (flares > 0) { if (Math.random() < flares) this.ignite((Math.random() * n) | 0, 0.7 + 0.3 * Math.random(), 0.05 + Math.random() * 0.25, Math.random() < 0.35 ? 1 : 2); flares -= 1; }
    // Walkers: strands that run across the grid lighting cells as they go.
    if (this.walkers.length < s.walkers && Math.random() < dt * s.walkers * 0.6) {
      const a = Math.random() < 0.6 ? (Math.random() < 0.5 ? 0 : Math.PI) + (Math.random() - 0.5) * 0.4 : Math.PI / 2 * (Math.random() < 0.5 ? 1 : 3);
      this.walkers.push({ x: Math.random() * this.cols, y: Math.random() * this.rows, a, life: 0.4 + Math.random() * 1.2, v: 14 + Math.random() * 30, tint: Math.random() < 0.5 ? 1 : 2 });
    }
    for (let w = this.walkers.length - 1; w >= 0; w--) {
      const k2 = this.walkers[w];
      k2.life -= dt;
      k2.a += (Math.random() - 0.5) * 0.8 * dt * 10;
      k2.x += Math.cos(k2.a) * k2.v * dt;
      k2.y += Math.sin(k2.a) * k2.v * dt * 0.6;
      const c = k2.x | 0, r = k2.y | 0;
      if (k2.life <= 0 || c < 0 || r < 0 || c >= this.cols || r >= this.rows) { this.walkers.splice(w, 1); continue; }
      this.ignite(r * this.cols + c, 1, 0.08, k2.tint);
    }
    // Cooling.
    for (let i = 0; i < n; i++) {
      if (this.heat[i] <= 0) continue;
      if (this.hold[i] > 0) this.hold[i] -= dt;
      else this.heat[i] = Math.max(0, this.heat[i] - dt * 2.4);
    }
    this.draw();
  }

  private draw() {
    const ctx = this.ctx;
    const W = this.canvas.width, H = this.canvas.height;
    ctx.clearRect(0, 0, W, H);
    const s = this.cur;
    const tearing = this.time < this.tearUntil;
    const t = this.time;
    const n = this.cols * this.rows;
    const flowScale = 9;
    for (let i = 0; i < n; i++) {
      const c = i % this.cols, r = (i / this.cols) | 0;
      let shift = 0;
      if (tearing) {
        const band = Math.floor(r / 3) + this.tearSeed;
        if (hash2(band, 1) > 0.55) shift = Math.round((hash2(band, 2) - 0.5) * 18 * this.cw);
      }
      const x = c * this.cw + shift;
      const y = r * this.ch;
      let rest = 0;
      if (this.luck[i] < s.density) {
        const appear = Math.min(1, Math.max(0, (t - this.born[i]) * 3));
        const seed = hash2(c, r);
        rest = this.level[i] * s.baseOpacity * appear * (0.8 + 0.2 * Math.sin(t * (0.5 + 1.2 * seed) + seed * 40));
        const nz = noise2(c / flowScale + t * 0.08, r / flowScale * 1.6 + t * 0.05);
        rest *= 0.25 + 1.5 * Math.max(0, Math.min(1, (nz - 0.3) / 0.5));
      }
      const heat = this.heat[i];
      const mask = this.mask[i] * this.centerMask(c, r, s.centerFade);
      const base = Math.min(1, rest) * (1 - heat) * mask;
      const g = this.glyph[i];
      if (base > 0.02) {
        ctx.globalAlpha = base;
        ctx.drawImage(this.atlases[0], g * this.cw, 0, this.cw, this.ch, x, y, this.cw, this.ch);
      }
      if (heat > 0.02) {
        ctx.globalAlpha = Math.min(1, heat * mask * 1.3);
        ctx.drawImage(this.atlases[this.tint[i] || 1], g * this.cw, 0, this.cw, this.ch, x, y, this.cw, this.ch);
      }
    }
    ctx.globalAlpha = 1;
  }
}

// ── The electric mark (Canvas 2D) ────────────────────────────────────────────

/** Coverage (0 or 1 per pixel) of an RGBA buffer: bright pixels count. Green
 *  weighs most: the faces are green light on black. */
export function coverageFromLuma(px: Uint8ClampedArray, w: number, h: number, threshold = 110): Float32Array {
  const cov = new Float32Array(w * h);
  for (let i = 0; i < w * h; i++) {
    const luma = 0.25 * px[i * 4] + 0.6 * px[i * 4 + 1] + 0.15 * px[i * 4 + 2];
    cov[i] = luma >= threshold ? 1 : 0;
  }
  return cov;
}

/** The bright edges of a picture (a face in green light), as a shape the mark
 *  can run its strands along. `anchors` in 0..1 of the picture, optional. */
export function shapeFromImage(img: HTMLImageElement, anchors: number[] = [], threshold = 110): Shape {
  const iw = img.naturalWidth || img.width, ih = img.naturalHeight || img.height;
  const scale = RASTER / Math.max(iw, ih);
  const w = Math.max(2, Math.round(iw * scale)), h = Math.max(2, Math.round(ih * scale));
  const c = document.createElement("canvas");
  c.width = w; c.height = h;
  const ctx = c.getContext("2d", { willReadFrequently: true })!;
  ctx.drawImage(img, 0, 0, w, h);
  const cov = coverageFromLuma(ctx.getImageData(0, 0, w, h).data, w, h, threshold);
  const s = traceEdges(cov, w, h, 2400);
  s.anchors = anchors.map((v, i) => v * (i % 2 ? h : w));
  return s;
}

export type MarkKind = string;
interface Strand { head: number; dx: number; dy: number; trail: Float32Array; len: number; seed: number }
interface Spark { pts: Float32Array; born: number; life: number; width: number }
/** Where a shape sits in the host, as fractions of its size: box width and
 *  height, and the vertical centre. */
export interface Fit { w: number; h: number; cy: number }

/**
 * Lightning strands that run along a shape's outline, sparks that leap off its
 * edges, and a morph from one shape to the next: react-bits' Electric Logo,
 * drawn with strokes. Shapes are added by name with the box they fit in.
 */
export class ElectricMark {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private shapes = new Map<MarkKind, Shape>();
  private fits = new Map<MarkKind, Fit>();
  private kind: MarkKind;
  private prev: MarkKind | null = null;
  private morph = 1; // 0..1 while changing shape
  private strands: Strand[] = [];
  private from: Float32Array[] = []; // strand trails frozen at the morph's start
  private sparks: Spark[] = [];
  private raf = 0; private last = 0; private time = 0; private flash = 0; private blinkAt = 4;
  private ro: ResizeObserver | null = null;
  private alive = true;
  private outlines = new Map<MarkKind, Path2D>();
  private dpr = 1;
  /** 1 while loading; the face hums lower. Glides towards `energyTarget`. */
  private energy = 1;
  public energyTarget = 1;
  public intensity = 1;
  /** Called after each layout with the box of the current shape (CSS px). */
  public onLayout: ((box: { x: number; y: number; w: number; h: number }, kind: MarkKind) => void) | null = null;

  constructor(private host: HTMLElement, kind: MarkKind, shape: Shape, fit: Fit, private colors: { core: string; glow: string; ink: string }) {
    this.shapes.set(kind, shape);
    this.fits.set(kind, fit);
    this.kind = kind;
    this.canvas = document.createElement("canvas");
    this.canvas.className = "fx-stage";
    this.ctx = this.canvas.getContext("2d")!;
    host.appendChild(this.canvas);
    this.ro = new ResizeObserver(() => this.layout());
    this.ro.observe(host);
    this.layout();
    this.seedStrands();
    this.raf = requestAnimationFrame((t) => this.frame(t));
  }

  destroy() {
    this.alive = false;
    cancelAnimationFrame(this.raf);
    this.ro?.disconnect();
    this.canvas.remove();
  }

  addShape(kind: MarkKind, shape: Shape, fit: Fit) {
    this.shapes.set(kind, shape);
    this.fits.set(kind, fit);
    this.outlines.delete(kind);
  }

  /** The box the shape `kind` is drawn in, in CSS px of the host. */
  boxOf(kind: MarkKind = this.kind) {
    const f = this.fitFor(kind);
    const s = this.shapes.get(kind)!;
    return { x: f.ox, y: f.oy, w: s.w * f.scale, h: s.h * f.scale };
  }

  /** Change shape with a morph (strands fly to the new outline). */
  setKind(kind: MarkKind) {
    if (kind === this.kind || !this.shapes.has(kind)) return;
    this.prev = this.kind;
    this.kind = kind;
    this.morph = 0;
    this.from = this.strands.map((s) => s.trail.slice());
    const shape = this.shapes.get(kind)!;
    const f = this.fitFor(kind);
    this.strands.forEach((s, k) => {
      // The head goes to its anchor on the new shape; every trail point moves to
      // the outline point nearest it, so nothing is left stretched across the
      // picture once the morph ends.
      s.head = this.startOf(shape, k, this.strands.length);
      for (let i = 0; i < s.len; i++) {
        const j = nearest(shape, s.trail[i * 2] / f.scale, s.trail[i * 2 + 1] / f.scale);
        if (j < 0) continue;
        s.trail[i * 2] = shape.pts[j * 2] * f.scale; s.trail[i * 2 + 1] = shape.pts[j * 2 + 1] * f.scale;
      }
      const ang = Math.random() * Math.PI * 2;
      s.dx = Math.cos(ang); s.dy = Math.sin(ang);
    });
    this.flash = 1;
    this.burst(8);
    this.onLayout?.(this.boxOf(kind), kind);
  }

  /** A moment of danger: a white flash and a burst of sparks. */
  surge() { this.flash = 1; this.burst(5); }

  private layout() {
    if (!this.alive) return;
    const r = this.host.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return;
    this.dpr = Math.min(2, window.devicePixelRatio || 1);
    this.canvas.width = Math.round(r.width * this.dpr);
    this.canvas.height = Math.round(r.height * this.dpr);
    this.canvas.style.width = `${r.width}px`;
    this.canvas.style.height = `${r.height}px`;
    this.outlines.clear();
    this.onLayout?.(this.boxOf(this.kind), this.kind);
  }

  private fitFor(kind: MarkKind) {
    const s = this.shapes.get(kind)!;
    const fit = this.fits.get(kind)!;
    const W = this.canvas.width / this.dpr, H = this.canvas.height / this.dpr;
    const scale = Math.min((W * fit.w) / s.w, (H * fit.h) / s.h);
    return { scale, ox: (W - s.w * scale) / 2, oy: H * fit.cy - (s.h * scale) / 2, size: Math.max(s.w, s.h) * scale };
  }

  /** The edge point strand `k` starts from: its anchor, else an even spread. */
  private startOf(shape: Shape, k: number, count: number): number {
    const a = shape.anchors;
    if (a && a.length >= 2) {
      const i = (k % (a.length / 2)) * 2;
      return Math.max(0, nearest(shape, a[i], a[i + 1]));
    }
    return Math.floor((k / count) * (shape.pts.length / 2));
  }

  private seedStrands() {
    const shape = this.shapes.get(this.kind)!;
    const count = 8;
    const len = 60;
    this.strands = Array.from({ length: count }, (_, k) => {
      const head = this.startOf(shape, k, count);
      const ang = Math.random() * Math.PI * 2;
      const trail = new Float32Array(len * 2);
      const f = this.fitFor(this.kind);
      for (let i = 0; i < len; i++) { trail[i * 2] = shape.pts[head * 2] * f.scale; trail[i * 2 + 1] = shape.pts[head * 2 + 1] * f.scale; }
      return { head, dx: Math.cos(ang), dy: Math.sin(ang), trail, len, seed: Math.random() * 100 };
    });
  }

  private burst(n: number) { for (let i = 0; i < n; i++) this.spark(); }

  private spark() {
    const shape = this.shapes.get(this.kind)!;
    const count = shape.pts.length / 2;
    if (count < 2) return;
    const i = (Math.random() * count) | 0;
    const f = this.fitFor(this.kind);
    const ax = shape.pts[i * 2] * f.scale, ay = shape.pts[i * 2 + 1] * f.scale;
    const nx = shape.nrm[i * 2], ny = shape.nrm[i * 2 + 1];
    // Half the sparks leap outward along the normal; the rest arc to another
    // edge point not too near and not too far, bowed off the shape.
    let bx: number, by: number;
    if (Math.random() < 0.55) {
      const reach = f.size * (0.08 + Math.random() * 0.22);
      const spin = (Math.random() - 0.5) * 1.2;
      bx = ax + (nx * Math.cos(spin) - ny * Math.sin(spin)) * reach;
      by = ay + (nx * Math.sin(spin) + ny * Math.cos(spin)) * reach;
    } else {
      let j = -1;
      for (let tries = 0; tries < 20 && j < 0; tries++) {
        const k = (Math.random() * count) | 0;
        const d = Math.hypot(shape.pts[k * 2] - shape.pts[i * 2], shape.pts[k * 2 + 1] - shape.pts[i * 2 + 1]) * f.scale;
        if (d > f.size * 0.08 && d < f.size * 0.3) j = k;
      }
      if (j < 0) return;
      bx = shape.pts[j * 2] * f.scale; by = shape.pts[j * 2 + 1] * f.scale;
    }
    // Midpoint displacement makes the bolt jagged.
    let pts = [ax, ay, bx, by];
    const len = Math.hypot(bx - ax, by - ay);
    let amp = len * 0.28;
    for (let level = 0; level < 4; level++) {
      const next: number[] = [];
      for (let p = 0; p < pts.length - 2; p += 2) {
        const x0 = pts[p], y0 = pts[p + 1], x1 = pts[p + 2], y1 = pts[p + 3];
        const mx = (x0 + x1) / 2, my = (y0 + y1) / 2;
        const dx = x1 - x0, dy = y1 - y0;
        const dl = Math.hypot(dx, dy) || 1;
        const off = (Math.random() - 0.5) * 2 * amp;
        next.push(x0, y0, mx - (dy / dl) * off, my + (dx / dl) * off);
      }
      next.push(bx, by);
      pts = next;
      amp *= 0.5;
    }
    this.sparks.push({ pts: Float32Array.from(pts), born: this.time, life: 0.25 + Math.random() * 0.45, width: 0.8 + Math.random() * 1.2 });
  }

  private frame(now: number) {
    if (!this.alive) return;
    this.raf = requestAnimationFrame((t) => this.frame(t));
    const dt = Math.min(0.05, this.last ? (now - this.last) / 1000 : 0.016);
    this.last = now;
    this.time += dt;
    const shape = this.shapes.get(this.kind)!;
    const f = this.fitFor(this.kind);
    if (this.morph < 1) this.morph = Math.min(1, this.morph + dt / 0.9);
    this.flash = Math.max(0, this.flash - dt * 2.5);
    this.energy += (this.energyTarget - this.energy) * Math.min(1, dt * 0.8);
    // Strands walk the outline; each frame the head moves on and the trail follows.
    const reach = Math.max(3, shape.cell * 1.2);
    const speed = (this.morph < 1 ? 2 : 1) * (0.6 + this.energy) * this.intensity;
    for (const s of this.strands) {
      let steps = speed * dt * 120;
      while (steps > 0) {
        if (Math.random() >= Math.min(1, steps)) break;
        steps -= 1;
        const j = stepAlong(shape, s.head, s.dx, s.dy, reach, Math.random);
        if (j < 0) { s.dx = -s.dx; s.dy = -s.dy; continue; }
        const ex = shape.pts[j * 2] - shape.pts[s.head * 2], ey = shape.pts[j * 2 + 1] - shape.pts[s.head * 2 + 1];
        const d = Math.hypot(ex, ey) || 1;
        s.dx = s.dx * 0.6 + (ex / d) * 0.4; s.dy = s.dy * 0.6 + (ey / d) * 0.4;
        const n2 = Math.hypot(s.dx, s.dy) || 1; s.dx /= n2; s.dy /= n2;
        s.head = j;
        s.trail.copyWithin(2, 0, (s.len - 1) * 2);
        s.trail[0] = shape.pts[j * 2] * f.scale; s.trail[1] = shape.pts[j * 2 + 1] * f.scale;
      }
    }
    // Sparks.
    const rate = (1.1 + 2.1 * this.energy) * this.intensity + this.flash * 6;
    if (Math.random() < rate * dt) this.spark();
    this.sparks = this.sparks.filter((sp) => this.time - sp.born < sp.life);
    // Settled: a short surge every few seconds, like a blink.
    if (this.morph >= 1 && this.energy < 0.8 && this.time > this.blinkAt) { this.blinkAt = this.time + 3 + Math.random() * 5; this.flash = 0.5; }
    this.draw(f);
  }

  private pathOf(kind: MarkKind, f: { scale: number }): Path2D {
    const hit = this.outlines.get(kind);
    if (hit) return hit;
    const s = this.shapes.get(kind)!;
    const p = new Path2D();
    const r = Math.max(0.8, 1.1 * f.scale);
    for (let i = 0; i < s.pts.length; i += 2) p.rect(s.pts[i] * f.scale - r / 2, s.pts[i + 1] * f.scale - r / 2, r, r);
    this.outlines.set(kind, p);
    return p;
  }

  private draw(f: { scale: number; ox: number; oy: number; size: number }) {
    const ctx = this.ctx;
    const W = this.canvas.width, H = this.canvas.height;
    ctx.clearRect(0, 0, W, H);
    ctx.save();
    ctx.scale(this.dpr, this.dpr);
    ctx.globalCompositeOperation = "lighter";
    ctx.lineCap = "round"; ctx.lineJoin = "round";
    const t = this.time;
    const flicker = 1 - 0.18 * (0.5 + 0.5 * Math.sin(t * 23) * Math.sin(t * 7.3));
    const glowBase = (0.5 + 0.5 * this.energy) * this.intensity * flicker;
    const m = this.morph;
    // The outline itself: faint ink dots, the previous shape fading out while
    // the new one glitches in (rows of it slip sideways during the morph). On a
    // settled face the ink fades with the energy: the picture speaks for itself.
    const inkFor = (kind: MarkKind, alpha: number) => {
      if (alpha <= 0.01) return;
      const ff = this.fitFor(kind);
      ctx.save();
      ctx.translate(ff.ox, ff.oy);
      if (m < 1) {
        const slip = (Math.random() - 0.5) * 14 * (1 - m);
        ctx.translate(slip, 0);
      }
      ctx.fillStyle = this.colors.ink;
      ctx.globalAlpha = alpha * (0.12 + 0.3 * this.energy + 0.3 * this.flash) * this.intensity;
      ctx.fill(this.pathOf(kind, ff));
      ctx.restore();
    };
    if (this.prev && m < 1) inkFor(this.prev, 1 - m);
    inkFor(this.kind, m < 1 ? m : 1);
    // Strands.
    ctx.translate(f.ox, f.oy);
    for (let k = 0; k < this.strands.length; k++) {
      const s = this.strands[k];
      const path = new Path2D();
      for (let i = 0; i < s.len; i++) {
        let x = s.trail[i * 2], y = s.trail[i * 2 + 1];
        if (m < 1 && this.from[k]) {
          // Fly from where the strand was to where it is now, with a jolt.
          const e = m * m * (3 - 2 * m);
          const jolt = (1 - e) * 10;
          x = this.from[k][i * 2] + (x - this.from[k][i * 2]) * e + (Math.random() - 0.5) * jolt;
          y = this.from[k][i * 2 + 1] + (y - this.from[k][i * 2 + 1]) * e + (Math.random() - 0.5) * jolt;
        }
        // Crackle: a small wobble off the line that drifts with time.
        const n = noise2(i * 0.35 + s.seed, t * 6 + s.seed) - 0.5;
        x += n * 2.4; y += (noise2(i * 0.35 + s.seed + 7, t * 6) - 0.5) * 2.4;
        if (i === 0) path.moveTo(x, y); else path.lineTo(x, y);
      }
      const fade = 0.55 + 0.45 * noise2(t * 1.5, s.seed);
      ctx.strokeStyle = this.colors.glow; ctx.lineWidth = 7; ctx.globalAlpha = 0.10 * glowBase * fade; ctx.stroke(path);
      ctx.strokeStyle = this.colors.glow; ctx.lineWidth = 2.6; ctx.globalAlpha = 0.45 * glowBase * fade; ctx.stroke(path);
      ctx.strokeStyle = this.colors.core; ctx.lineWidth = 1.1; ctx.globalAlpha = 0.95 * glowBase * fade; ctx.stroke(path);
    }
    // Sparks.
    for (const sp of this.sparks) {
      const age = (t - sp.born) / sp.life;
      const a = age < 0.2 ? age / 0.2 : 1 - (age - 0.2) / 0.8;
      const path = new Path2D();
      for (let i = 0; i < sp.pts.length; i += 2) { if (i === 0) path.moveTo(sp.pts[i], sp.pts[i + 1]); else path.lineTo(sp.pts[i], sp.pts[i + 1]); }
      ctx.strokeStyle = this.colors.glow; ctx.lineWidth = sp.width * 3; ctx.globalAlpha = 0.25 * a * this.intensity; ctx.stroke(path);
      ctx.strokeStyle = this.colors.core; ctx.lineWidth = sp.width; ctx.globalAlpha = 0.9 * a * this.intensity; ctx.stroke(path);
    }
    // A flash whitens everything for a moment.
    if (this.flash > 0.01) {
      ctx.fillStyle = this.colors.core;
      ctx.globalAlpha = 0.22 * this.flash * this.intensity;
      ctx.fill(this.pathOf(this.kind, f));
    }
    ctx.restore();
  }
}
