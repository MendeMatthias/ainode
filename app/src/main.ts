import "./styles.css";
import { getApi, inTauri, realEngine, watchMinimized, type Api, type AppState, type CatalogModel, type Community, type Face, type ModelPrefs, type Place, type Theme, type UpdateInfo } from "./api";
import { actionFor, ainodesText, btxNodesOf, engineLine, btxNodesText, CHECKIN_EVERY_MS, loginNoteText, easyNodeRestarted, gpuNoteText, nodeHereText, countDue, etaText, fmtBytes, friendlyError, prefsFor, PRESETS, readSize, roomLeft, startSize, newerCommunity, statusOf, TEMPERATURES, feedbackUrl, type FeedbackKind, pollDue, updateDue, updateNote, updateOfferable, autoInstallNow, awayNow, wslBarText, type Sample, type Status } from "./logic";
import { openChat, syncMine } from "./chat";
import { CARD_LINE, CARD_LINE_TITLE, CHIP_NAME, chipLabel, chipOf, chipTitle, firstLede, type Coverage } from "./chip";
import { copyText } from "./clip";
import { modelLink } from "./safe";
import { createStartSearch, type Window as WinState } from "./startsearch";
import { closeNetwork, forgetNetwork, networkStateChanged, wireNetwork } from "./network";
import { OS, PLATFORM, THIS, localizeStatic } from "./platform";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const POLL_MS = 1500;
const ARC = 603.2; // circumference of the core's progress ring (r = 96)

let api: Api;
let state: AppState | null = null;
let models: CatalogModel[] = [];
let lastModelsKey = "";
let busy = false; // a user action is in flight: don't let a poll repaint over it
/** The counters of the last network answer in this engine run (the start-up search or the view's): the chip's only number. */
let netCoverage: Coverage | null = null;
/** The engine run netCoverage belongs to. */
let coverageRun = 0;
/** Visible, and minimized or not once the window has said so (boot's away()). Unknown counts as not shown. */
const win: WinState = { visible: false, minimized: null };
const samples = new Map<string, Sample[]>(); // verified-progress history per downloading uri

function track(ms: CatalogModel[]) {
  const now = Date.now();
  for (const m of ms) {
    if (m.local !== "downloading") { samples.delete(m.uri); continue; }
    const list = samples.get(m.uri) ?? [];
    list.push({ t: now, p: m.progress });
    while (list.length && now - list[0].t > 60_000) list.shift();
    samples.set(m.uri, list);
  }
}

function progressLine(m: CatalogModel): string {
  // The engine keeps retrying a host that went away for a long time without
  // failing (docs/experiments.md E10); say so instead of sitting still.
  if (m.waiting) return "Host not answering. Still trying; verified pieces are kept.";
  if (m.progress <= 0) return "Starting. On a slow line the first piece can take a minute.";
  const eta = etaText(samples.get(m.uri) ?? [], m.size_bytes, Date.now());
  return `${fmtBytes(m.size_bytes * m.progress)} of ${fmtBytes(m.size_bytes)} verified${eta ? ` · ${eta}` : ""}`;
}

// ── Theme ─────────────────────────────────────────────────────────────────
const media = window.matchMedia("(prefers-color-scheme: dark)");
function applyTheme(t: Theme) {
  const dark = t === "dark" || (t === "system" && media.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  for (const b of document.querySelectorAll<HTMLButtonElement>("#themeSeg button")) {
    b.setAttribute("aria-pressed", String(b.dataset.t === t));
  }
}
media.addEventListener("change", () => state && applyTheme(state.theme));

async function setTheme(t: Theme) {
  await api.setTheme(t);
  if (state) state.theme = t;
  applyTheme(t);
}

// ── The face (chat loading scene) ────────────────────────────────────────
function applyFace(f: Face) {
  for (const b of document.querySelectorAll<HTMLButtonElement>("#faceSeg button")) {
    b.setAttribute("aria-pressed", String(b.dataset.f === f));
  }
}
async function setFace(f: Face) {
  await api.setFace(f);
  if (state) state.face = f;
  applyFace(f);
}

// ── Toast ─────────────────────────────────────────────────────────────────
let toastTimer = 0;
function toast(msg: string, tone: "info" | "error" = "info") {
  const el = $("toast");
  el.textContent = msg;
  el.dataset.tone = tone;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (el.hidden = true), tone === "error" ? 6000 : 3200);
}
function errText(e: unknown): string {
  return e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e);
}

// ── Core ──────────────────────────────────────────────────────────────────
function renderCore(s: AppState) {
  const core = $("core");
  const engine = s.engine.state;
  core.dataset.state = engine;
  const on = core.querySelector(".on")!;
  on.textContent = engine === "starting" ? "STARTING" : engine === "running" ? "ONLINE" : engine === "error" ? "RETRY" : "START";
  core.setAttribute("aria-label", engine === "running" ? "Stop" : "Start");

  // The ring shows overall download progress while anything downloads.
  const dl = models.filter((m) => m.local === "downloading");
  const arc = $("coreArc");
  if (dl.length) {
    const total = dl.reduce((a, m) => a + m.size_bytes, 0);
    const done = dl.reduce((a, m) => a + m.size_bytes * m.progress, 0);
    arc.style.opacity = "1";
    arc.setAttribute("stroke-dashoffset", String(ARC * (1 - done / Math.max(1, total))));
    $("coreSub").textContent = `${Math.floor((done / Math.max(1, total)) * 100)}% · verifying`;
  } else {
    arc.style.opacity = engine === "running" ? ".9" : "0";
    arc.setAttribute("stroke-dashoffset", engine === "running" ? "0" : String(ARC));
    $("coreSub").textContent = engine === "running" && s.engine.pq1_ready !== false ? "post-quantum" : "";
  }

  const st = statusOf(s, dl.length > 0, community, s.keeper?.restart_pending ?? false, byEasyNode);
  $("status").dataset.tone = st.tone;
  renderStatusText($("statusText"), st);
  renderWsl(s);
  renderChip(s);
  networkStateChanged();
}

