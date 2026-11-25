// Local chat with a downloaded model. The backend starts llama.cpp's server on
// 127.0.0.1 (never another interface) and returns its port; this module streams
// from it with the OpenAI-compatible endpoint. Nothing here talks to the network.
import type { Api, CatalogModel, Face, ModelPrefs, SavedChat } from "./api";
import { realEngine } from "./api";
import type { Looked } from "./knowledge/notes";
import { GLYPHS, calm, lattice } from "./fx";
import { chatErrorText, cleanAnswer, fitHistory, lengthPlan, loopCut, mdBlocks, stopIntended, systemPrompt, type Inline } from "./logic";
import { mountScene, type Scene } from "./scene";
import { THIS } from "./platform";
import { parseSse } from "./sse";
import { kindOf, looksScanned, splitMarkdown, splitPdf, splitText } from "./knowledge/own";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

type Msg = { role: "system" | "user" | "assistant"; content: string };

let history: Msg[] = [];
// What AInode looks things up in, on this Mac (knowledge/notes.ts): its own
// library, BTX's docs, postquantum.wiki and the offline encyclopedia (~58 MB).
// They load in a background worker (knowledge/worker.ts), warmed up when the
// chat opens. Notes go on the latest message only, as it is sent; the history
// keeps what the user typed.
let worker: Worker | null = null;
let workerFailed = false; // then lookups answer at once, without notes
let seq = 0;
const waiting = new Map<number, (l: Looked | null) => void>();
function library(): Worker {
  if (!worker) {
    worker = new Worker(new URL("./knowledge/worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (e: MessageEvent<{ id: number; looked: Looked | null }>) => {
      waiting.get(e.data.id)?.(e.data.looked);
      waiting.delete(e.data.id);
    };
    // A worker that cannot start must not cost every question an 8 s wait.
    worker.onerror = () => {
      workerFailed = true;
      for (const done of waiting.values()) done(null);
      waiting.clear();
    };
  }
  return worker;
}
/** What to add for this message. Never holds the answer back for long: after 8 s
 *  (the very first question while the library still loads) it goes without. */
function lookUpOffline(q: string, earlier: string[], recent?: string): Promise<Looked> {
  const none: Looked = { notes: "", source: "", titles: [] };
  if (workerFailed) return Promise.resolve(none);
  return new Promise((resolve) => {
    const id = ++seq;
    waiting.set(id, (l) => resolve(l ?? none));
    library().postMessage({ id, q, earlier, recent });
    setTimeout(() => { if (waiting.delete(id)) resolve(none); }, 8000);
  });
}
// The window is 8,192 tokens (chat.rs CONTEXT): about 1,500 for the
// instruction, the answer's budget, the rest for the conversation, at about 3
// characters a token. The model's answer length sets the split (lengthPlan).
// The user's own files (0.7.0): the worker gets every passage when the chat
// opens and after each change; `recentFile` is the one added last in this chat,
// for "summarise it".
let recentFile: string | undefined;
let apiRef: Api | null = null;
export async function syncMine(api: Api = apiRef!): Promise<void> {
  // No worker yet: the chat sends the files when it opens (no need to load
  // the encyclopedia just because a file was removed in Status).
  if (!api || !worker) return;
  const all = await api.knowledgePassages().catch(() => []);
  library().postMessage({ id: 0, mine: all.map((p) => ({ t: p.t, x: p.x })) });
}
const MAX_FILE_BYTES = 100e6;

/** Read dropped or chosen files on this Mac, split them, and keep their text. */
async function addFiles(api: Api, files: File[]) {
  scene?.close();
  scene = null;
  for (const f of files) {
    const kind = kindOf(f.name);
    if (!kind) { sys(`${f.name}: AInode reads PDF, Markdown and text files.`); continue; }
    if (f.size > MAX_FILE_BYTES) { sys(`${f.name} is ${Math.round(f.size / 1e6)} MB: AInode reads files up to 100 MB.`); continue; }
    const note = sys(`Reading ${f.name}…`);
    try {
      let docs;
      let extra = "";
      if (kind === "pdf") {
        const { pdfPages, MAX_PAGES } = await import("./knowledge/pdf");
        const { pages, total } = await pdfPages(await f.arrayBuffer());
        if (looksScanned(pages)) {
          note.textContent = `${f.name} has no text to read: it looks like a scan (pictures of pages). Nothing was added.`;
          continue;
        }
        docs = splitPdf(f.name, pages);
        if (total > MAX_PAGES) extra = ` Only its first ${MAX_PAGES} of ${total} pages were read.`;
      } else {
        const text = await f.text();
        docs = kind === "md" ? splitMarkdown(f.name, text) : splitText(f.name, text);
      }
      if (!docs.length) { note.textContent = `${f.name} is empty. Nothing was added.`; continue; }
      const added = await api.knowledgeAdd(f.name, kind, docs);
      recentFile = added.name;
      note.textContent = `Added ${added.name}: ${added.count} passage${added.count === 1 ? "" : "s"}.${extra} Ask about it: AInode looks in your files first. It stays on ${THIS} (Status, Your files).`;
      note.classList.add("ok");
    } catch (e) {
      note.textContent = `${f.name} could not be added: ${e instanceof Error ? e.message : String(e)}`;
      note.classList.add("bad");
    }
  }
  await syncMine(api);
  scroll();
}

let port = 0;
let key = "";
let current: CatalogModel | null = null;
let ready = false;
const START_LIMIT_MS = 120_000;
let abort: AbortController | null = null;
/** When the reply in progress was asked (performance.now()), for Stop. */
let sentAt = 0;
let wired = false;
// This model's settings (its card's settings button; Think also from the
// switch here). Think: the model reasons before it answers, on by default
// (state.rs) since measurements showed better and much shorter answers for a
// few seconds more. Remembered per model in settings.json.
let prefs: ModelPrefs = { think: true, length: "normal", temperature: 0.6 };
let savePrefs: (id: string, p: ModelPrefs) => Promise<void> = async () => {};
let face: Face = "random";
// Kept chats (chats.rs), only when the user turned them on in Status. Each
// conversation has an id; Clear and another model start a new one.
const newChatId = () => Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) => b.toString(16).padStart(2, "0")).join("");
let chatId = newChatId();
let keep: () => boolean = () => false;
// Bumped whenever the conversation is thrown away (Clear, another model), so a
// reply that was still streaming does not land in the new, empty history.
let gen = 0;
// The loading scene (scene.ts): shown while the model starts and until the
// first message; a start that fails says so there.
let scene: Scene | null = null;
const glyph = () => GLYPHS[Math.floor(Math.random() * GLYPHS.length)];

