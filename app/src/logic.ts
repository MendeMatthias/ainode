// Pure presentation rules. No DOM, no API: everything here is unit-tested.
const GiB = 1024 ** 3;
import { DEVICE, OS, THIS, deviceFor, platformFor, type Os } from "./platform";
import type { AppState, BtxNodes, CatalogModel, Community, Length, ModelPrefs, SystemInfo, UpdateInfo, WslState } from "./api";
import btxPack from "./knowledge/btx.md?raw";
import instructionText from "./knowledge/instruction.md?raw";

const GB = 1e9;

/** Sizes the way Finder shows them (decimal GB), because that is what the user checks against. */
export function fmtBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 GB";
  if (n < 1e9) return `${Math.max(1, Math.round(n / 1e6))} MB`;
  // One decimal below 100 GB, so 474 MB of 25 GB used reads "24.5 GB left",
  // not a rounded "25 GB" that hides the usage. A trailing ".0" is dropped.
  const g = n / GB;
  return `${g < 100 ? g.toFixed(1).replace(/\.0$/, "") : Math.round(g)} GB`;
}

export function fmtPct(p: number): string {
  const v = Math.max(0, Math.min(1, p));
  return `${Math.floor(v * 100)}%`;
}

export interface Status {
  tone: "idle" | "busy" | "ok" | "warn" | "error";
  line: string;
  /** A part of `line` that opens a page (the BTX nodes open easybtx.com/nodes). */
  link?: { text: string; url: string };
}

/** Where the BTX nodes on the home line lead: easybtx.com's census of them. */
export const BTX_NODES_URL = "https://easybtx.com/nodes";

/**
 * The one line under the core. `fetching` is whether any download is running:
 * the engine's peers_connected counts open connections and reads 0 when idle,
 * so it cannot say whether a host is reachable.
 */
/** "7 AInodes" or "1 AInode". */
export function ainodesText(n: number): string {
  return n === 1 ? "1 AInode" : `${n} AInodes`;
}

/** "46 BTX nodes" or "1 BTX node". */
export function btxNodesText(n: number): string {
  return n === 1 ? "1 BTX node" : `${n} BTX nodes`;
}

/** The note under Start with this Mac (Rust login.rs). */
export function loginNoteText(on: boolean, error: string | null, thisComputer: string): string {
  if (error) return `${error[0].toUpperCase()}${error.slice(1)}. AInode works the same; it just opens when you open it.`;
  if (on) return `AInode opens by itself when ${thisComputer} starts, minimized, and its engine starts with it: online without a click. Open AInode to see its window.`;
  return "AInode opens only when you open it.";
}

/** Status' row for a BTX node on this computer (Rust neighbour.rs), or null when none answers. */
export function nodeHereText(height: number | null): string | null {
  if (!height) return null;
  return `At block ${height.toLocaleString("en-US")} · easyNode on this computer, read from its witness on 127.0.0.1:3081`;
}

/** The note under Graphics card. With a node on this computer, the card keeps its blocks quick. */
export function gpuNoteText(on: boolean, nodeHere: boolean, thisComputer: string): string {
  if (on && nodeHere) return "Chats run on the graphics card when AInode finds one. easyNode on this computer checks BTX blocks on it too: Leave it free keeps those quick.";
  if (on) return "Chats run on the graphics card when AInode finds one: faster answers. Leave it free if it mines or runs a node.";
  return `Chats run on ${thisComputer}'s processor only: slower answers, and the graphics card stays free for mining or a node.`;
}

/** The BTX nodes in a count from easybtx.com (/api/ainode-online), or null for none or anything else. */
export function btxNodesOf(c: Pick<Community, "btxNodes"> | null): BtxNodes | null {
  const n = c?.btxNodes;
  if (typeof n?.live !== "number" || !Number.isFinite(n.live) || n.live < 1) return null;
  return { live: Math.floor(n.live), countries: typeof n.countries === "number" && n.countries > 0 ? Math.floor(n.countries) : 0 };
}