// ── The ModelNet chip (chip.ts): what the engine reported, nothing else ────
const MARK = `<svg class="net-mark" viewBox="0 0 16 16" aria-hidden="true"><path d="M8 2.6 2.8 12.6h10.4Z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/><circle cx="8" cy="2.6" r="1.6" fill="currentColor"/><circle cx="2.8" cy="12.6" r="1.6" fill="currentColor"/><circle cx="13.2" cy="12.6" r="1.6" fill="currentColor"/></svg>`;
// The count on the chip without opening the view: one search with no words
// per engine run, asked for here while the window is shown, and kept to one
// per run by the backend (backend/network.rs network_start_search). A failure
// leaves the chip without a count, as before.
const startSearch = createStartSearch({
  ask: () => api.networkStartSearch(),
  currentRun: () => state?.network.engine_run ?? 0,
  onCoverage: (c, run) => {
    // A newer answer from the view in the same run wins.
    if (netCoverage && coverageRun === run && netCoverage.atMs > c.atMs) return;
    netCoverage = c;
    coverageRun = run;
    if (state) renderChip(state);
  },
  onError: (e) => console.warn("start-up search:", errText(e)),
});
function maybeStartSearch(s: AppState) {
  startSearch.maybe(s.engine.state, s.network.engine_run ?? 0, win);
}

function renderChip(s: AppState) {
  const run = s.network.engine_run ?? 0;
  if (netCoverage && run !== coverageRun) {
    // An answer from an earlier engine run describes nothing now.
    netCoverage = null;
  }
  if (s.engine.state !== "running" && netCoverage) {
    // What a stopped engine answered earlier describes nothing now.
    netCoverage = null;
    forgetNetwork();
  }
  const dl = models.filter((m) => m.local === "downloading");
  const c = chipOf({
    engine: s.engine.state,
    hosts: s.network.peers_configured,
    fetching: dl.length > 0,
    waiting: dl.length > 0 && dl.every((m) => !!m.waiting),
    coverage: netCoverage,
    nowMs: Date.now(),
  });
  const chip = $("netChip");
  const label = chipLabel(c);
  if (chip.getAttribute("aria-label") === label && chip.dataset.tone === c.tone) return;
  chip.dataset.tone = c.tone;
  chip.setAttribute("aria-label", label);
  chip.title = chipTitle(c);
  $("netChipText").innerHTML = `${c.lead ? `${esc(c.lead)} ` : ""}<b>${CHIP_NAME}</b>${c.detail ? `<span class="net-chip-detail"> · ${esc(c.detail)}</span>` : ""}`;
}

// ── Windows: WSL stuck with other Linux running (Rust backend/wslheal.rs) ──
// AInode restarts WSL by itself only when nothing else runs in it; otherwise
// this bar names what would stop, and only the click restarts it.
let wslRestarting = false;
function renderWsl(s: AppState) {
  const text = s.engine.state === "error" ? wslBarText(s.wsl) : null;
  $("wslBar").hidden = !text || wslRestarting;
  if (text) $("wslText").textContent = text;
  $<HTMLButtonElement>("wslGo").disabled = wslRestarting;
}
async function onWslRestart() {
  const others = state?.wsl?.stuck_others;
  if (!state || !others || wslRestarting) return;
  wslRestarting = true;
  busy = true;
  state.engine.state = "starting";
  state.wsl = { ...state.wsl!, restarting: true };
  renderCore(state);
  try {
    await api.restartWsl(others);
  } catch (e) {
    toast(errText(e), "error");
  } finally {
    wslRestarting = false;
    busy = false;
    await refresh();
  }
}

/** The home line, with its link (the BTX nodes) as a button that opens the page. */
function renderStatusText(el: HTMLElement, st: Status) {
  const at = st.link ? st.line.lastIndexOf(st.link.text) : -1;
  if (!st.link || at < 0) {
    el.textContent = st.line;
    return;
  }
  const b = document.createElement("button");
  b.type = "button";
  b.className = "statusLink";
  b.dataset.url = st.link.url;
  b.title = st.link.url.replace("https://", "");
  b.textContent = st.link.text;
  el.replaceChildren(st.line.slice(0, at), b, st.line.slice(at + st.link.text.length));
}

async function onCore() {
  if (!state || busy) return;
  if (!state.consent) return showSpace(false);
  busy = true;
  try {
    if (state.engine.state === "running") {
      await api.setPref("autostart", false);
      await api.stopEngine();
    } else if (state.engine.state !== "starting") {
      await api.setPref("autostart", true);
      state.engine.state = "starting";
      renderCore(state);
      await api.startEngine();
    }
  } catch (e) {
    // A stuck WSL is explained on the status line, with its bar and button
    // under it: a toast would only cover them.
    if (!/^Windows' Linux/.test(errText(e))) toast(errText(e), "error");
  } finally {
    busy = false;
    await refresh();
  }
}

// ── Models ────────────────────────────────────────────────────────────────
const svgMini = (p: number) => {
  const c = 2 * Math.PI * 5.5;
  return `<svg class="mini" viewBox="0 0 14 14"><circle class="bg" cx="7" cy="7" r="5.5"/><circle class="fg" cx="7" cy="7" r="5.5" stroke-dasharray="${c}" stroke-dashoffset="${c * (1 - p)}"/></svg>`;
};

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[ch]!);
}

/** On the home screen: models with anything on this Mac. The store lists the rest. */
const mine = (m: CatalogModel) => m.local !== "none" || m.runnable || m.progress > 0 || m.offered === false;

function actButton(m: CatalogModel, s: AppState): string {
  const a = actionFor(m, s);
  return `<div class="act-wrap"><button class="act" data-kind="${a.kind}" data-do="${a.kind}" data-uri="${esc(m.uri)}" ${a.kind === "blocked" ? 'aria-disabled="true"' : ""}>
      ${a.kind === "progress" ? svgMini(m.progress) : ""}${esc(a.label)}
    </button>${a.hint ? `<div class="act-hint">${esc(a.hint)}</div>` : ""}</div>`;
}

function statusLines(m: CatalogModel): string {
  return `${m.error && m.local !== "ready" ? `<div class="model-error">${esc(friendlyError(m.error))}</div>` : ""}
    ${m.local === "downloading" ? `<div class="model-progress${m.waiting ? " waiting" : ""}">${esc(progressLine(m))}</div>` : ""}`;
}