export async function openChat(
  api: Api, m: CatalogModel, p: ModelPrefs, faceSetting: Face,
  toast: (s: string, t?: "info" | "error") => void, save: (id: string, p: ModelPrefs) => Promise<void>,
  keepChats: () => boolean = () => false,
) {
  keep = keepChats;
  $("chatHistory").hidden = !keep();
  $("historySheet").hidden = true;
  library().postMessage({ id: 0 }); // load and index while the model starts, not on the first question
  apiRef = api;
  void syncMine(api);
  prefs = { ...p };
  savePrefs = save;
  face = faceSetting;
  if (!wired) wire(api, toast);
  $("think").setAttribute("aria-pressed", String(prefs.think));
  if (current?.uri !== m.uri) {
    gen++;
    chatId = newChatId();
    history = [];
    scene?.close();
    scene = null;
    $("thread").innerHTML = "";
  }
  current = m;
  $("chatTitle").textContent = m.name;
  $("chat").hidden = false;
  if (!history.length) {
    scene?.close();
    scene = mountScene($("thread"), m.name, face);
  }
  setReady(false);
  try {
    // Loading a model takes seconds; never let a start that does not answer
    // leave the panel waiting forever.
    const limit = new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error("it did not start within 2 minutes. Go back and press Chat to try again.")), START_LIMIT_MS));
    const started = await Promise.race([api.chatStart(m.uri), limit]);
    if (current?.uri !== m.uri) return; // the user moved on while it loaded
    ({ port, key } = started);
  } catch (e) {
    if (current?.uri === m.uri) {
      const why = `Could not start the model: ${e instanceof Error ? e.message : String(e)}`;
      if (scene && !history.length) scene.fail(why); else sys(why);
      // Nothing is starting any more: the box must not go on saying so.
      $<HTMLTextAreaElement>("prompt").placeholder = "The model did not start";
    }
    return;
  }
  if (!history.length) scene?.ready(m.name);
  setReady(true);
  $<HTMLTextAreaElement>("prompt").focus();
}

