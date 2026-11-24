// The BTX ModelNet view (0.7.0 as "Network"; the truthful view of 2026-11-23).
// Read-only: what this AInode's host or hosts answered. The models they
// returned, bounties and release campaigns as the engine reports them, and the
// human path upstream's HUMANS.md describes: open a pasted btx:// link, fetch
// that model free, search. Every call goes to the engine on this Mac (Rust
// backend/network.rs), which checks each link first. Nothing here funds, pays
// or signs: "How to fund this" explains and copies, and opens no wallet.
import type { Api, CatalogModel, IndexPeer, LinkModel } from "./api";
import type { Coverage } from "./chip";
import { copyText } from "./clip";
import { fmtBytes, friendlyError } from "./logic";
import { THIS } from "./platform";
import {
  BOUNTY_TERMS_NOT_CARRIED, ID_NOT_IN_FORM, indexStatusText, WHOSE_VIEW, agoText, byLine, coverageOf, coverageText, fundBounty, fundCampaign, isBounty, isCampaign, isOwnCopyOnly,
  looksLikeLink, mergeBounties, mergeCampaigns, modelView, resultsOf, searchFailText, settle, shortId, silentNote,
  type BountyView, type CampaignView, type FundHow, type Kept, type ModelView,
} from "./netview";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const errText = (e: unknown) => (e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e));

type Toast = (s: string, t?: "info" | "error") => void;
interface Deps {
  api: Api; toast: Toast; running: () => boolean; models: () => CatalogModel[]; openStore: () => void;
  /** The host the engine was given, as the app's state has it. */
  host: () => string | undefined;
  /** The counters of the last network answer, for the home screen's chip. */
  onCoverage: (c: Coverage | null) => void;
}
let d: Deps;
let linkTimer = 0;

let keptModels: Kept<ModelView> | null = null;
let keptBounties: Kept<BountyView> | null = null;
let keptCampaigns: Kept<CampaignView> | null = null;
let coverage: Coverage | null = null;
/**
 * The "How to fund this" texts of the cards on screen, by the key on their
 * button ("c0", "b1"). A list's texts are replaced in the same step that
 * redraws its cards, never earlier: while a refresh is still asking the
 * network the old cards stay on screen and their Copy buttons keep working.
 */
const hows = new Map<string, FundHow>();
function forgetHows(prefix: string) {
  for (const k of [...hows.keys()]) if (k.startsWith(prefix)) hows.delete(k);
}
let loading = false;

export function wireNetwork(deps: Deps) {
  d = deps;
  $("networkBtn").addEventListener("click", () => openNetwork());
  $("netChip").addEventListener("click", () => openNetwork());
  $("networkBack").addEventListener("click", () => closeNetwork());
  $("networkRefresh").addEventListener("click", () => void loadLists());
  $("netGo").addEventListener("click", () => void go());
  $("netInput").addEventListener("keydown", (e) => { if (e.key === "Enter") void go(); });
  $("netIndexAdd").addEventListener("click", () => void addIndexPeer());
  $("netIndexInput").addEventListener("keydown", (e) => { if (e.key === "Enter") void addIndexPeer(); });
  $("network").addEventListener("click", (e) => void onClick(e));
}

export function openNetwork() {
  $("network").hidden = false;
  void renderIndexPeers();
  void loadLists();
  void renderLinks();
  clearInterval(linkTimer);
  linkTimer = window.setInterval(() => { if (!$("network").hidden) void renderLinks(); }, 2000);
}

export function closeNetwork() {
  $("network").hidden = true;
  clearInterval(linkTimer);
}

// ── The hosts the engine asks when it searches (index peers) ──────────────
// The list lives in the backend's settings and is given to the engine after
// every start (backend/network.rs). What a user types is checked there; here
// it is only passed on. Every name shown came from the backend and is escaped.
let indexPeers: IndexPeer[] = [];