function renderModels(s: AppState) {
  const key = JSON.stringify([models, s.engine.state, s.network.peers_configured, s.consent?.total_bytes, Math.round(s.usage.total_bytes / 1e8)]);
  if (key === lastModelsKey) return;
  lastModelsKey = key;
  const parts: string[] = [];
  if (s.engine.state === "running" && s.network.peers_configured === 0) {
    parts.push(`<div class="empty-note">No public BTX model host is listed yet, so nothing can be fetched. If you know one, add it under Status.</div>`);
  }
  const own = models.filter(mine);
  if (!own.length) {
    parts.push(`<div class="empty-card"><p>No model on ${THIS} yet.</p><button class="primary small" data-do="store">Open the store</button></div>`);
  }
  for (const m of own) {
    // Proper buttons (Mende, 2026-10-13: the faint text links were hard to read).
    // Remove whenever a runnable copy exists, even if the engine evicted the
    // pieces: that copy is the user's disk space.
    const ready = m.local === "ready" || m.runnable;
    const tools = m.local === "downloading"
      ? `<div class="btn-row"><button class="tool danger" data-do="cancel" data-uri="${esc(m.uri)}">Cancel</button></div>`
      : ready
        ? `<div class="btn-row">${m.offered !== false ? `<button class="tool" data-do="settings" data-uri="${esc(m.uri)}">${GEAR}Settings</button>` : ""}<button class="tool" data-do="reveal" data-uri="${esc(m.uri)}">Show file</button><button class="tool" data-do="source" data-url="${esc(m.source_url)}">Source</button><button class="tool danger" data-do="remove" data-uri="${esc(m.uri)}">Remove</button></div>`
        : "";
    parts.push(`
      <article class="model">
        <div class="model-name">${esc(m.name)}</div>
        ${actButton(m, s)}
        <div class="model-meta">${esc(m.publisher)} · ${esc(m.licence)} · ${fmtBytes(m.size_bytes)}</div>
        ${statusLines(m)}
        <div class="model-sum">${esc(m.offered === false ? "No longer offered: in testing it repeated itself. Remove it to free its space." : m.summary)}</div>
        ${ready && m.local !== "downloading" ? `<div class="model-net" title="${esc(CARD_LINE_TITLE)}"><span>${MARK}${esc(CARD_LINE)}</span>${modelLink(m.uri) ? `<button class="link-btn" data-do="copylink" data-uri="${esc(m.uri)}" title="Copy this model's btx:// link. Whoever opens it gets the same checked files.">Copy link</button>` : ""}</div>` : ""}
        ${tools}
      </article>`);
  }
  $("models").innerHTML = parts.join("");
  renderStore(s);
}

/** The store: every offered model, with what a person needs to choose one. */
function renderStore(s: AppState) {
  const offered = models.filter((m) => m.offered !== false);
  const ram = s.ram_bytes ? Math.round(s.ram_bytes / 1024 ** 3) : 0;
  $("storeList").innerHTML = offered.map((m) => `
      <article class="model store-item">
        <div class="model-name">${esc(m.name)}</div>
        ${actButton(m, s)}
        <div class="model-meta">${esc(m.publisher)} · <button class="inline-link" data-do="source" data-url="${esc(m.licence_url)}">${esc(m.licence)}</button></div>
        <div class="facts">
          <span><b>${fmtBytes(m.size_bytes)}</b> download</span>
          <span><b>${m.ram_gb ? `${m.ram_gb} GB` : "Any"}</b> memory${m.ram_gb && ram && m.ram_gb > ram ? ` (${THIS} has less)` : ""}</span>
        </div>
        ${m.good_at ? `<div class="good-at">Good at: ${esc(m.good_at)}</div>` : ""}
        <div class="model-sum">${esc(m.summary)}</div>
        ${statusLines(m)}
        <div class="btn-row"><button class="tool" data-do="source" data-url="${esc(m.source_url)}">Publisher's page</button>${m.local === "downloading" ? `<button class="tool danger" data-do="cancel" data-uri="${esc(m.uri)}">Cancel</button>` : ""}</div>
      </article>`).join("");
}

const GEAR = `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true"><circle cx="8" cy="8" r="2.2"/><path d="M8 1.8v1.6M8 12.6v1.6M1.8 8h1.6M12.6 8h1.6M3.6 3.6l1.1 1.1M11.3 11.3l1.1 1.1M12.4 3.6l-1.1 1.1M4.7 11.3l-1.1 1.1"/></svg>`;

// ── One model's settings ──────────────────────────────────────────────────
let sheetModel: CatalogModel | null = null;
// The sheet's own copy: changes apply to it at once and are saved in order, so
// two quick clicks cannot both start from the same old settings.
let sheetPrefs: ModelPrefs | null = null;
let saving: Promise<void> = Promise.resolve();
function openModelSheet(m: CatalogModel) {
  sheetModel = m;
  sheetPrefs = prefsFor(state, m.id);
  $("modelSheetTitle").textContent = `${m.name} settings`;
  $("mpTemp").innerHTML = TEMPERATURES.map((t) => `<button data-v="${t.value}">${t.label}</button>`).join("");
  $("mpFiles").innerHTML = `<button class="tool" data-do="reveal" data-uri="${esc(m.uri)}">Show file</button><button class="tool" data-do="source" data-url="${esc(m.source_url)}">Source</button><button class="tool danger" data-do="remove" data-uri="${esc(m.uri)}">Remove</button>`;
  $("mpInfo").textContent = `${m.publisher} · ${m.licence} · ${fmtBytes(m.size_bytes)} · ${m.uri}`;
  paintModelSheet();
  $("modelSheet").hidden = false;
}
function paintModelSheet() {
  if (!sheetModel || !sheetPrefs) return;
  const p = sheetPrefs;
  const press = (id: string, v: string) => {
    for (const b of $(id).querySelectorAll<HTMLButtonElement>("button")) b.setAttribute("aria-pressed", String(b.dataset.v === v));
  };
  press("mpThink", p.think ? "on" : "off");
  press("mpLength", p.length);
  const t = TEMPERATURES.reduce((a, b) => (Math.abs(b.value - p.temperature) < Math.abs(a.value - p.temperature) ? b : a));
  press("mpTemp", String(t.value));
  $("mpTempHint").textContent = t.hint;
}
function changeModelPrefs(change: Partial<ModelPrefs>) {
  if (!sheetModel || !sheetPrefs) return;
  const id = sheetModel.id;
  const next = { ...sheetPrefs, ...change };
  sheetPrefs = next;
  paintModelSheet();
  saving = saving.then(() => savePrefs(id, next)).catch((e) => toast(errText(e), "error"));
}