function wire(api: Api, toast: (s: string, t?: "info" | "error") => void) {
  wired = true;
  const ta = $<HTMLTextAreaElement>("prompt");
  const send = $<HTMLButtonElement>("send");
  const grow = () => { ta.style.height = "auto"; ta.style.height = `${Math.min(140, ta.scrollHeight)}px`; };
  ta.addEventListener("input", () => { grow(); send.disabled = !ta.value.trim() || abort !== null || !ready; });
  ta.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); void submit(toast); }
  });
  const thinkBtn = $<HTMLButtonElement>("think");
  thinkBtn.addEventListener("click", () => {
    prefs = { ...prefs, think: !prefs.think };
    thinkBtn.setAttribute("aria-pressed", String(prefs.think));
    // Remembered for this model, and as the default for models never set.
    if (current) savePrefs(current.id, prefs).catch(() => { /* a preference only; the switch still works */ });
    api.setPref("think", prefs.think).catch(() => {});
  });
  // Your own files: the + button or a drop anywhere on the chat.
  const input = $<HTMLInputElement>("attachInput");
  $("attach").addEventListener("click", () => input.click());
  input.addEventListener("change", () => {
    const files = [...(input.files ?? [])];
    input.value = "";
    if (files.length) void addFiles(api, files);
  });
  const panel = $("chat");
  const veil = $("dropVeil");
  let depth = 0;
  panel.addEventListener("dragenter", (e) => { if (e.dataTransfer?.types.includes("Files")) { depth++; veil.hidden = false; } });
  panel.addEventListener("dragleave", () => { if (--depth <= 0) { depth = 0; veil.hidden = true; } });
  panel.addEventListener("dragover", (e) => e.preventDefault());
  panel.addEventListener("drop", (e) => {
    e.preventDefault();
    depth = 0;
    veil.hidden = true;
    const files = [...(e.dataTransfer?.files ?? [])];
    if (files.length) void addFiles(api, files);
  });
  // History of kept chats (only shown when Keep chats is on).
  $("chatHistory").addEventListener("click", () => void showHistory(toast));
  $("historyClose").addEventListener("click", () => ($("historySheet").hidden = true));
  $("historyList").addEventListener("click", async (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-h]");
    if (!b || !apiRef) return;
    const id = b.dataset.id ?? "";
    if (b.dataset.h === "open") return void openSaved(id, toast);
    if (b.dataset.armed !== "1") {
      b.dataset.armed = "1";
      b.textContent = "Delete?";
      setTimeout(() => { b.dataset.armed = ""; b.textContent = "Delete"; }, 3500);
      return;
    }
    await apiRef.chatDelete(id).catch((x) => toast(String(x), "error"));
    if (id === chatId) chatId = newChatId();
    void showHistory(toast);
  });
  // Clear, inside the chat (Mende, 2026-10-13): the conversation and its
  // memory go, and the face comes back. The memory is only the history this
  // module sends with each question; the model keeps nothing between requests
  // and nothing of a chat is written to disk. Two clicks, like Remove.
  const clear = $<HTMLButtonElement>("chatClear");
  clear.addEventListener("click", () => {
    if (clear.dataset.armed !== "1") {
      clear.dataset.armed = "1";
      clear.textContent = "Clear chat?";
      setTimeout(() => { clear.dataset.armed = ""; clear.textContent = "Clear"; }, 3500);
      return;
    }
    clear.dataset.armed = "";
    clear.textContent = "Clear";
    clearChat();
  });
  send.addEventListener("click", (e) => {
    if (abort) {
      if (stopIntended(e.detail, performance.now() - sentAt)) abort.abort();
      return;
    }
    void submit(toast);
  });
  $("chatBack").addEventListener("click", async () => {
    abort?.abort();
    scene?.close();
    scene = null;
    $("chat").hidden = true;
    port = 0;
    key = "";
    try { await api.chatStop(); } catch { /* the app stops it on exit anyway */ }
  });
}