/**
 * The app's poll (main.ts refresh) also checks in, starts a stopped engine
 * again and looks for updates. It used to run only while the window was
 * visible, so a minimized or covered AInode dropped out of the count after
 * 15 minutes, never healed its engine and never saw an update (the PC,
 * 2026-10-16: the Mac counted 0 while running). Hidden, it now runs once a
 * minute; visible, every tick.
 */
export const HIDDEN_POLL_MS = 60 * 1000;
export function pollDue(hidden: boolean, now: number, polledAt: number): boolean {
  return !hidden || now - polledAt >= HIDDEN_POLL_MS;
}

/** Rust checks in every 5 minutes (backend/keeper.rs); the page reads the public count every minute. */
export const CHECKIN_EVERY_MS = 5 * 60 * 1000;
export const COUNT_EVERY_MS = 60 * 1000;

export function countDue(now: number, readAt: number): boolean {
  return now - readAt >= COUNT_EVERY_MS;
}

// ── Send feedback (plan 3.2) ────────────────────────────────────────────
// A new issue on ainode-releases, opened in the browser and filled in with the
// version and the system. The user reads it and sends it there; the app sends
// nothing. Model names only when the user ticks the box; never chats, files
// or paths.
export const FEEDBACK_REPO = "https://github.com/MendeMatthias/ainode-releases";
export type FeedbackKind = "bug" | "idea" | "model";

export function systemLine(i: SystemInfo): string {
  const gb = i.ram_bytes ? `${Math.round(i.ram_bytes / 1024 ** 3)} GB memory` : "";
  return [i.os, i.machine, i.chip, gb].filter(Boolean).join(" · ");
}

export function feedbackUrl(kind: FeedbackKind, version: string, platform: string, info: SystemInfo, modelNames: string[] = []): string {
  const q = new URLSearchParams({ template: `${kind}.yml`, version: `AInode ${version} for ${platform}`, system: systemLine(info) });
  if (modelNames.length) q.set("models", modelNames.join(", ").slice(0, 400));
  return `${FEEDBACK_REPO}/issues/new?${q.toString()}`;
}

/**
 * Do the animations rest (styles.css data-away)? While the window is hidden or
 * minimized, and on Linux also while another window has the focus, since
 * there every frame costs the processor (#89). Minimized counts on its own:
 * WebView2 keeps `document.hidden` false for a minimized window, and an
 * AInode started at login opens minimized.
 */
export function awayNow(hidden: boolean, minimized: boolean, os: string, focused: boolean): boolean {
  return hidden || minimized || (os === "linux" && !focused);
}

// ── Updates (Rust update.rs) ────────────────────────────────────────────
/** At start, then every hour while AInode is open: a release reaches everyone the same day. */
export const UPDATE_EVERY_MS = 60 * 60 * 1000;

export function updateDue(now: number, checkedAt: number): boolean {
  return checkedAt === 0 || now - checkedAt >= UPDATE_EVERY_MS;
}

/**
 * Install by itself now (Mende, 2026-10-18: no click needed)? A newer version
 * this computer can install unattended, not tried yet this round, at a quiet
 * moment (nothing downloading, no chat or panel open). A failed attempt waits
 * for the next hourly check rather than retrying in a loop.
 */
export function autoInstallNow(u: UpdateInfo | null, tried: string | null, quiet: boolean, updating: boolean): boolean {
  return !!u?.version && u.by_itself && !updating && quiet && tried !== u.version;
}

/** The restart is offered only when it would cut nothing: no model downloading, no chat open. */
export function updateOfferable(models: CatalogModel[], chatOpen: boolean): boolean {
  return !chatOpen && !models.some((m) => m.local === "downloading");
}