async function onModelClick(ev: MouseEvent) {
  const btn = (ev.target as HTMLElement).closest<HTMLButtonElement>("button[data-do]");
  if (!btn || busy) return;
  const uri = btn.dataset.uri ?? "";
  const m = models.find((x) => x.uri === uri);
  try {
    switch (btn.dataset.do) {
      case "get":
        busy = true;
        await api.download(uri);
        // Show it at once: the next snapshot can predate the job by a poll.
        if (m && state) {
          m.local = "downloading";
          m.error = undefined;
          lastModelsKey = "";
          renderModels(state);
          renderCore(state);
        }
        break;
      case "chat":
        if (m) {
          $("store").hidden = true;
          await openChat(api, m, prefsFor(state, m.id), state?.face ?? "random", toast, savePrefs, () => state?.keep_chats === true);
        }
        break;
      case "settings":
        if (m) openModelSheet(m);
        return;
      case "store":
        $("store").hidden = false;
        return;
      case "reveal":
        await api.reveal(uri);
        break;
      case "source":
        await api.openUrl(btn.dataset.url ?? "");
        break;
      case "copylink": {
        // Only a link the backend's own check accepts goes to the clipboard.
        const link = modelLink(uri);
        if (!link) throw new Error("That link is not in the form of a BTX model link, so it was not copied");
        await copyText(link);
        toast("Link copied");
        return;
      }
      case "cancel":
        busy = true;
        await api.cancel(uri);
        break;
      case "remove":
        if (btn.dataset.armed !== "1") {
          btn.dataset.armed = "1";
          btn.textContent = `Remove from ${THIS}?`;
          setTimeout(() => { btn.dataset.armed = ""; btn.textContent = "Remove"; }, 3500);
          return;
        }
        busy = true;
        await api.remove(uri);
        // The copy and the engine's record of it go now (store_once.rs, 0.7.1):
        // its space is free at once.
        toast(`${m?.name ?? "Model"} removed. Its ${fmtBytes(m?.size_bytes ?? 0)} are free again.`);
        $("modelSheet").hidden = true;
        break;
      case "blocked":
        return;
    }
  } catch (e) {
    toast(errText(e), "error");
  } finally {
    busy = false;
  }
  await refresh();
}

/** The chat's Think switch saves into the model's own settings. */
async function savePrefs(id: string, p: ModelPrefs) {
  await api.setModelPrefs(id, p);
  if (state) state.models = { ...state.models, [id]: p };
}

// ── Footer meter ──────────────────────────────────────────────────────────
function renderMeter(s: AppState) {
  if (!s.consent) { $("meterText").textContent = ""; $("spaceChange").hidden = true; return; }
  const used = s.usage.total_bytes;
  const cap = s.consent.total_bytes;
  $<HTMLDivElement>("meterFill").style.width = `${Math.min(100, (used / cap) * 100)}%`;
  $("meterText").textContent = `${fmtBytes(used)} of ${fmtBytes(cap)} used`;
  $("spaceChange").hidden = false;
}