/** Empty the thread and the history, and replay the scene: the mark, then the face. */
export function clearChat() {
  abort?.abort();
  gen++;
  chatId = newChatId(); // a kept chat stays in History
  history = [];
  recentFile = undefined;
  scene?.close();
  $("thread").innerHTML = "";
  if (!current) return;
  const name = current.name;
  scene = mountScene($("thread"), name, face);
  // The model is still loaded: let the storm run a moment, then the face.
  if (ready) {
    const s = scene;
    setTimeout(() => { if (scene === s && !history.length) s.ready(name); }, 1600);
  }
  $<HTMLTextAreaElement>("prompt").focus();
}

/** The box is always typeable (a disabled box reads as broken); only sending
 *  waits for the model. Mende hit exactly that: a start that never finished
 *  left a box he could not type into. */
function setReady(ok: boolean) {
  ready = ok;
  const ta = $<HTMLTextAreaElement>("prompt");
  ta.disabled = false;
  ta.placeholder = ok ? "Ask anything" : "Starting the model…";
  $<HTMLButtonElement>("send").disabled = !ok || !ta.value.trim();
}

function sys(text: string): HTMLDivElement {
  const d = document.createElement("div");
  d.className = "msg sys";
  d.textContent = text;
  $("thread").appendChild(d);
  scroll();
  return d;
}

function scroll() {
  const t = $("thread");
  t.scrollTop = t.scrollHeight;
}

const SEND_ICON = `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M8 13V3M3.5 7.5 8 3l4.5 4.5"/></svg>`;
const STOP_ICON = `<svg viewBox="0 0 16 16" fill="currentColor"><rect x="4" y="4" width="8" height="8" rx="1.5"/></svg>`;

/**
 * Send the box's text, or (`again`) ask the last question once more: the last
 * answer is dropped from the history and the thread and a new one is written.
 */