/** Status's line under Check for updates. */
export function updateNote(u: UpdateInfo | null, error: string | null, checking: boolean, version: string): string {
  if (checking) return "Looking for a newer AInode.";
  if (error) return `${error}. The download on easybtx.com/ai always works.`;
  if (!u) return `AInode looks for updates when it starts and every hour, and installs them by itself. This is ${version}.`;
  if (u.refused) return `${u.refused}. AInode stays on ${version}; the download on easybtx.com/ai always works.`;
  if (u.version) return `AInode ${u.version} is ready. Restart to update; your models and chats stay as they are.`;
  return `AInode ${version} is the latest.`;
}

/** The public count is cached for a minute at the edge: an older reading never replaces a newer one. */
export function newerCommunity(had: Community | null, got: Community | null): Community | null {
  if (!got) return had;
  if (had && got.at < had.at) return had;
  return got;
}

/**
 * `byEasyNode`: the engine stopped within a few minutes of easyNode saying it
 * was restarting WSL (Rust neighbour.rs reads its note). Then the home line
 * says so while AInode waits to start again, instead of an engine error.
 */
/** easyNode's note counts for a stop at `nowMs` when it is at most 5 minutes older (or a minute of clock skew newer). */
export function easyNodeRestarted(noteAt: number | null, nowMs: number): boolean {
  if (!noteAt) return false;
  const age = nowMs / 1000 - noteAt;
  return age >= -60 && age <= 5 * 60;
}
export const EASYNODE_RESTART_LINE = "easyNode is restarting WSL to get its graphics card back. AInode starts again by itself";
/** Windows: AInode restarts WSL itself (it was stuck and nothing else ran, Rust backend/wslheal.rs). */
export const WSL_RESTART_LINE = "Windows' Linux (WSL) lost this PC's drives. AInode is restarting it and starts again by itself";

