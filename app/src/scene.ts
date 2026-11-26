// The loading scene in the chat: while a model loads (five to twenty seconds),
// a storm of glitching letters and an electric BTX mark; when it is ready the
// mark morphs into one of the three faces Mende chose on 2026-10-13 (one at
// random, or the one pinned in Status), its short loop fades in under the
// strands, the field calms, and the words scramble in. Reduced motion shows
// the still and plain text. The scene is removed with the first message.
import type { Face } from "./api";
import { OS, THIS, playsVideo } from "./platform";
import { ElectricMark, LetterField, calm, hoverScramble, lattice, scrambleIn, shapeBTX, shapeFromImage } from "./fx";
import chipStill from "./assets/faces/chip.webp";
import glyphsStill from "./assets/faces/glyphs.webp";
import swarmStill from "./assets/faces/swarm.webp";

export interface Scene {
  /** The model answered: morph to the face and say it is ready. */
  ready(name: string): void;
  /** The start failed: say why, in the scene's own words. */
  fail(text: string): void;
  close(): void;
}

type FaceKind = Exclude<Face, "random">;
interface FaceDef { still: string; loop: string | null; anchors: number[]; threshold: number; name: string }
// The loops (Higgsfield, from the stills) are added when they exist next to the
// stills; a face without one shows its still. Anchors: where strands start,
// in 0..1 of the picture (eyes, and for the chip the chip itself, so the
// strands spawn out of it).
const loops = import.meta.glob("./assets/faces/*.mp4", { eager: true, import: "default", query: "?url" }) as Record<string, string>;
const loopOf = (name: string) => loops[`./assets/faces/${name}.mp4`] ?? null;
const FACES: Record<FaceKind, FaceDef> = {
  swarm: { still: swarmStill, loop: loopOf("swarm"), anchors: [0.37, 0.41, 0.63, 0.41, 0.5, 0.08, 0.5, 0.92, 0.2, 0.5, 0.8, 0.5, 0.37, 0.41, 0.63, 0.41], threshold: 120, name: "swarm" },
  glyphs: { still: glyphsStill, loop: loopOf("glyphs"), anchors: [0.34, 0.4, 0.67, 0.4, 0.5, 0.06, 0.5, 0.9, 0.12, 0.5, 0.88, 0.5, 0.34, 0.4, 0.67, 0.4], threshold: 130, name: "glyphs" },
  chip: { still: chipStill, loop: loopOf("chip"), anchors: [0.5, 0.78, 0.4, 0.8, 0.6, 0.8, 0.5, 0.82, 0.38, 0.23, 0.62, 0.23, 0.5, 0.06, 0.5, 0.5], threshold: 120, name: "chip" },
};
export const FACE_KINDS: FaceKind[] = ["swarm", "glyphs", "chip"];

export function pickFace(setting: Face, rnd: () => number = Math.random): FaceKind {
  if (setting !== "random") return setting;
  return FACE_KINDS[Math.min(FACE_KINDS.length - 1, Math.floor(rnd() * FACE_KINDS.length))];
}