// ── Space: chosen at first run, changeable from the footer ───────────────
// Rust holds the rule (state::consent_limits) and reports the limits as
// state.space; this only offers sizes inside them and says why when it cannot.
const FIRST_LEDE = firstLede(THIS);
let chosen: number | null = null;
let typedError: string | null = null;
let changing = false;
/** The first start only: another drive or folder for the data, and the sizes that drive allows. */
let chosenDir: string | null = null;
let placeLimits: { min_gb: number; max_gb: number; free_bytes: number } | null = null;
function limits() {
  return placeLimits ?? { min_gb: state!.space.min_gb, max_gb: state!.space.max_gb, free_bytes: state!.disk.free_bytes };
}
function showSpace(change: boolean) {
  if (!state) return;
  changing = change;
  chosenDir = null;
  placeLimits = null;
  $("where").hidden = change || !!state.consent;
  $("places").hidden = true;
  $("wherePath").textContent = state.data_dir;
  renderSizes();
  $("consentLede").textContent = change ? `How much of ${THIS}'s disk AInode may use for models.` : FIRST_LEDE;
  $("consentGo").textContent = change ? "SAVE" : "START";
  $("consentCancel").hidden = !change;
  updateSpaceFine();
  $("consent").hidden = false;
}
function renderSizes() {
  if (!state) return;
  const { min_gb: min, max_gb: max } = limits();
  const current = state.consent ? Math.round(state.consent.total_bytes / 1e9) : null;
  chosen = startSize(min, max, current);
  typedError = null;
  $("choices").innerHTML = PRESETS.map((p) => {
    const fits = p.gb >= min && p.gb <= max;
    return `<button class="choice" data-g="${p.gb}" aria-pressed="${p.gb === chosen}"${fits ? "" : " disabled"}><span>${p.name}</span>${p.gb} GB</button>`;
  }).join("");
  const input = $<HTMLInputElement>("customGb");
  input.value = chosen !== null && !PRESETS.some((p) => p.gb === chosen) ? String(chosen) : "";
  input.placeholder = max >= min ? String(Math.min(max, 200)) : "";
  input.disabled = max < min;
}
/** Change on the Where row: the drives, each with its free space; picking one re-offers the sizes for it. */
async function onWhereChange() {
  try {
    const places = await api.places();
    $("places").innerHTML = places.map((p) => `<button class="choice" data-dir="${esc(p.data_dir)}" data-min="${p.min_gb}" data-max="${p.max_gb}" data-free="${p.free_bytes}" aria-pressed="${p.current}"><b>${esc(p.root)}</b><span>${fmtBytes(p.free_bytes)} free</span></button>`).join("");
    $("places").hidden = false;
  } catch (e) {
    toast(errText(e), "error");
  }
}
function pickPlace(b: HTMLButtonElement) {
  for (const x of $("places").querySelectorAll("button")) x.setAttribute("aria-pressed", String(x === b));
  const p: Place = { root: "", free_bytes: Number(b.dataset.free), total_bytes: 0, data_dir: b.dataset.dir ?? "", min_gb: Number(b.dataset.min), max_gb: Number(b.dataset.max), current: false };
  chosenDir = p.data_dir === state?.data_dir ? null : p.data_dir;
  placeLimits = { min_gb: p.min_gb, max_gb: p.max_gb, free_bytes: p.free_bytes };
  $("wherePath").textContent = p.data_dir;
  // The row says where now; the list folds away so the sizes and START stay in view.
  $("places").hidden = true;
  renderSizes();
  updateSpaceFine();
}
function pickPreset(gb: number) {
  chosen = gb;
  typedError = null;
  $<HTMLInputElement>("customGb").value = "";
  for (const x of $("choices").querySelectorAll("button")) x.setAttribute("aria-pressed", String(Number(x.dataset.g) === gb));
  updateSpaceFine();
}
function onTyped() {
  if (!state) return;
  const text = $<HTMLInputElement>("customGb").value;
  for (const x of $("choices").querySelectorAll("button")) x.setAttribute("aria-pressed", "false");
  if (!text.trim()) { chosen = null; typedError = null; updateSpaceFine(); return; }
  const r = readSize(text, limits().min_gb, limits().max_gb);
  if ("gb" in r) { chosen = r.gb; typedError = null; } else { chosen = null; typedError = r.error; }
  for (const x of $("choices").querySelectorAll("button")) x.setAttribute("aria-pressed", String(Number(x.dataset.g) === chosen));
  updateSpaceFine();
}
function updateSpaceFine() {
  if (!state) return;
  const fine = $("consentFine");
  const { min_gb: min, max_gb: max, free_bytes } = limits();
  const free = fmtBytes(free_bytes);
  fine.classList.toggle("warn", typedError !== null || max < min);
  if (max < min) {
    fine.textContent = changing
      ? `Only ${free} free: too little to change the size now.`
      : `Only ${free} free: not enough for any model. Free some space and reopen AInode.`;
  } else if (typedError) {
    fine.textContent = typedError;
  } else if (changing) {
    fine.textContent = `AInode holds ${fmtBytes(state.usage.total_bytes)} now. Choose ${min} to ${max} GB. ${free} free on ${THIS}.`;
  } else {
    fine.textContent = chosen === null
      ? `Choose ${min} to ${max} GB of ${THIS}'s ${free} free.`
      : `At most ${chosen} GB of ${THIS}'s ${free} free, only for models you choose. Never uploads.`;
  }
  $<HTMLButtonElement>("consentGo").disabled = chosen === null;
}
async function onConsentGo() {
  if (chosen === null) return;
  const go = $<HTMLButtonElement>("consentGo");
  go.disabled = true;
  try {
    const r = await api.acceptConsent(chosen, chosenDir ?? undefined);
    if (r?.relaunch) {
      // Another folder: the app reads its paths again and comes back to Start.
      toast(`AInode's models go to ${chosenDir}. Opening again.`);
      await api.relaunch();
      return;
    }
    $("consent").hidden = true;
    await refresh();
    if (changing) toast(`AInode may now use ${chosen} GB.`);
    else await onCore();
  } catch (e) {
    toast(errText(e), "error");
  } finally {
    go.disabled = false;
  }
}

// ── About / status ────────────────────────────────────────────────────────
function renderAbout(s: AppState) {
  const rows: [string, string][] = [
    ["Engine", s.engine.state === "running" ? "Running" : s.engine.state === "error" ? `Error: ${s.engine.error ?? ""}` : s.engine.state === "starting" ? "Starting" : "Stopped"],
    ["AInodes online", community
      ? `${ainodesText(community.online)} now · Mac ${community.platforms.macos} · Windows ${community.platforms.windows} · Linux ${community.platforms.linux}`
      : s.consent ? "Not known yet" : "-"],
    ["BTX nodes", (() => {
      const b = btxNodesOf(community);
      return b ? `${btxNodesText(b.live)} online${b.countries ? ` in ${b.countries} countries` : ""} · they keep the chain, easyNodes among them` : s.consent ? "Not known yet" : "-";
    })()],
    ...(nodeHereText(nodeHeight) ? [["BTX node here", nodeHereText(nodeHeight) ?? ""] as [string, string]] : []),
    // The engine that reaches the network, under the network's own name (plan section 6).
    [CHIP_NAME, engineLine(s.engine)],
    ["Transport", s.engine.state !== "running" ? "Post-quantum TLS, when running"
      : s.engine.pq1_ready === false ? "Post-quantum TLS not ready" : "ML-KEM-768 + ML-DSA-44"],
    ["Host", s.network.active_host
      ? `${s.network.active_host} · ${s.network.peers_connected} open connection${s.network.peers_connected === 1 ? "" : "s"}`
        + (s.network.switched ? ` · switched here because ${s.network.switched.split(" -> ")[0]} stopped answering` : "")
        + ((s.network.hosts_known ?? 0) > 1 ? ` · ${s.network.hosts_known} hosts known, AInode moves to the next if one stops answering` : "")
      : "None yet"],
    ["Space", s.consent ? `${fmtBytes(s.usage.total_bytes)} used of ${fmtBytes(s.consent.total_bytes)} allowed · each model kept once, right after its download` : "Not chosen yet"],
    ["Room left", s.consent ? fmtBytes(roomLeft(s) ?? 0) : "-"],
    ["Checks", "SHA-384, every 4 MiB piece"],
    ["Knowledge", "Your files, AInode's library, BTX's docs, postquantum.wiki and 47,000 Wikipedia articles, all offline"],
    ["AInode", `v${s.version}`],
  ];
  $("kv").innerHTML = rows.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("");
}

// ── Send feedback (plan 3.2) ─────────────────────────────────────────────
// The browser opens a new issue, filled in; nothing leaves AInode itself.
async function sendFeedback(kind: FeedbackKind, withModels: boolean) {
  try {
    const info = await api.systemInfo();
    const names = withModels ? models.filter((m) => m.local === "ready" || m.runnable).map((m) => m.name) : [];
    await api.openUrl(feedbackUrl(kind, state?.version ?? "", PLATFORM, info, names));
    toast("Your browser opens a new issue. Nothing is sent until you submit it there.");
  } catch (e) {
    toast(errText(e), "error");
  }
}