async function submit(toast: (s: string, t?: "info" | "error") => void, again = false) {
  const ta = $<HTMLTextAreaElement>("prompt");
  const last = history[history.length - 1];
  const text = again ? (last?.role === "user" ? last.content : "") : ta.value.trim();
  if (!text || abort || !ready) return; // Enter while the model starts keeps the text
  const g = gen;
  scene?.close();
  scene = null;
  // Only the latest answer offers Ask again and Edit.
  for (const el of document.querySelectorAll("#thread .latest-only")) el.remove();
  if (!again) {
    ta.value = "";
    ta.style.height = "auto";
    history.push({ role: "user", content: text });
    const u = document.createElement("div");
    u.className = "msg user";
    u.textContent = text;
    $("thread").appendChild(u);
  }

  const b = document.createElement("div");
  b.className = "msg bot";
  b.innerHTML = `<div class="think"></div><span class="body"></span><span class="cursor"></span>`;
  $("thread").appendChild(b);
  scroll();

  const body = b.querySelector<HTMLSpanElement>(".body")!;
  const think = b.querySelector<HTMLDivElement>(".think")!;
  // While the model thinks: a lattice of cells lighting in a wave, a few
  // shifting glyphs, and the seconds so far.
  const thinkOn = prefs.think;
  const plan = lengthPlan(prefs.length);
  const temperature = prefs.temperature;
  const lat = lattice("orbit", thinkOn ? "thinking" : "working");
  const glyphs = document.createElement("span");
  glyphs.className = "glyphs";
  think.append(lat.el, glyphs);
  const tick = window.setInterval(() => {
    if (!calm()) glyphs.textContent = Array.from({ length: 5 }, glyph).join("");
    lat.timer.textContent = `${((performance.now() - t0) / 1000).toFixed(1)}s`;
  }, 90);
  const send = $<HTMLButtonElement>("send");
  send.innerHTML = STOP_ICON;
  send.disabled = false;
  send.setAttribute("aria-label", "Stop");

  abort = new AbortController();
  const t0 = performance.now();
  sentAt = t0;
  let reasoningMs = 0;
  let answer = "";
  let looped = false;
  try {
    const system = systemPrompt(current?.name ?? "an open model", current?.publisher || "its publisher") + plan.note;
    const sent = fitHistory(history, plan.historyChars);
    const earlier = history.slice(0, -1).filter((m) => m.role === "user").map((m) => m.content);
    const looked = await lookUpOffline(text, earlier, recentFile);
    const notes = looked.notes;
    if (notes) {
      // Say what was looked up, and where: AInode's own library, on this Mac.
      const c = document.createElement("div");
      c.className = "consulted";
      c.textContent = `consulted · ${looked.source} · ${looked.titles.join(", ")}`;
      b.insertBefore(c, body);
    }
    const last = sent[sent.length - 1];
    const msgs: Msg[] = [{ role: "system", content: system }, ...sent.slice(0, -1), { ...last, content: last.content + notes }];
    const opts = { temperature, maxTokens: plan.maxTokens };
    let stream = realEngine ? llamaStream(port, key, msgs, thinkOn, opts, abort.signal) : mockStream(text, thinkOn, abort.signal);
    let retried = false;
    for (;;) {
    for await (const ev of stream) {
      if (ev.reasoning) {
        reasoningMs = performance.now() - t0;
      }
      if (ev.content) {
        if (!answer) {
          window.clearInterval(tick);
          think.textContent = reasoningMs > 400 ? `thought for ${Math.max(1, Math.round(reasoningMs / 1000))}s` : "";
          if (!think.textContent) think.remove();
        }
        answer += ev.content;
        const cut = loopCut(answer);
        if (cut !== null) {
          // Stop a model that repeats itself rather than let it run to the limit.
          answer = answer.slice(0, cut);
          looped = true;
          abort?.abort();
        }
        renderMd(body, cleanAnswer(answer), !looped);
        scroll();
      }
    }
    // Thinking can use up the whole answer budget and leave no answer: the 3B
    // did on a fact it did not know (lab, 2026-10-12). Ask once more without
    // thinking, so the user always gets one, even if it is "I do not know".
    if (answer || !thinkOn || retried || !realEngine || abort?.signal.aborted) break;
    retried = true;
    stream = llamaStream(port, key, msgs, false, opts, abort!.signal);
    }
  } catch (e) {
    if ((e as Error).name !== "AbortError") toast(e instanceof ChatError ? e.message : `The model stopped: ${(e as Error).message}`, "error");
  } finally {
    window.clearInterval(tick);
    b.querySelector(".cursor")?.remove();
    if (answer) renderMd(body, cleanAnswer(answer)); // the final text, decoded
    if (!answer) { think.remove(); body.textContent = abort?.signal.aborted ? "(stopped)" : "(no answer)"; }
    if (looped) {
      const note = document.createElement("div");
      note.className = "loop-note";
      note.textContent = "Stopped: the model kept repeating itself.";
      b.appendChild(note);
    }
    if (g === gen) {
      history.push({ role: "assistant", content: cleanAnswer(answer) });
      if (answer) addActions(b, cleanAnswer(answer), toast);
      keepThisChat(toast);
    }
    abort = null;
    send.innerHTML = SEND_ICON;
    send.setAttribute("aria-label", "Send");
    send.disabled = !ta.value.trim();
    ta.focus();
  }
}

/** Write the conversation to History, if the user keeps chats. */
function keepThisChat(toast: (s: string, t?: "info" | "error") => void) {
  if (!keep() || !apiRef || !current || !history.length) return;
  const msgs = history.filter((m): m is SavedChat["messages"][number] => m.role !== "system" && !!m.content);
  const title = msgs.find((m) => m.role === "user")?.content.slice(0, 80) ?? "A chat";
  apiRef.chatSave({ id: chatId, model_id: current.id, title, messages: msgs }).catch((e) => toast(e instanceof Error ? e.message : String(e), "error"));
}