export function mountScene(thread: HTMLElement, name: string, faceSetting: Face = "random"): Scene {
  const root = document.createElement("div");
  root.className = "scene";
  root.dataset.phase = "loading";
  const css = getComputedStyle(document.documentElement);
  const accent = css.getPropertyValue("--accent").trim() || "#4ade80";
  const hot = css.getPropertyValue("--text").trim() || "#e8ece9";
  const font = css.getPropertyValue("--font").trim() || "monospace";
  const face = FACES[pickFace(faceSetting)];
  root.dataset.face = face.name;

  let field: LetterField | null = null;
  let mark: ElectricMark | null = null;
  const stops: (() => void)[] = [];
  // The face's picture (its loop, or the still), placed on the mark's box.
  // On Linux always the still: see playsVideo.
  const media: HTMLImageElement | HTMLVideoElement = face.loop && !calm() && playsVideo(OS) ? document.createElement("video") : document.createElement("img");
  media.className = "scene-face-media";
  if (media instanceof HTMLVideoElement) {
    media.muted = true; media.loop = true; media.playsInline = true; media.autoplay = true;
    media.poster = face.still;
    media.src = face.loop!;
  } else {
    media.src = face.still;
    media.alt = "";
  }
  const place = (box: { x: number; y: number; w: number; h: number }) => {
    media.style.left = `${box.x}px`; media.style.top = `${box.y}px`;
    media.style.width = `${box.w}px`; media.style.height = `${box.h}px`;
  };

  if (!calm()) {
    field = new LetterField(root, { base: accent, hot, mid: accent }, font);
    mark = new ElectricMark(root, "btx", shapeBTX(font), { w: 0.62, h: 0.3, cy: 0.46 }, { core: hot, glow: accent, ink: accent });
    root.appendChild(media);
    mark.onLayout = (box, kind) => { if (kind !== "btx") place(box); };
    const scan = document.createElement("div");
    scan.className = "fx-scan";
    root.appendChild(scan);
    // Trace the face's bright edges from its still, so the strands can run on
    // it when the model is ready (the picture is small; this takes a frame).
    const img = new Image();
    img.src = face.still;
    const traced = new Promise<void>((resolve) => {
      img.onload = () => { try { mark?.addShape("face", shapeFromImage(img, face.anchors, face.threshold), { w: 0.78, h: 0.66, cy: 0.44 }); } catch { /* the mark keeps the BTX shape */ } resolve(); };
      img.onerror = () => resolve();
    });
    // Now and then the field tears: a reminder that this is live, not a picture.
    const tearing = window.setInterval(() => { if (root.dataset.phase === "loading" && Math.random() < 0.5) field?.tear(); }, 1400);
    stops.push(() => window.clearInterval(tearing));
    stops.push(() => { void traced; });
    (root as HTMLElement & { _traced?: Promise<void> })._traced = traced;
  } else {
    // Reduced motion: the still, centred, nothing moving.
    root.appendChild(media);
    media.style.left = "11%"; media.style.width = "78%"; media.style.top = "6%"; media.style.height = "66%";
    media.style.transition = "none";
  }

  const text = document.createElement("div");
  text.className = "scene-text";
  const title = document.createElement("div");
  title.className = "scene-title";
  const sub = document.createElement("div");
  sub.className = "scene-sub";
  const lat = lattice("sweep", "loading the model");
  sub.appendChild(lat.el);
  text.append(title, sub);
  root.appendChild(text);
  thread.replaceChildren(root);

  const t0 = performance.now();
  const timer = window.setInterval(() => { lat.timer.textContent = `${((performance.now() - t0) / 1000).toFixed(1)}s`; }, 100);
  stops.push(() => window.clearInterval(timer));
  stops.push(scrambleIn(title, `LOADING ${name.toUpperCase()}`, 900));

  const close = () => {
    for (const s of stops.splice(0)) s();
    field?.destroy(); mark?.destroy();
    field = null; mark = null;
    if (media instanceof HTMLVideoElement) { media.pause(); media.removeAttribute("src"); media.load(); }
    root.remove();
  };
  return {
    ready(model) {
      if (!root.isConnected) return;
      window.clearInterval(timer);
      lat.done();
      const show = () => {
        if (!root.isConnected) return;
        root.dataset.phase = "ready";
        mark?.setKind("face");
        if (mark) mark.energyTarget = 0.45;
        field?.setMode("calm");
        field?.tear(260);
        if (media instanceof HTMLVideoElement) media.play().catch(() => { /* the still poster stays */ });
        stops.push(scrambleIn(title, `${model.toUpperCase()} IS READY`, 800));
        window.setTimeout(() => {
          if (!root.isConnected) return;
          const line = document.createElement("span");
          line.className = "scene-line";
          sub.replaceChildren(line);
          stops.push(scrambleIn(line, `It runs entirely on ${THIS}. Nothing you type leaves it.`, 900));
          window.setTimeout(() => { if (root.isConnected) stops.push(hoverScramble(line), hoverScramble(title)); }, 1000);
        }, 500);
      };
      const traced = (root as HTMLElement & { _traced?: Promise<void> })._traced;
      if (traced) void traced.then(show); else show();
    },
    fail(why) {
      if (!root.isConnected) return;
      root.dataset.phase = "fail";
      window.clearInterval(timer);
      lat.done();
      mark?.surge();
      field?.setMode("calm");
      stops.push(scrambleIn(title, "COULD NOT START", 600));
      const line = document.createElement("span");
      line.className = "scene-line";
      sub.replaceChildren(line);
      stops.push(scrambleIn(line, why, 700));
    },
    close,
  };
}