/** "Ubuntu", "Ubuntu and Debian", "Ubuntu, Debian and Kali". */
function namesText(names: string[]): string {
  return names.length <= 1 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/**
 * Windows: the bar above the button when WSL is stuck and other Linux runs.
 * Names every distribution that would stop; null when there is nothing to ask.
 */
export function wslBarText(w: WslState | null | undefined): string | null {
  const others = w?.stuck_others;
  if (!others || w?.restarting) return null;
  const names = namesText(others);
  const also = names ? `: this also stops ${names}` : "";
  const node = w.node_here ? ". A BTX node runs on this PC: it stops too" : "";
  return `Restart Windows' Linux${also}${node}.`;
}
/** How long the home line tells what a WSL restart started again: all of it, or not. */
export const WSL_AGAIN_OK_S = 5 * 60;
export const WSL_AGAIN_WARN_S = 30 * 60;

/**
 * Windows: after AInode restarted WSL on the user's click, the other Linux it
 * stopped was started again (Rust wsl::start_again). The home line says what
 * runs again, and what did not, with the command to start it by hand.
 */
export function wslAgainStatus(w: WslState | null | undefined, nowMs: number): Status | null {
  const a = w?.again;
  if (!a || (a.back.length === 0 && a.not_back.length === 0)) return null;
  const age = nowMs / 1000 - a.at;
  // Each name stays whole on a narrow window (a non-breaking hyphen); the command stays plain to copy.
  const names = (n: string[]) => namesText(n).replace(/-/g, "\u2011");
  const back = a.back.length ? ` ${names(a.back)} ${a.back.length === 1 ? "runs" : "run"} again.` : "";
  if (a.not_back.length === 0) {
    return age <= WSL_AGAIN_OK_S ? { tone: "ok", line: `Windows' Linux restarted.${back}` } : null;
  }
  if (age > WSL_AGAIN_WARN_S) return null;
  const how = a.not_back.map((d) => `wsl -d ${d}`).join(", ");
  return { tone: "warn", line: `Windows' Linux restarted.${back} ${names(a.not_back)} did not start again: start ${a.not_back.length === 1 ? "it" : "them"} with ${how}` };
}

export function statusOf(s: AppState, fetching = false, community: Community | null = null, restarting = false, byEasyNode = false, nowMs = Date.now()): Status {
  if (!s.consent) return { tone: "idle", line: "Choose how much space AInode may use" };
  switch (s.engine.state) {
    case "stopped":
      return { tone: "idle", line: "Press start to connect to BTX ModelNet" };
    case "starting":
      if (s.wsl?.easynode) return { tone: "busy", line: EASYNODE_RESTART_LINE };
      if (s.wsl?.restarting) return { tone: "busy", line: WSL_RESTART_LINE };
      return { tone: "busy", line: "Starting the engine" };
    case "error":
      if (restarting && byEasyNode) return { tone: "busy", line: EASYNODE_RESTART_LINE };
      // A stuck WSL with other Linux running: the line explains it, the bar asks.
      if (restarting && !s.wsl?.stuck_others) return { tone: "busy", line: "The engine stopped. Starting it again" };
      return { tone: "error", line: `Could not start. ${engineErrorText(s.engine.error)}` };
    case "running": {
      const again = wslAgainStatus(s.wsl, nowMs);
      if (again) return again;
      const cfg = s.network.peers_configured;
      const hosts = `${cfg} host${cfg === 1 ? "" : "s"}`;
      if (cfg === 0) return { tone: "warn", line: "Running. No public host is listed yet" };
      if (fetching) return { tone: "busy", line: `Fetching and verifying · ${hosts}` };
      // The numbers only: the ring already says ONLINE. The AInodes online
      // (easybtx.com's count), this app's hosts, then the BTX nodes keeping
      // the chain (easybtx.com's census, easyNodes among them).
      // Each figure stays whole on a narrow window: it wraps only between them.
      const btx = btxNodesOf(community);
      const whole = (f: string) => f.replace(/ /g, "\u00a0");
      const nodes = btx ? whole(btxNodesText(btx.live)) : "";
      const figures = [community && community.online > 0 ? ainodesText(community.online) : "", hosts].map(whole);
      const line = [...figures, nodes].filter(Boolean).join(" · ");
      return nodes ? { tone: "ok", line, link: { text: nodes, url: BTX_NODES_URL } } : { tone: "ok", line };
    }
  }
}

export type ActionKind = "get" | "progress" | "chat" | "blocked" | "remove";
export interface Action {
  kind: ActionKind;
  label: string;
  hint?: string;
}

export function actionFor(m: CatalogModel, s: AppState): Action {
  // Listed only because a copy is on disk (catalog::shown): remove, not chat.
  // Mende opened the 350M this way after it was dropped and it looped again.
  if (m.offered === false) return { kind: "remove", label: "Remove" };
  if (m.local === "ready") return { kind: "chat", label: "Chat" };
  if (m.local === "downloading") return { kind: "progress", label: fmtPct(m.progress) };
  const verb = m.error ? "Retry" : m.progress > 0 ? "Resume" : "Get";
  // Not on a computer that cannot run it: the download would only end in a
  // model that cannot load. (hw.memsize is binary; ram_gb is the marketed
  // size; the Linux backend rounds its reading up the same way, disk.rs.)
  if (m.ram_gb && s.ram_bytes && m.ram_gb * GiB > s.ram_bytes) {
    return { kind: "blocked", label: verb, hint: `Needs a ${DEVICE} with ${m.ram_gb} GB of memory` };
  }
  if (s.engine.state !== "running") return { kind: "blocked", label: verb, hint: "Start first" };
  if (s.network.peers_configured === 0) return { kind: "blocked", label: verb, hint: "No host yet" };
  const room = roomLeft(s);
  // Getting a model needs its size twice (the pieces plus the runnable copy
  // made right after its download, 0.8.0); afterwards the pieces are deleted (store_once.rs,
  // 0.7.1), but the room must exist for that moment.
  if (room !== null && m.size_bytes * 2 > room) return { kind: "blocked", label: verb, hint: "Not enough space" };
  // And the pieces alone must fit the engine's half of the budget, or the engine
  // evicts other models and then fails with "disk quota" anyway.
  if (s.consent && m.size_bytes > s.consent.engine_bytes - s.usage.engine_used_bytes) {
    return { kind: "blocked", label: verb, hint: "Not enough space" };
  }
  return { kind: "get", label: verb };
}

/** Bytes still free inside the consented budget, or null before consent. */
export function roomLeft(s: AppState): number | null {
  if (!s.consent) return null;
  return Math.max(0, s.consent.total_bytes - s.usage.total_bytes);
}

/**
 * Sizes a person can pick without typing. Minimal holds the 3B (about twice
 * its 2.2 GB: the download plus a copy ready to run) with room to spare.
 */
export const PRESETS: readonly { name: string; gb: number }[] = [
  { name: "Minimal", gb: 10 },
  { name: "Medium", gb: 25 },
  { name: "Big", gb: 100 },
];

/**
 * Where the size sheet starts, inside the limits Rust reports (state.space):
 * the current size, else Medium, else the biggest preset that fits, else the
 * most allowed. Null when nothing fits (max below min: the disk is full).
 */
export function startSize(min: number, max: number, current: number | null): number | null {
  if (max < min) return null;
  if (current !== null && current >= min && current <= max) return current;
  const fit = PRESETS.map((p) => p.gb).filter((g) => g >= min && g <= max);
  if (fit.includes(25)) return 25;
  return fit.length ? fit[fit.length - 1] : max;
}

/**
 * A typed size, read leniently ("200", "200 GB", "200gb"), checked against
 * the same limits the backend enforces. Whole GB only, like the backend.
 */
export function readSize(text: string, min: number, max: number): { gb: number } | { error: string } {
  const m = /^\s*(\d{1,7})\s*(gb)?\s*$/i.exec(text);
  if (!m) return { error: "Type a whole number of GB, like 200." };
  const gb = Number(m[1]);
  if (gb < min) return { error: `At least ${min} GB.` };
  if (gb > max) return { error: `At most ${max} GB: that is what ${THIS} can spare now.` };
  return { gb };
}

/** The engine's errors, said in words a person can act on. Unknown ones pass through. */
export function friendlyError(e: string): string {
  if (/pin mismatch/i.test(e)) return "This host's identity changed since you last used it, so AInode refused it. Nothing was downloaded from it.";
  if (/disk quota/i.test(e)) return "Not enough room in the space you allowed. Remove a model or allow more.";
  if (/429|RESOURCE_GOVERNOR/i.test(e)) return "The host is limiting uploads and refused. Try again later.";
  if (/cancel/i.test(e)) return "Cancelled.";
  // The first request of a fetch: the host either lacks this model or did not answer.
  if (/manifest failed/i.test(e)) return "The host does not have this model, or did not answer. Try again later.";
  if (/eof|reset|refused|timed? ?out|connect/i.test(e)) return "Lost the host mid-way. Try again: verified pieces are kept.";
  return `Stopped: ${e}`;
}

/**
 * What a failed chat request means, from llama-server's status and error
 * message. Out of graphics memory was seen in the lab (2026-10-12): two large
 * models plus the 3B on a 24 GB Mac made llama-server answer 500 with
 * "Insufficient Memory"/"Compute error".
 */
export function chatErrorText(status: number, message = ""): string {
  if (/insufficient memory|out ?of ?memory|compute error|failed to allocate|oom/i.test(message)) {
    return OS === "mac"
      ? "This Mac ran out of memory for the model. Its graphics memory is shared with other apps: close some, or use a smaller model, and try again."
      : `This ${DEVICE} ran out of memory for the model. Close some apps, or use a smaller model, and try again.`;
  }
  if (/context|too long|exceeds/i.test(message)) return "The conversation is too long for the model. Start a new chat.";
  return message ? `The model stopped: ${message}` : `The model stopped (local model answered ${status}).`;
}

/** One observation of a download's verified progress (0..1) at time t (ms). */
export interface Sample { t: number; p: number }

/**
 * Time left for a download, from verified-progress samples over the last
 * `windowMs`. Progress moves in 4 MiB steps and up to 8 pieces land together,
 * so a short window would swing wildly; null until there is a real rate.
 */
export function etaText(samples: Sample[], sizeBytes: number, now: number, windowMs = 30_000): string | null {
  const recent = samples.filter((x) => now - x.t <= windowMs);
  if (recent.length < 2) return null;
  const a = recent[0], b = recent[recent.length - 1];
  const dp = b.p - a.p, dt = (b.t - a.t) / 1000;
  if (dp <= 0 || dt < 3) return null;
  const secs = ((1 - b.p) / dp) * dt;
  const rate = (dp * sizeBytes) / dt;
  const speed = rate >= 1e6 ? `${(rate / 1e6).toFixed(1)} MB/s` : `${Math.max(1, Math.round(rate / 1e3))} KB/s`;
  const left = secs < 60 ? "under a minute" : secs < 3600 ? `${Math.ceil(secs / 60)} min` : `${(secs / 3600).toFixed(1)} h`;
  return `${speed} · ${left} left`;
}

/**
 * Status, "Network": what the bundled engine is. The version and where the
 * helper comes from are read from the engine folder's own manifest, which
 * staging writes from the tag it pins. No version is written here by hand:
 * 0.9.0's first build still said "BTX v0.34.15, unmodified".
 */
export function engineLine(e: AppState["engine"]): string {
  const b = e.bundled;
  if (!b) return "BTX engine";
  return b.built_by_ainode
    ? `BTX ${b.version} engine, built by AInode from upstream's source`
    : `BTX ${b.version} engine, upstream's file unmodified`;
}

/** Why the engine would not start, in words a person can act on. */
export function engineErrorText(e?: string): string {
  if (!e) return "Press to try again.";
  // The helper mints its post-quantum identity itself on first start (since
  // the v0.34.16 engine; before, with a bundled OpenSSL tool, experiments E1).
  // If that fails, the bundle is damaged.
  if (/certificate|mldsa|openssl|pq1/i.test(e)) return "Its post-quantum identity could not be created. Reinstall AInode.";
  if (/disk|space|quota|No space/i.test(e)) return "There is not enough free disk space.";
  if (/socket|address already in use|in use/i.test(e)) return "Another copy of the engine is still running. Quit AInode and open it again.";
  if (/signal|exited|stopped/i.test(e)) return "The engine stopped unexpectedly. Press to start it again.";
  return e;
}

/**
 * The one instruction every model gets before the conversation: who it is,
 * the BTX knowledge pack, and how to answer. Both live as text next to this
 * file (knowledge/instruction.md, knowledge/btx.md), with why each line is
 * there; lab/replay-chat.py reads the same files.
 */
const strip = (s: string) => s.replace(/<!--[\s\S]*?-->/g, "").trim();
export const SYSTEM_PROMPT = strip(instructionText);

/** The pack as the model sees it: its source comments removed. */
export const KNOWLEDGE = strip(btxPack);

/** {device} and {platform}: "Mac" and "Mac" on a Mac, so its text is the one measured. */
export function systemPrompt(model: string, maker: string, os: Os = OS): string {
  return SYSTEM_PROMPT.replace("{knowledge}", KNOWLEDGE)
    .split("{model}").join(model)
    .split("{maker}").join(maker)
    .split("{device}").join(deviceFor(os))
    .split("{platform}").join(platformFor(os));
}

/**
 * The most recent messages that fit `maxChars`, oldest dropped first, so a
 * long chat never outgrows the model's window (8,192 tokens, shared with the
 * instruction and the answer). The latest message is always kept.
 */
export function fitHistory<T extends { content: string }>(msgs: T[], maxChars: number): T[] {
  let used = 0;
  let i = msgs.length;
  while (i > 0 && (i === msgs.length || used + msgs[i - 1].content.length <= maxChars)) {
    used += msgs[--i].content.length;
  }
  return msgs.slice(i);
}

/**
 * The answer without a draft the model leaked before a stray `</think>`:
 * Granite 4.2 3B sometimes writes an answer, then `</think>`, then the answer
 * again (seen in the lab's code and poem replays, 2026-10-12). What follows the
 * last tag is the real answer.
 */
export function cleanAnswer(text: string): string {
  const i = text.lastIndexOf("</think>");
  return i < 0 ? text : text.slice(i + "</think>".length).replace(/^\s+/, "");
}

/**
 * LaTeX maths as plain text, for display: gpt-oss writes "\\(6 \\times 6 = 36\\)"
 * and "\\frac{1}{6}", which the chat would otherwise show raw. Code blocks
 * are never touched (mdBlocks only calls this on text lines).
 */
export function plainMath(line: string): string {
  const sym: Record<string, string> = {
    times: "×", div: "÷", cdot: "·", le: "≤", leq: "≤", ge: "≥", geq: "≥", neq: "≠", ne: "≠", approx: "≈",
    pm: "±", pi: "π", infty: "∞", to: "→", rightarrow: "→", Rightarrow: "⇒", degree: "°", circ: "°", ldots: "…", dots: "…",
  };
  let t = line.replace(/\\\(|\\\)|\\\[|\\\]/g, "");
  for (let i = 0; i < 3; i++) t = t.replace(/\\d?frac\{([^{}]*)\}\{([^{}]*)\}/g, "$1/$2");
  t = t.replace(/\\sqrt\{([^{}]*)\}/g, "√$1").replace(/\\(?:text|mathrm|mathbf|operatorname)\{([^{}]*)\}/g, "$1");
  t = t.replace(/\^\{([^{}]*)\}/g, "^$1").replace(/\\[,;! ]/g, " ");
  return t.replace(/\\([A-Za-z]+)/g, (m, name: string) => sym[name] ?? m);
}

/**
 * Where a looping answer should be cut, or null when it is not looping.
 * Granite 4.0 350M wrote one 200-character paragraph over and over until the
 * 2,048-token limit (Mende's chat, 2026-10-12). Looping means the last 100
 * characters already occur three more times in the 2,000 before them: long
 * enough that a repeated line of code or a table rule does not trigger it.
 * The cut keeps everything up to the end of the first repetition.
 */
/**
 * Send turns into Stop under the pointer the moment a question goes out, so
 * the second click of a double click landed on Stop and the question ended
 * at once, "(stopped)" (the real window, 2026-10-16). A click counts as Stop
 * only when it is not part of a double click and comes half a second or more
 * after the question went out.
 */
export const STOP_GRACE_MS = 500;
export function stopIntended(clickCount: number, msSinceSent: number): boolean {
  return clickCount <= 1 && msSinceSent >= STOP_GRACE_MS;
}

export function loopCut(text: string): number | null {
  if (text.length < 500) return null;
  const tail = text.slice(-100);
  const win = text.slice(-2100, -100);
  let n = 0;
  for (let at = win.indexOf(tail); at !== -1; at = win.indexOf(tail, at + 1)) n++;
  return n >= 3 ? text.indexOf(tail) + tail.length : null;
}

/** One piece of a line of model output, as the chat shows it. */
export type Inline = { t: "text" | "b" | "i" | "code"; v: string };
export type Block =
  | { k: "code"; v: string }
  | { k: "line"; parts: Inline[]; head?: boolean; bullet?: string }
  | { k: "table"; rows: Inline[][][] }; // rows of cells; the first row is the header

/**
 * The little Markdown small models write (bold, italics, `code`, fenced code,
 * headings, bullets), parsed into pieces the chat turns into DOM nodes with
 * textContent, so model output never becomes HTML. Anything else stays as
 * typed. An unclosed fence (mid-stream) is still code. Underscores are left
 * alone: they are snake_case far more often than emphasis.
 */
export function mdBlocks(src: string): Block[] {
  const out: Block[] = [];
  let code: string[] | null = null;
  for (let line of src.split("\n")) {
    if (/^\s*```/.test(line)) {
      if (code) { out.push({ k: "code", v: code.join("\n") }); code = null; } else code = [];
      continue;
    }
    if (code) { code.push(line); continue; }
    line = plainMath(line);
    // A Markdown table (gpt-oss writes them): "| a | b |" rows, "|---|---|" ruler skipped.
    if (/^\s*\|.*\|\s*$/.test(line)) {
      const cells = line.trim().slice(1, -1).split("|").map((c) => c.trim());
      if (cells.every((c) => /^:?-{2,}:?$/.test(c))) continue;
      const prev = out[out.length - 1];
      const row = cells.map(inline);
      if (prev?.k === "table") prev.rows.push(row);
      else out.push({ k: "table", rows: [row] });
      continue;
    }
    const h = /^#{1,6}\s+(.*)$/.exec(line);
    const li = /^(\s*)[-*+]\s+(.*)$/.exec(line);
    if (h) out.push({ k: "line", parts: inline(h[1]), head: true });
    else if (li) out.push({ k: "line", parts: inline(li[2]), bullet: `${li[1]}\u2022 ` });
    else out.push({ k: "line", parts: inline(line) });
  }
  if (code) out.push({ k: "code", v: code.join("\n") });
  return out;
}

function inline(s: string): Inline[] {
  const parts: Inline[] = [];
  const re = /`([^`]+)`|\*\*([^*]+)\*\*|\*([^*\s](?:[^*]*[^*\s])?)\*/g;
  let at = 0;
  for (let m; (m = re.exec(s)); at = re.lastIndex) {
    if (m.index > at) parts.push({ t: "text", v: s.slice(at, m.index) });
    if (m[1] !== undefined) parts.push({ t: "code", v: m[1] });
    else if (m[2] !== undefined) parts.push({ t: "b", v: m[2] });
    else parts.push({ t: "i", v: m[3] });
  }
  if (at < s.length) parts.push({ t: "text", v: s.slice(at) });
  return parts;
}

/** A model's chat settings: its own, else Think as the chat last had it and the defaults. */
export function prefsFor(s: Pick<AppState, "models" | "think"> | null, id: string): ModelPrefs {
  const own = s?.models?.[id];
  return own ? { ...own } : { think: s?.think ?? true, length: "normal", temperature: 0.6 };
}

/**
 * What an answer length means for one request. The window is 8,192 tokens
 * (chat.rs CONTEXT): the instruction takes about 1,500, the answer budget and
 * the conversation share the rest, so a long answer gets a shorter history.
 * Thinking counts against the budget too (an empty answer is asked again
 * without it, chat.ts). The note goes at the end of the instruction.
 */
export function lengthPlan(l: Length): { maxTokens: number; historyChars: number; note: string } {
  switch (l) {
    case "short":
      return { maxTokens: 1024, historyChars: 12_000, note: "\n\nThe user chose short answers: answer in one to three sentences unless they ask for more." };
    case "long":
      return { maxTokens: 3072, historyChars: 6_000, note: "\n\nThe user chose long answers: when a question deserves it, answer in depth, with steps, examples and structure." };
    default:
      return { maxTokens: 2048, historyChars: 12_000, note: "" };
  }
}

/** The temperature presets the settings sheet offers, in words. */
export const TEMPERATURES: readonly { label: string; value: number; hint: string }[] = [
  { label: "Precise", value: 0.2, hint: "Sticks to the likeliest words: facts, code, maths." },
  { label: "Balanced", value: 0.6, hint: "AInode's default." },
  { label: "Creative", value: 1.0, hint: "Freer wording: stories, ideas, poems. Makes more mistakes." },
];