// ── Kept chats (Status) ───────────────────────────────────────────────────
function applyKeepChats(on: boolean) {
  $("chatHistory").hidden = !on;
  for (const b of document.querySelectorAll<HTMLButtonElement>("#chatsSeg button")) b.setAttribute("aria-pressed", String((b.dataset.k === "on") === on));
  $("chatsNote").textContent = on
    ? `Each conversation is kept on ${THIS}, under History in the chat. Nothing leaves it. Delete them there; Clear starts a new one.`
    : "Nothing of a conversation is written anywhere: Clear or quitting forgets it. Chats kept earlier stay in History until you delete them.";
}

function applyLogin(on: boolean, error?: string | null) {
  for (const b of document.querySelectorAll<HTMLButtonElement>("#loginSeg button")) b.setAttribute("aria-pressed", String((b.dataset.login === "on") === on));
  $("loginNote").textContent = loginNoteText(on, error ?? null, THIS);
}

function applyCountMe(on: boolean) {
  for (const b of document.querySelectorAll<HTMLButtonElement>("#countSeg button")) b.setAttribute("aria-pressed", String((b.dataset.count === "on") === on));
  $("countNote").textContent = on
    ? "Every 5 minutes while AInode is open, it tells easybtx.com that it is here: a random number for this install, the platform, the version, whether the engine runs, how many models are ready. Never what you ask, never your files. It reads the AInode and BTX node counts from there too."
    : "AInode does not tell easybtx.com it is here. It still shows how many AInodes and BTX nodes are online; this AInode is just not counted.";
}

function applyGpu(on: boolean) {
  for (const b of document.querySelectorAll<HTMLButtonElement>("#gpuSeg button")) b.setAttribute("aria-pressed", String((b.dataset.gpu === "on") === on));
  $("gpuNote").textContent = gpuNoteText(on, nodeHeight !== null, THIS);
}

// ── A BTX node on this computer (Rust neighbour.rs) ─────────────────────
// Asked only while Status is open, after consent, at most every 30 s: one
// loopback request to easyNode's witness, which answers with its height.
let nodeHeight: number | null = null;
let nodeAskedAt = 0;
async function lookForNode() {
  if (!state?.consent || $("about").hidden || Date.now() - nodeAskedAt < 30_000) return;
  nodeAskedAt = Date.now();
  const n = await api.neighbour().catch(() => null);
  const h = n?.node_height ?? null;
  if (h === nodeHeight) return;
  nodeHeight = h;
  if (state && !$("about").hidden) { renderAbout(state); applyGpu(state.gpu); }
}

// ── Your own files (Status) ──────────────────────────────────────────────
async function renderOwn() {
  const files = await api.knowledgeList().catch(() => []);
  $("ownList").innerHTML = files.map((f) => `<li><span class="own-name" title="${esc(f.name)}">${esc(f.name)}</span>
      <span class="own-meta">${f.count} passage${f.count === 1 ? "" : "s"} · ${f.bytes < 1e6 ? `${Math.max(1, Math.round(f.bytes / 1e3))} KB` : fmtBytes(f.bytes)}</span>
      <button class="tool danger" data-own="${esc(f.id)}">Remove</button></li>`).join("");
  $("ownNote").textContent = files.length
    ? `AInode keeps the text it read from these files, on ${THIS} only, and looks in them first when you ask. It counts against your space.`
    : `None yet. Drop a PDF, Markdown or text file into a chat, or press + there: AInode reads it on ${THIS} and looks in it first when you ask.`;
}

// ── Loop ──────────────────────────────────────────────────────────────────
let refreshing = false;
async function refresh() {
  // One poll at a time: a slow engine call must not stack polls behind it.
  if (refreshing) return;
  refreshing = true;
  try {
    await refreshOnce();
  } finally {
    refreshing = false;
  }
}

// ── The AInodes online (easybtx.com) ─────────────────────────────────────
// Rust checks in every 5 minutes after consent, unless "count me" is off
// (backend/keeper.rs: it runs whether or not this page's timers do). Here,
// every minute, the page reads how many AInodes are open, so a newcomer shows
// up within about a minute. A failure keeps the last number for up to 20
// minutes, then shows none.
let community: Community | null = null;
let communityAt = 0;
let countReadAt = 0;
async function maybeCommunity(force = false) {
  if (!state?.consent) return;
  const now = Date.now();
  if (!force && !countDue(now, countReadAt)) return;
  countReadAt = now;
  const c = await api.community().catch(() => null);
  if (c) {
    community = newerCommunity(community, c);
    communityAt = now;
  } else if (now - communityAt > 4 * CHECKIN_EVERY_MS) {
    community = null;
  }
  if (state && !busy) renderCore(state);
  if (state && !$("about").hidden) renderAbout(state);
}

// ── Self-healing: the engine starts again after it stops on its own ─────
// Rust does it (backend/keeper.rs: 20 s, 1 min, 3 min, at most 3 times in 10
// minutes), whether or not this page's timers run; the home line says so
// (state.keeper.restart_pending).
let askedWhy = false;
// Windows: did easyNode restart WSL just before the engine stopped? Asked once
// per stop (Rust neighbour.rs reads easyNode's note in %TEMP%), kept until the
// engine runs again.
let byEasyNode = false;
async function askWhyItStopped() {
  const n = await api.neighbour().catch(() => null);
  byEasyNode = easyNodeRestarted(n?.wsl_restart_at ?? null, Date.now());
  if (state) renderCore(state);
}
function watchStops() {
  if (!state) return;
  if (state.engine.state !== "error") {
    if (state.engine.state === "running") byEasyNode = false;
    askedWhy = false;
    return;
  }
  if (!askedWhy) {
    askedWhy = true;
    if (OS === "windows") void askWhyItStopped();
  }
}