async function showHistory(toast: (s: string, t?: "info" | "error") => void) {
  if (!apiRef || !current) return;
  const list = await apiRef.chatsList(current.id).catch(() => []);
  const when = (t: number) => new Date(t * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  const esc = (x: string) => x.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
  $("historyList").innerHTML = list.length
    ? list.map((c) => `<div class="history-item${c.id === chatId ? " current" : ""}">
        <button class="h-title" data-h="open" data-id="${c.id}" title="${esc(c.title)}">${esc(c.title)}</button>
        <span class="h-meta">${when(c.updated)} · ${c.count}</span>
        <button class="tool danger" data-h="delete" data-id="${c.id}">Delete</button></div>`).join("")
    : `<p class="net-empty">No kept chats with ${esc(current.name)} yet. Each answer is kept as it arrives.</p>`;
  $("historySheet").hidden = false;
  void toast;
}

/** Open a kept chat: its messages become the conversation again. */
async function openSaved(id: string, toast: (s: string, t?: "info" | "error") => void) {
  if (!apiRef) return;
  let c: SavedChat;
  try { c = await apiRef.chatLoad(id); } catch (e) { toast(e instanceof Error ? e.message : String(e), "error"); return; }
  abort?.abort();
  gen++;
  chatId = c.id;
  scene?.close();
  scene = null;
  history = c.messages.map((m) => ({ role: m.role, content: m.content }));
  const thread = $("thread");
  thread.innerHTML = "";
  for (const m of c.messages) {
    const d = document.createElement("div");
    if (m.role === "user") {
      d.className = "msg user";
      d.textContent = m.content;
      thread.appendChild(d);
    } else {
      d.className = "msg bot";
      const body = document.createElement("span");
      body.className = "body";
      d.appendChild(body);
      thread.appendChild(d);
      renderMd(body, m.content);
      addActions(d, m.content, toast);
    }
  }
  const latest = thread.querySelectorAll(".latest-only");
  latest.forEach((el, i) => { if (i < latest.length - 2) el.remove(); });
  $("historySheet").hidden = true;
  scroll();
}

/** Copy, on every answer; Ask again and Edit, on the latest one only. */
function addActions(b: HTMLElement, text: string, toast: (s: string, t?: "info" | "error") => void) {
  const row = document.createElement("div");
  row.className = "msg-actions";
  row.innerHTML = `<button data-a="copy">Copy</button><button class="latest-only" data-a="again">Ask again</button><button class="latest-only" data-a="edit">Edit question</button>`;
  row.addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-a]");
    if (!btn || abort) return;
    if (btn.dataset.a === "copy") void copyText(text, btn, toast);
    else if (btn.dataset.a === "again") { dropLastAnswer(); void submit(toast, true); }
    else if (btn.dataset.a === "edit") editLast();
  });
  b.appendChild(row);
  // A copy button on each code block.
  for (const pre of b.querySelectorAll<HTMLPreElement>(".body pre")) {
    const c = document.createElement("button");
    c.className = "code-copy";
    c.textContent = "Copy";
    c.addEventListener("click", () => void copyText(pre.firstChild?.textContent ?? pre.textContent ?? "", c, toast));
    pre.appendChild(c);
  }
}

async function copyText(text: string, btn: HTMLButtonElement, toast: (s: string, t?: "info" | "error") => void) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // WebKit can refuse the async clipboard; the old way still works on a click.
    const t = document.createElement("textarea");
    t.value = text;
    t.style.position = "fixed";
    t.style.opacity = "0";
    document.body.appendChild(t);
    t.select();
    const ok = document.execCommand("copy");
    t.remove();
    if (!ok) { toast("Could not copy", "error"); return; }
  }
  const was = btn.textContent;
  btn.textContent = "Copied";
  setTimeout(() => (btn.textContent = was), 1400);
}

/** The last answer leaves the history and the thread (Ask again, Edit). */
function dropLastAnswer() {
  if (history[history.length - 1]?.role === "assistant") history.pop();
  const bots = document.querySelectorAll("#thread .msg.bot");
  bots[bots.length - 1]?.remove();
}

/** The last question goes back into the box, with its answer gone. */
function editLast() {
  dropLastAnswer();
  const q = history[history.length - 1];
  if (q?.role !== "user") return;
  history.pop();
  const users = document.querySelectorAll("#thread .msg.user");
  users[users.length - 1]?.remove();
  const ta = $<HTMLTextAreaElement>("prompt");
  ta.value = q.content;
  ta.dispatchEvent(new Event("input"));
  ta.focus();
}

/**
 * While streaming, the last few characters "decode": some show as glyphs for a
 * frame, and the next token redraws them. A little cypherpunk, as Mende asked;
 * the finished answer is always rendered clean, and reduced motion turns it off.
 */