async function renderIndexPeers(list?: IndexPeer[]) {
  if (list) indexPeers = list;
  else {
    try { indexPeers = await d.api.indexPeers(); } catch (e) { $("netIndexList").innerHTML = `<p class="net-empty bad">${esc(errText(e))}</p>`; return; }
  }
  // The model host is asked at every search whatever the list holds: shown
  // first, so every host a search goes to can be seen here. It is changed
  // under Status, not here.
  const model = d.host();
  drawnHost = model;
  const modelRow = model
    ? `<div class="net-index-row net-index-model">
        <span class="net-index-host">${esc(model)}</span>
        <span class="net-index-tag" title="The host AInode fetches models from. Change it under Status.">your model host</span>
      </div>`
    : "";
  // After a switch under Status the model host may be in the list as well:
  // shown once, as the model host (the engine asks it once too).
  const rows = indexPeers.filter((p) => p.endpoint !== model).map((p) => `<div class="net-index-row">
        <span class="net-index-host">${esc(p.endpoint)}</span>
        ${p.builtin ? `<span class="net-index-tag" title="One of AInode's own hosts">AInode's host</span>` : ""}
        <button class="tool danger" data-net="index-remove" data-host="${esc(p.endpoint)}">Remove</button>
      </div>`).join("");
  $("netIndexList").innerHTML = modelRow + (rows || `<p class="net-empty">No other host. Add one below to have it asked too.</p>`);
  renderIndexStatus();
}

function renderIndexStatus() {
  const model = d.host();
  $("netIndexStatus").textContent = indexStatusText(indexPeers.filter((p) => p.endpoint !== model).length, coverage, Date.now());
}

/** An add is on its way: Enter twice gave a success and then "in the list already". */
let adding = false;

async function addIndexPeer() {
  const input = $<HTMLInputElement>("netIndexInput");
  const v = input.value.trim();
  if (!v || adding) return;
  adding = true;
  $<HTMLButtonElement>("netIndexAdd").disabled = true;
  try {
    await renderIndexPeers(await d.api.indexPeerAdd(v));
    input.value = "";
    d.toast(d.running() ? "Added. It is asked from the next search on." : "Added. It is asked once the engine runs.");
  } catch (e) {
    d.toast(errText(e), "error");
    // Show what is true now: the backend changed nothing.
    await renderIndexPeers();
  } finally {
    adding = false;
    $<HTMLButtonElement>("netIndexAdd").disabled = false;
  }
}

/**
 * The model host as the list was last drawn with. A failover or "Use a host"
 * changes it while the view may be open: then the list is drawn again
 * (networkStateChanged, called on every state update).
 */
let drawnHost: string | undefined;

export function networkStateChanged() {
  if (!d || $("network").hidden) return;
  if (d.host() !== drawnHost) void renderIndexPeers();
}

/** The engine stopped: what it answered before describes nothing now. */
export function forgetNetwork() {
  keptModels = keptBounties = keptCampaigns = null;
  coverage = null;
}

function notRunning(): boolean {
  if (d.running()) return false;
  forgetNetwork();
  const msg = `<p class="net-empty">Start AInode first: these come from the engine on ${THIS}.</p>`;
  for (const id of ["netModels", "netBounties", "netCampaigns"]) $(id).innerHTML = msg;
  renderCover("");
  return true;
}

/** The line under the title: whom the engine asked, who answered, when, and whose view this is. */
function renderCover(note: string) {
  const host = d.host();
  const asked = coverageText(coverage);
  const parts = [
    asked ? `${asked} Checked ${agoText(coverage!.atMs, Date.now())}.` : "",
    note,
  ].filter(Boolean).join(" ");
  $("netCover").innerHTML = `
    ${parts ? `<p class="net-cover-line${coverage && coverage.asked > 0 && coverage.answered === 0 ? " warn" : ""}">${esc(parts)}</p>` : ""}
    <p class="net-fine">${esc(WHOSE_VIEW)}${host ? ` Its host: ${esc(host)}.` : ""}</p>`;
}

const staleNote = (s: string) => (s ? `<p class="net-empty stale-note">${esc(s)}</p>` : "");

async function loadLists() {
  if (notRunning() || loading) return;
  loading = true;
  try {
    await loadListsNow();
  } finally {
    loading = false;
  }
}

async function loadListsNow() {
  const waiting = `<p class="net-empty">Asking the network…</p>`;
  if (!keptModels) $("netModels").innerHTML = waiting;
  if (!keptBounties) $("netBounties").innerHTML = waiting;
  if (!keptCampaigns) $("netCampaigns").innerHTML = waiting;
  renderCover("Asking the network…");
  let cov: Coverage | null = null;
  const openButton = (x: ModelView) => `<button class="tool" data-net="open" data-uri="${esc(x.uri)}">Open</button>`;
  const drawModels = (m: { items: ModelView[]; stale: string }, empty: string) => {
    $("netModels").innerHTML = m.items.length ? staleNote(m.stale) + m.items.map((x) => modelCard(x, openButton(x), !!m.stale)).join("") : empty;
  };
  const drawCampaigns = (c: { items: CampaignView[]; stale: string }, empty: string, note = "") => {
    forgetHows("c");
    $("netCampaigns").innerHTML = c.items.length ? staleNote(c.stale || note) + c.items.map((x, i) => campaignCard(x, `c${i}`, !!c.stale)).join("") : staleNote(note) + empty;
  };
  const drawBounties = (b: { items: BountyView[]; stale: string }, empty: string) => {
    forgetHows("b");
    $("netBounties").innerHTML = b.items.length ? staleNote(b.stale) + b.items.map((x, i) => bountyCard(x, `b${i}`, !!b.stale)).join("") : empty;
  };
  // One after the other: the engine answers one call at a time.
  try {
    // One network request brings the hosts' models and campaign records; the
    // campaign list is read after it (a campaign some other helper published is
    // in that list only once its signed record arrived).
    const reply = await d.api.networkCampaigns();
    const now = Date.now();
    const refresh = (reply as { refresh?: unknown }).refresh;
    // The search can fail while the campaign list still answers: then there is
    // no model list to show, and that is said, not passed off as an empty answer.
    const searched = refresh != null;
    const why = typeof reply.refresh_error === "string" && reply.refresh_error ? reply.refresh_error : "the engine gave no reason";
    cov = coverageOf(refresh, now);
    const answered = !!cov && cov.answered > 0;
    const quiet = silentNote(cov);
    const found = resultsOf(refresh).filter((r) => !isBounty(r) && !isOwnCopyOnly(r));
    const models = searched ? found.filter((r) => !isCampaign(r)).map(modelView).filter((m) => m.uri) : null;
    const m = settle(models, keptModels, now, answered, quiet);
    keptModels = m.kept;
    drawModels(m, searched
      ? `<p class="net-empty">No model came back this time. The store lists the models AInode offers.</p>`
      : `<p class="net-empty bad">${esc(searchFailText(why))}</p>`);
    const c = settle(mergeCampaigns([...resultsOf(reply), ...found.filter(isCampaign)]), keptCampaigns, now, answered, quiet);
    keptCampaigns = c.kept;
    drawCampaigns(c, `<p class="net-empty">None found right now. A release campaign raises BTX to have a finished model published openly.</p>`,
      searched ? "" : "The network search did not answer this time, so campaigns of other publishers may be missing here.");
    // The counters are shown at once: the bounty search that follows can take
    // its time when a host does not answer.
    if (cov) {
      coverage = cov;
      d.onCoverage(cov);
      renderCover("");
      renderIndexStatus(); // the line under the host list, at once too
    }
  } catch (e) {
    const now = Date.now();
    const bad = `<p class="net-empty bad">${esc(errText(e))}</p>`;
    drawModels(settle<ModelView>(null, keptModels, now), bad);
    drawCampaigns(settle<CampaignView>(null, keptCampaigns, now), bad);
  }
  try {
    const reply = await d.api.networkBounties();
    const now = Date.now();
    const bcov = coverageOf(reply, now);
    if (!cov) {
      cov = bcov;
      if (cov) coverage = cov;
    }
    const b = settle(mergeBounties(resultsOf(reply)), keptBounties, now, !!bcov && bcov.answered > 0, silentNote(bcov));
    keptBounties = b.kept;
    drawBounties(b, `<p class="net-empty">None found on the hosts AInode can reach right now. A bounty is a prize in BTX for whoever builds a model that passes a published test.</p>`);
  } catch (e) {
    drawBounties(settle<BountyView>(null, keptBounties, Date.now()), `<p class="net-empty bad">${esc(errText(e))}</p>`);
  }
  d.onCoverage(cov);
  renderCover(cov ? "" : "The engine gave no answer to count this time.");
  renderIndexStatus();
}

function howBlock(key: string, how: FundHow): string {
  hows.set(key, how);
  return `<div class="net-how" id="how-${key}" hidden>
      ${how.lines.map((l) => `<p>${esc(l)}</p>`).join("")}
      ${how.copies.length ? `<div class="btn-row">${how.copies.map((c, i) => `<button class="tool" data-net="copy" data-how="${key}" data-i="${i}">${esc(c.label)}</button>`).join("")}</div>` : ""}
    </div>`;
}

function bountyCard(x: BountyView, key: string, stale: boolean): string {
  return `<article class="net-card${stale ? " stale" : ""}">
      <div class="net-title">${esc(x.title)}${x.prize ? `<span class="net-prize">Prize ${esc(x.prize)}</span>` : ""}</div>
      ${x.text ? `<p>${esc(x.text)}</p>` : ""}
      ${x.trust ? `<p class="net-fine">${esc(x.trust)}</p>` : ""}
      <p class="net-fact">${esc(x.raised)}${x.termsMissing ? ` · ${esc(BOUNTY_TERMS_NOT_CARRIED)}` : ""}</p>
      <p class="net-id">${x.signer ? `Signer named: ${esc(shortId(x.signer))} (not checked by AInode)` : "No signer named"}${x.id ? ` · id ${esc(shortId(x.id))}` : x.idBad ? ` · ${ID_NOT_IN_FORM}` : ""}</p>
      <div class="btn-row"><button class="tool" data-net="how" data-how="${key}">How to fund this</button></div>
      ${howBlock(key, fundBounty(x))}
    </article>`;
}

function campaignCard(x: CampaignView, key: string, stale: boolean): string {
  return `<article class="net-card${stale ? " stale" : ""}">
      <div class="net-title">${esc(x.name || "A model")}${x.goal ? `<span class="net-prize">Goal ${esc(x.goal)}</span>` : ""}</div>
      ${x.state ? `<p class="net-fact">${esc(x.state)}${x.size ? ` · ${esc(x.size)}` : ""}</p>` : ""}
      ${x.text ? `<p>${esc(x.text)}</p>` : ""}
      <p class="net-fact">${esc(x.raised)}${x.refundHeight ? ` · Funders can take their part back after block ${x.refundHeight.toLocaleString("en-US")} if it is not released.` : ""}</p>
      <p class="net-id">${x.signed ? (x.signerId ? `Signed by key ${esc(shortId(x.signerId, 8))}` : "Signed record") : "Not signed"}${x.id ? ` · release ${esc(shortId(x.id))}` : x.idBad ? ` · ${ID_NOT_IN_FORM}` : ""}</p>
      <div class="btn-row">${x.uri ? `<button class="tool" data-net="open" data-uri="${esc(x.uri)}">Open</button>` : ""}<button class="tool" data-net="how" data-how="${key}">How to fund this</button></div>
      ${howBlock(key, fundCampaign(x))}
    </article>`;
}

async function go() {
  const v = $<HTMLInputElement>("netInput").value.trim();
  if (!v) return;
  if (!d.running()) { d.toast("Start AInode first", "error"); return; }
  const out = $("netResult");
  out.innerHTML = `<p class="net-empty">${looksLikeLink(v) ? "Opening the link…" : "Searching the hosts AInode can reach…"}</p>`;
  try {
    if (looksLikeLink(v)) await showLink(v);
    else {
      // Models only: a bounty's record also answers a model search, as a model with a link.
      const r = resultsOf(await d.api.networkSearch(v)).filter((x) => !isBounty(x) && !isOwnCopyOnly(x)).map(modelView).filter((m) => m.uri);
      out.innerHTML = r.length
        ? r.map((m) => modelCard(m, `<button class="tool" data-net="open" data-uri="${esc(m.uri)}">Open</button>`)).join("")
        : `<p class="net-empty">Nothing found for “${esc(v)}” on the hosts AInode can reach. Upstream's search covers only what hosts publish; the store lists AInode's own models.</p>`;
    }
  } catch (e) {
    out.innerHTML = `<p class="net-empty bad">${esc(errText(e))}</p>`;
  }
}

/** `trusted`: the name and publisher come from AInode's own store, not from the network. */
function modelCard(m: ModelView, actions: string, stale = false, trusted = false): string {
  const by = byLine(m, trusted);
  return `<article class="net-card${stale ? " stale" : ""}">
      <div class="net-title">${esc(m.name || "A model")}${m.size ? `<span class="net-size">${esc(m.size)}</span>` : ""}</div>
      ${by ? `<p class="net-fine">${esc(by)}</p>` : ""}
      ${m.text ? `<p>${esc(m.text)}</p>` : ""}
      <p class="net-id">${esc(m.uri)}</p>
      <div class="btn-row">${actions}</div>
    </article>`;
}

async function showLink(uri: string) {
  const c = await d.api.linkOpen(uri);
  const own = c.store_id ? d.models().find((x) => x.id === c.store_id) : undefined;
  const m = modelView({ ...c.card, ...(c.show ?? {}), uri: c.uri, ...(own ? { name: own.name, publisher: own.publisher, size_bytes: own.size_bytes } : {}) });
  const actions = own
    ? `<button class="tool" data-net="store">In AInode's store: ${esc(own.name)}</button>`
    : `<button class="tool" data-net="get" data-uri="${esc(c.uri)}" data-title="${esc(m.name)}">Get free</button>`;
  const note = own
    ? ""
    : `<p class="net-fine">Not in AInode's store. Its pieces are checked against this link, but nobody checked what the model is or does. AInode keeps its files for you to open in another app; it chats only with the models in its store.${m.size ? "" : " Its size is not known until the download starts."}</p>`;
  $("netResult").innerHTML = modelCard(m, actions, false, !!own) + note;
}

async function renderLinks() {
  let list: LinkModel[] = [];
  try { list = await d.api.links(); } catch { return; }
  $("netLinksWrap").hidden = !list.length;
  $("netLinks").innerHTML = list.map((l) => {
    const status = l.local === "downloading"
      ? (l.waiting ? "Host not answering. Still trying; verified pieces are kept." : `${Math.floor(l.progress * 100)}% verified`)
      : l.local === "ready" ? `On ${THIS}${l.bytes ? ` · ${fmtBytes(l.bytes)}` : ""}` : l.error ? friendlyError(l.error) : l.progress > 0 ? `Stopped at ${Math.floor(l.progress * 100)}%` : "Not downloaded";
    const acts = [
      l.local === "ready" ? `<button class="tool" data-net="reveal" data-uri="${esc(l.uri)}">Show files</button>` : "",
      l.local === "none" ? `<button class="tool" data-net="get" data-uri="${esc(l.uri)}" data-title="${esc(l.title)}">${l.progress > 0 ? "Resume" : "Get free"}</button>` : "",
      `<button class="tool danger" data-net="remove" data-uri="${esc(l.uri)}">Remove</button>`,
    ].join("");
    return `<article class="net-card">
        <div class="net-title">${esc(l.title || "A model from a link")}</div>
        <p class="net-fine${l.error && l.local === "none" ? " bad" : ""}">${esc(status)}</p>
        <p class="net-id">${esc(l.uri)}</p>
        <div class="btn-row">${acts}</div>
      </article>`;
  }).join("");
}

async function onClick(e: MouseEvent) {
  const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-net]");
  if (!b) return;
  const uri = b.dataset.uri ?? "";
  try {
    switch (b.dataset.net) {
      case "open":
        $<HTMLInputElement>("netInput").value = uri;
        $("netResult").innerHTML = `<p class="net-empty">Opening the link…</p>`;
        await showLink(uri);
        $("netResult").scrollIntoView({ block: "nearest" });
        break;
      case "index-remove": {
        const host = b.dataset.host ?? "";
        if (b.dataset.armed !== "1") {
          b.dataset.armed = "1";
          b.textContent = "Remove it?";
          setTimeout(() => { b.dataset.armed = ""; b.textContent = "Remove"; }, 3500);
          return;
        }
        try {
          await renderIndexPeers(await d.api.indexPeerRemove(host));
        } catch (e) {
          await renderIndexPeers();
          throw e;
        }
        break;
      }
      case "how": {
        const el = document.getElementById(`how-${b.dataset.how}`);
        if (el) {
          el.hidden = !el.hidden;
          b.setAttribute("aria-expanded", String(!el.hidden));
          if (!el.hidden) el.scrollIntoView({ block: "nearest" });
        }
        break;
      }
      case "copy": {
        const c = hows.get(b.dataset.how ?? "")?.copies[Number(b.dataset.i)];
        if (c) {
          await copyText(c.text);
          d.toast("Copied");
        }
        break;
      }
      case "store":
        closeNetwork();
        d.openStore();
        break;
      case "get":
        b.disabled = true;
        await d.api.linkGet(uri, b.dataset.title ?? "");
        d.toast("Fetching it free. Every piece is checked against the link.");
        await renderLinks();
        break;
      case "reveal":
        await d.api.linkReveal(uri);
        break;
      case "remove":
        if (b.dataset.armed !== "1") {
          b.dataset.armed = "1";
          b.textContent = `Remove from ${THIS}?`;
          setTimeout(() => { b.dataset.armed = ""; b.textContent = "Remove"; }, 3500);
          return;
        }
        await d.api.linkRemove(uri);
        await renderLinks();
        break;
    }
  } catch (x) {
    d.toast(errText(x), "error");
    b.disabled = false;
  }
}