// ── Updates (Rust update.rs) ─────────────────────────────────────────────
// Looked for after consent at start and every hour. Where the computer allows
// it (not a .deb) a newer AInode installs by itself at the first quiet moment
// (maybeAutoInstall). Otherwise, or after a failed attempt, a bar under the
// status line offers Restart, only while no model downloads and no chat is
// open; Later hides it until the next check. Installing downloads it, stops
// the engine the way Quit does, installs and opens the new AInode. Status
// shows the outcome of the last check, a refusal included.
let update: UpdateInfo | null = null;
let updateError: string | null = null;
let updateCheckedAt = 0;
let updateChecking = false;
let updateLater = false;
let updating = false;
// Installed by itself (Mende, 2026-10-18) where the computer allows it (not a
// .deb): at the first quiet moment after a check finds it. `autoTried` keeps a
// failed attempt from repeating until the next check.
let autoTried: string | null = null;
let updatedShown = false;
async function checkUpdate(force = false) {
  if (!state?.consent || updateChecking || updating) return;
  const now = Date.now();
  if (!force && !updateDue(now, updateCheckedAt)) return;
  updateCheckedAt = now;
  updateChecking = true;
  renderUpdate();
  try {
    update = await api.updateCheck();
    updateError = null;
    updateLater = false;
    autoTried = null;
    if (update.updated_from && !updatedShown) {
      updatedShown = true;
      toast(`Updated to AInode ${state?.version ?? ""}.`);
    }
  } catch (e) {
    updateError = errText(e);
  } finally {
    updateChecking = false;
    renderUpdate();
    maybeAutoInstall();
  }
}
/** Nothing would be cut short: no download, no chat, no panel open. */
function quietMoment(): boolean {
  return updateOfferable(models, !$("chat").hidden) && ["store", "network", "modelSheet", "about"].every((id) => $(id).hidden);
}
function maybeAutoInstall() {
  if (!autoInstallNow(update, autoTried, quietMoment(), updating)) return;
  autoTried = update!.version;
  void installUpdate(true);
}
function renderUpdate() {
  const v = update?.version ?? null;
  $("updateBar").hidden = !v || (!updating && (updateLater || !updateOfferable(models, !$("chat").hidden)));
  if (v) $("updateText").textContent = updating ? `Updating to AInode ${v}. It opens again by itself.` : `AInode ${v} is ready.`;
  $<HTMLButtonElement>("updateGo").disabled = updating;
  $("updateLater").hidden = updating;
  $("updateNote").textContent = updating ? `Updating to AInode ${v}. It opens again by itself.` : updateNote(update, updateError, updateChecking, state?.version ?? "");
  $("updateNow").hidden = !v || updating;
  $<HTMLButtonElement>("updateCheck").disabled = updateChecking || updating;
}
/** `auto`: tried by AInode itself (maybeAutoInstall); a failure then leaves the Restart bar, no error note. */
async function installUpdate(auto = false) {
  if (updating || !update?.version) return;
  if (!updateOfferable(models, false)) {
    toast("A model is downloading. Restart to update once it is done.");
    return;
  }
  updating = true;
  renderUpdate();
  try {
    await api.updateInstall(); // the app restarts; on Windows the installer takes over
  } catch (e) {
    updating = false;
    renderUpdate();
    if (auto) console.error("update install failed:", errText(e));
    else toast(errText(e), "error");
    // The engine may have been stopped for the install: start it again.
    await refresh();
    if (state?.autostart && state.engine.state === "stopped") void onCore();
  }
}

async function refreshOnce() {
  try {
    const [s, c] = await Promise.all([api.getState(), api.catalog()]);
    state = s;
    models = c;
    track(c);
  } catch (e) {
    console.error(e);
    return;
  }
  void maybeCommunity();
  watchStops();
  void checkUpdate();
  void lookForNode();
  if (busy) return;
  renderUpdate();
  maybeAutoInstall();
  renderCore(state);
  renderModels(state);
  renderMeter(state);
  if (!$("about").hidden) renderAbout(state);
  maybeStartSearch(state);
}