function decodeTail(el: HTMLElement, n = 10): void {
  if (calm()) return;
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  for (let t = walker.nextNode(); t; t = walker.nextNode()) nodes.push(t as Text);
  let left = n;
  for (let i = nodes.length - 1; i >= 0 && left > 0; i--) {
    const t = nodes[i];
    const s = t.data;
    const k = Math.min(left, s.length);
    left -= k;
    const span = document.createElement("span");
    // Some of the band flashes white for a frame: letters glitching into place.
    span.className = Math.random() < 0.3 ? "decode hot" : "decode";
    span.textContent = [...s.slice(s.length - k)].map((c) => (c.trim() && Math.random() < 0.55 ? glyph() : c)).join("");
    t.data = s.slice(0, s.length - k);
    t.after(span);
  }
}

function appendInline(el: HTMLElement, parts: Inline[]): void {
  for (const p of parts) {
    if (p.t === "text") { el.append(p.v); continue; }
    const n = document.createElement(p.t === "b" ? "strong" : p.t === "i" ? "em" : "code");
    n.textContent = p.v;
    el.appendChild(n);
  }
}

/** Model output as text nodes and a few safe elements, never parsed as HTML. */
function renderMd(el: HTMLElement, text: string, streaming = false): void {
  const frag = document.createDocumentFragment();
  const blocks = mdBlocks(text);
  blocks.forEach((b, i) => {
    if (b.k === "code") {
      const pre = document.createElement("pre");
      pre.textContent = b.v;
      frag.appendChild(pre);
      return;
    }
    if (b.k === "table") {
      const table = document.createElement("table");
      b.rows.forEach((row, r) => {
        const tr = table.insertRow();
        for (const cell of row) {
          const td = document.createElement(r === 0 ? "th" : "td");
          appendInline(td, cell);
          tr.appendChild(td);
        }
      });
      frag.appendChild(table);
      return;
    }
    const line = document.createElement(b.head ? "strong" : "span");
    if (b.bullet) line.append(b.bullet);
    appendInline(line, b.parts);
    frag.appendChild(line);
    if (i < blocks.length - 1 && blocks[i + 1].k === "line") frag.append("\n");
  });
  el.replaceChildren(frag);
  if (streaming) decodeTail(el);
}

class ChatError extends Error {}

interface Delta { content?: string; reasoning?: string }

async function* llamaStream(
  p: number, k: string, msgs: Msg[], reason: boolean, o: { temperature: number; maxTokens: number }, signal: AbortSignal,
): AsyncGenerator<Delta> {
  // The server only answers requests carrying this session's key, so a web page
  // that finds the port cannot use the model.
  const res = await fetch(`http://127.0.0.1:${p}/v1/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${k}` },
    body: JSON.stringify({
      messages: msgs, stream: true, temperature: o.temperature, max_tokens: o.maxTokens,
      // Think, in each family's own words: Granite reads enable_thinking,
      // gpt-oss reads reasoning_effort (it always reasons; "low" keeps it
      // brief). A template ignores the keys it does not know.
      chat_template_kwargs: { enable_thinking: reason, reasoning_effort: reason ? "medium" : "low" },
    }),
    signal,
  });
  if (!res.ok || !res.body) {
    // llama-server explains itself in {"error":{"message":...}}; say it in plain words.
    const body = await res.text().catch(() => "");
    let message = "";
    try { message = JSON.parse(body)?.error?.message ?? ""; } catch { message = body.slice(0, 200); }
    throw new ChatError(chatErrorText(res.status, message));
  }
  for await (const data of parseSse(res.body)) {
    if (data === "[DONE]") return;
    const j = JSON.parse(data);
    const d = j.choices?.[0]?.delta ?? {};
    yield { content: d.content ?? undefined, reasoning: d.reasoning_content ?? undefined };
  }
}

async function* mockStream(prompt: string, reason: boolean, signal: AbortSignal): AsyncGenerator<Delta> {
  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; reason && i < 6; i++) { if (signal.aborted) return; await wait(120); yield { reasoning: "…" }; }
  const reply = `This is the preview build without a model. In the app, ${prompt.length > 40 ? "your question" : `"${prompt}"`} goes to the model running on ${THIS}, and the answer streams back here word by word.`;
  for (const w of reply.match(/\S+\s*/g) ?? []) { // no lookbehind: macOS 13.0 WebKit cannot parse it
    if (signal.aborted) return;
    await wait(35);
    yield { content: w };
  }
}