async function boot() {
  // Linux and Windows: their words for the computer, and a title bar without
  // the space macOS keeps for its window buttons (styles.css).
  document.documentElement.dataset.os = OS;
  // Hidden or minimized, the animations pause (styles.css data-away, logic.ts
  // awayNow); on Linux also while another window has the focus.
  let minimized = false;
  // Outside the app (the browser preview, the dev bridge) there is no window to ask.
  if (!inTauri) win.minimized = false;
  const away = () => {
    document.documentElement.toggleAttribute("data-away", awayNow(document.hidden, minimized, OS, document.hasFocus()));
    win.visible = !document.hidden;
    if (state && api) maybeStartSearch(state);
  };
  for (const ev of ["blur", "focus"]) window.addEventListener(ev, away);
  document.addEventListener("visibilitychange", away);
  away();
  void watchMinimized((m) => { minimized = m; win.minimized = m; away(); }).catch((e) => console.error(e));
  localizeStatic(document.body);
  api = await getApi();
  state = await api.getState();
  applyTheme(state.theme);
  applyFace(state.face ?? "random");
  await refresh();
  $("ver").textContent = `v${state.version}`;
  if (!state.consent) showSpace(false);
  else if (state.engine.state === "stopped" && state.autostart) {
    // Opening the app is the start button. Pressing stop is remembered.
    void onCore();
  }

  $("core").addEventListener("click", () => void onCore());
  for (const id of ["models", "storeList", "mpFiles"]) $(id).addEventListener("click", (e) => void onModelClick(e));
  $("storeBtn").addEventListener("click", () => ($("store").hidden = false));
  wireNetwork({
    api, toast,
    running: () => state?.engine.state === "running",
    models: () => models,
    openStore: () => ($("store").hidden = false),
    host: () => state?.network.active_host,
    onCoverage: (c) => {
      netCoverage = c;
      coverageRun = state?.network.engine_run ?? 0;
      if (state) renderChip(state);
    },
  });
  $("status").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-url]");
    if (b) void api.openUrl(b.dataset.url!).catch((x) => toast(errText(x), "error"));
  });
  $("network").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-url]");
    if (b) void api.openUrl(b.dataset.url!).catch((x) => toast(errText(x), "error"));
  });
  $("storeBack").addEventListener("click", () => ($("store").hidden = true));
  $("modelSheetBack").addEventListener("click", () => ($("modelSheet").hidden = true));
  $("mpThink").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-v]");
    if (b) changeModelPrefs({ think: b.dataset.v === "on" });
  });
  $("mpLength").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-v]");
    if (b) changeModelPrefs({ length: b.dataset.v as ModelPrefs["length"] });
  });
  $("mpTemp").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-v]");
    if (b) changeModelPrefs({ temperature: Number(b.dataset.v) });
  });
  $("choices").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-g]");
    if (b && !b.disabled) pickPreset(Number(b.dataset.g));
  });
  $("customGb").addEventListener("input", onTyped);
  $("customGb").addEventListener("keydown", (e) => { if (e.key === "Enter") void onConsentGo(); });
  $("consentGo").addEventListener("click", () => void onConsentGo());
  $("consentCancel").addEventListener("click", () => ($("consent").hidden = true));
  $("whereChange").addEventListener("click", () => void onWhereChange());
  $("places").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest("button");
    if (b) pickPlace(b as HTMLButtonElement);
  });
  $("spaceChange").addEventListener("click", () => showSpace(true));
  $("themeBtn").addEventListener("click", () => {
    const dark = document.documentElement.dataset.theme === "dark";
    void setTheme(dark ? "light" : "dark");
  });
  $("faceSeg").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-f]");
    if (b) void setFace(b.dataset.f as Face);
  });
  $("themeSeg").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-t]");
    if (b) void setTheme(b.dataset.t as Theme);
  });
  $("aboutBtn").addEventListener("click", () => { if (state) { renderAbout(state); applyKeepChats(state.keep_chats); applyGpu(state.gpu); applyCountMe(state.count_me); applyLogin(state.start_at_login, state.login_error); } void renderOwn(); $("about").hidden = false; nodeAskedAt = 0; void lookForNode(); });
  $("loginSeg").addEventListener("click", async (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-login]");
    if (!b || !state) return;
    const on = b.dataset.login === "on";
    try {
      await api.setPref("start_at_login", on);
      const s = await api.getState();
      state.start_at_login = s.start_at_login;
      state.login_error = s.login_error;
      applyLogin(state.start_at_login, state.login_error);
    } catch (x) {
      toast(errText(x), "error");
    }
  });
  $("countSeg").addEventListener("click", async (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-count]");
    if (!b || !state) return;
    const on = b.dataset.count === "on";
    try {
      await api.setPref("count_me", on);
      state.count_me = on;
      applyCountMe(on);
      void maybeCommunity(true);
    } catch (x) {
      toast(errText(x), "error");
    }
  });
  $("gpuSeg").addEventListener("click", async (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-gpu]");
    if (!b || !state) return;
    const on = b.dataset.gpu === "on";
    try {
      await api.setPref("gpu", on);
      state.gpu = on;
      applyGpu(on);
      toast(on ? "The next chat uses the graphics card." : "The next chat leaves the graphics card free.");
    } catch (x) {
      toast(errText(x), "error");
    }
  });
  $("chatsSeg").addEventListener("click", async (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-k]");
    if (!b || !state) return;
    const on = b.dataset.k === "on";
    try {
      await api.setPref("keep_chats", on);
      state.keep_chats = on;
      applyKeepChats(on);
    } catch (x) {
      toast(errText(x), "error");
    }
  });
  $("ownList").addEventListener("click", async (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-own]");
    if (!b) return;
    if (b.dataset.armed !== "1") {
      b.dataset.armed = "1";
      b.textContent = "Remove?";
      setTimeout(() => { b.dataset.armed = ""; b.textContent = "Remove"; }, 3500);
      return;
    }
    try {
      await api.knowledgeRemove(b.dataset.own!);
      await syncMine(api);
      await renderOwn();
      await refresh();
    } catch (x) {
      toast(errText(x), "error");
    }
  });
  $("aboutBack").addEventListener("click", () => ($("about").hidden = true));
  $("updateGo").addEventListener("click", () => void installUpdate());
  $("wslGo").addEventListener("click", () => void onWslRestart());
  $("updateNow").addEventListener("click", () => void installUpdate());
  $("updateLater").addEventListener("click", () => { updateLater = true; renderUpdate(); });
  $("updateCheck").addEventListener("click", () => void checkUpdate(true));
  $("feedbackRow").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-fb]");
    if (b) void sendFeedback(b.dataset.fb as FeedbackKind, $<HTMLInputElement>("fbModels").checked);
  });
  $("chatFeedback").addEventListener("click", () => void sendFeedback("bug", false));
  $("about").addEventListener("click", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-url]");
    if (b) void api.openUrl(b.dataset.url!).catch((x) => toast(errText(x), "error"));
  });
  $("peerAdd").addEventListener("click", async () => {
    const v = $<HTMLInputElement>("peerInput").value.trim();
    if (!v) return;
    try {
      await api.addPeer(v);
      $<HTMLInputElement>("peerInput").value = "";
      toast(`Now using ${v}`);
      await refresh();
    } catch (e) {
      toast(errText(e), "error");
    }
  });
  // A file dropped outside the chat must not replace the app's page (the
  // webview would open it); only the chat takes files.
  document.addEventListener("dragover", (e) => e.preventDefault());
  document.addEventListener("drop", (e) => e.preventDefault());
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (!$("modelSheet").hidden) $("modelSheet").hidden = true;
    else if (!$("about").hidden) $("about").hidden = true;
    else if (!$("store").hidden) $("store").hidden = true;
    else if (!$("network").hidden) closeNetwork();
    else if (!$("chat").hidden) $("chatBack").click(); // stops a reply in progress too
  });

  // Poll while visible, and once a minute while hidden (logic.ts pollDue);
  // catch up the moment the window is shown again. The dev bridge (a browser
  // tab driving the real engine) polls regardless, since a background tab is
  // "hidden" and would freeze the test.
  const devBridge = realEngine && !inTauri;
  let polledAt = 0;
  setInterval(() => {
    const now = Date.now();
    if (!pollDue(document.hidden && !devBridge, now, polledAt)) return;
    polledAt = now;
    void refresh();
  }, POLL_MS);
  document.addEventListener("visibilitychange", () => { if (!document.hidden) void refresh(); });
}

void boot();
