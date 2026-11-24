// The UI's only door to the backend. Shapes mirror app/ENGINE-CONTRACT.md.
//
// Outside Tauri (a plain browser on `npm run dev`) the same interface is served
// by an in-memory mock, so the interface can be designed, screenshot and tested
// without the engine. The mock is never bundled into a Tauri build's code path:
// `inTauri` decides once at load.

import pkg from "../package.json";

export type Theme = "system" | "dark" | "light";
export type EngineState = "stopped" | "starting" | "running" | "error";
export type LocalState = "none" | "downloading" | "ready";

export interface AppState {
  version: string;
  consent: null | { total_bytes: number; engine_bytes: number; accepted_at: number };
  disk: { free_bytes: number; total_bytes: number };
  /** Where AInode's data lives: the default, or the folder chosen at the first start. */
  data_dir: string;
  /** bundled: what the helper in the engine folder is, from its MANIFEST.json (Rust engine.rs `bundled`). */
  engine: { state: EngineState; error?: string; openssl?: string; pq1_ready?: boolean; bundled?: { version: string; built_by_ainode: boolean } };
  /** peers_configured is 0 or 1: the engine is given one host at a time (active_host). */
  /** hosts_known: hosts.json plus the user's. switched: "from -> to" after an automatic switch (0.7.0). */
  network: { peers_configured: number; peers_connected: number; active_host?: string; hosts_known?: number; switched?: string;
    /** Which engine run this is (0 when the engine is not running): the start-up search runs once per value. */
    engine_run?: number };
  /** own_bytes: the text of the user's own files (Rust own.rs). total includes it. */
  usage: { engine_used_bytes: number; runnable_bytes: number; own_bytes: number; total_bytes: number };
  /** Sizes the user may choose now, in whole GB (Rust: state::consent_limits). max < min: disk full. */
  space: { min_gb: number; max_gb: number };
  /** This Mac's memory, for CatalogModel.ram_gb. 0 if unknown. */
  ram_bytes: number;
  theme: Theme;
  /** Opening the app starts the engine, unless the user pressed stop. */
  autostart: boolean;
  /** The chat's Think switch. */
  think: boolean;
  /** The loading scene's face: one of the three at random, or pinned. */
  face: Face;
  /** Per-model chat settings by catalog id; a model without one uses defaultPrefs. */
  models: Record<string, ModelPrefs>;
  /** Saved conversations are on (off unless the user turns them on). */
  keep_chats: boolean;
  /** Chat on the graphics card when there is one; off: the processor only, the card stays free. */
  gpu: boolean;
  /** Be counted among the AInodes online (easybtx.com); the number shows either way. */
  count_me: boolean;
  /** Start with the computer, minimized (Rust login.rs). On by default; the entry is written after consent. */
  start_at_login: boolean;
  /** Why the start entry could not be written, if it could not. */
  login_error?: string | null;
  /** This install's random id in that count; null before consent. */
  community_id: string | null;
  /** Windows: what AInode is doing about WSL (Rust backend/wslheal.rs); null elsewhere. */
  wsl?: WslState | null;
  /** Rust's own timers (backend/keeper.rs): the engine's self-heal and the check-in. */
  keeper?: KeeperState;
}

/**
 * `restart_pending`: the engine stopped on its own and Rust starts it again
 * soon (20 s, 1 min, 3 min). `checked_in_at` (unix s) and `checkin_ok`: the
 * last check-in with easybtx.com, every 5 minutes, after consent, unless
 * "count me" is off.
 */
export interface KeeperState { restart_pending: boolean; checked_in_at: number | null; checkin_ok: boolean | null }

/**
 * Windows only. `restarting`: AInode restarts WSL now (it was stuck); `easynode`:
 * AInode waits while easyNode restarts it; `stuck_others`: WSL is stuck and these
 * other distributions run, so only the user's click restarts it; `node_here`: a
 * BTX node answers on this PC (it stops too); `again`: after AInode's last
 * restart of WSL, the other distributions it stopped and started again.
 */
export interface WslState { restarting: boolean; easynode: boolean; stuck_others?: string[]; node_here: boolean; again?: WslAgain }
/** `at`: unix seconds, when starting them again was over; `back` run again, `not_back` did not start. */
export interface WslAgain { at: number; back: string[]; not_back: string[] }

/** How many AInodes are open now (self-reported, the last 15 minutes). */
export interface Community {
  online: number;
  platforms: { macos: number; windows: number; linux: number };
  enginesRunning: number;
  withModels: number;
  seenToday: number;
  at: number;
  /** The BTX nodes easybtx.com's census finds live (null when it cannot read it). */
  btxNodes?: BtxNodes | null;
}

/**
 * The BTX network as easybtx.com's node census counts it, the /nodes page's
 * headline numbers: every node found live, easyNodes among them, and their
 * countries. These nodes keep the chain; they are not model hosts.
 */
export interface BtxNodes {
  live: number;
  countries: number;
}

export const COMMUNITY_BASE = "https://easybtx.com/api";

/**
 * Read how many AInodes are open (the public count). The check-in itself is
 * Rust's (backend/keeper.rs, every 5 minutes whether or not the window's page
 * runs). Null when easybtx.com cannot be reached: the app then simply shows
 * no number.
 */
export async function fetchCommunity(f: typeof fetch = fetch): Promise<Community | null> {
  try {
    const r = await f(`${COMMUNITY_BASE}/ainode-online`);
    if (!r.ok) return null;
    const d = await r.json();
    return typeof d?.online === "number" && d.platforms ? (d as Community) : null;
  } catch {
    return null;
  }
}

/** What a check for updates found (Rust update.rs UpdateOut). */
export interface UpdateInfo {
  /** A newer AInode on offer, if any. */
  version: string | null;
  notes: string | null;
  /** Why the feed was refused (its files not bound to its version), if it was. */
  refused: string | null;
  /** This computer installs it without a click (Rust update.rs installs_by_itself: not a .deb). */
  by_itself: boolean;
  /** The version AInode was updated from, after an update. */
  updated_from: string | null;
}

/** The system, for Send feedback (Rust os.rs SystemInfo). */
export interface SystemInfo { os: string; machine: string; chip: string; ram_bytes: number }

/**
 * A BTX node on this computer (Rust neighbour.rs): its height, read from
 * easyNode's local witness, and (Windows) when easyNode last said it was
 * restarting WSL, unix seconds, if within 15 minutes.
 */
export interface Neighbour { node_height: number | null; wsl_restart_at: number | null }

/** A kept conversation (Rust chats.rs). */
export interface SavedChat { id: string; model_id: string; title: string; created?: number; updated?: number; messages: { role: "user" | "assistant"; content: string }[] }
export interface ChatMeta { id: string; model_id: string; title: string; updated: number; count: number }

export type Length = "short" | "normal" | "long";
/** One model's chat settings (settings button on its card). Rust: state::ModelPrefs. */
export interface ModelPrefs {
  think: boolean;
  length: Length;
  /** 0 to 1.5; 0.6 by default. */
  temperature: number;
}

export type Pref = "autostart" | "think" | "keep_chats" | "gpu" | "count_me" | "start_at_login";
export type Face = "random" | "swarm" | "glyphs" | "chip";

export interface CatalogModel {
  id: string;
  name: string;
  publisher: string;
  licence: string;
  licence_url: string;
  source_url: string;
  /** The smallest Mac memory it runs well on, in GB. 0: no limit. */
  ram_gb: number;
  /** False: no longer offered, listed only so its copy can be removed. */
  offered: boolean;
  summary: string;
  /** What it is good at, for the store. */
  good_at: string;
  uri: string;
  size_bytes: number;
  local: LocalState;
  progress: number;
  runnable: boolean;
  /** Set when the last download failed for a reason other than a cancel. */
  error?: string;
  /** Set while a download is stuck retrying: no piece for 20 s and a transport error. */
  waiting?: string;
}

/** One of the user's own files, as Status lists it (Rust own::OwnFile). */
export interface OwnFile { id: string; name: string; kind: "pdf" | "md" | "txt"; added: number; chars: number; count: number; bytes: number }
/** One passage: "file > section" or "file, page N", and its text. */
export interface Passage { t: string; x: string }

/** A model fetched from a pasted btx:// link (Rust backend/network.rs). */
export interface LinkModel { uri: string; title: string; local: LocalState; progress: number; bytes: number; error?: string; waiting?: string }
/** Opening a link: the share card and what the helper knows (both upstream's JSON, read leniently). */
export interface LinkCard { uri: string; store_id?: string; card: Record<string, unknown>; show: Record<string, unknown> | null }
/** The start-up search's answer: the engine run, when it answered (ms), and the engine's reply. */
export interface StartSearch { run: number; at_ms: number; answer: Results }
/** One host the engine asks when it searches; `builtin`: one of AInode's own hosts. */
export interface IndexPeer { endpoint: string; builtin: boolean }
/** Upstream replies (searchbounties, getfundablemodels, searchmodels): each has a `results` list. */
export interface Results { results?: Record<string, unknown>[]; [k: string]: unknown }

/** The local llama-server: loopback port plus a per-session key it requires. */
export interface ChatPort {
  port: number;
  key: string;
}

/** A drive or volume for the first-run choice (Rust backend::PlaceOut). */
export interface Place {
  root: string;
  free_bytes: number;
  total_bytes: number;
  /** Where AInode's data would go on it. */
  data_dir: string;
  min_gb: number;
  max_gb: number;
  /** The one the current data dir is on. */
  current: boolean;
}

export interface Api {
  getState(): Promise<AppState>;
  /** dataDir only at the first start; `relaunch` means the app restarts to read its paths again. */
  acceptConsent(totalGb: number, dataDir?: string): Promise<{ relaunch: boolean }>;
  places(): Promise<Place[]>;
  relaunch(): Promise<void>;
  /** Read how many AInodes are online (Rust checks in). */
  community(): Promise<Community | null>;
  startEngine(): Promise<void>;
  /** Windows: restart WSL on the user's click; `others` are the distributions the button named. */
  restartWsl(others: string[]): Promise<void>;
  stopEngine(): Promise<void>;
  catalog(): Promise<CatalogModel[]>;
  download(uri: string): Promise<void>;
  cancel(uri: string): Promise<void>;
  remove(uri: string): Promise<void>;
  reveal(uri: string): Promise<void>;
  chatStart(uri: string): Promise<ChatPort>;
  chatStop(): Promise<void>;
  setTheme(t: Theme): Promise<void>;
  setPref(name: Pref, on: boolean): Promise<void>;
  setFace(face: Face): Promise<void>;
  setModelPrefs(id: string, prefs: ModelPrefs): Promise<void>;
  knowledgeList(): Promise<OwnFile[]>;
  knowledgeAdd(name: string, kind: string, passages: Passage[]): Promise<OwnFile>;
  knowledgeRemove(id: string): Promise<void>;
  knowledgePassages(): Promise<{ f: string; t: string; x: string }[]>;
  networkBounties(): Promise<Results>;
  networkCampaigns(): Promise<Results>;
  networkSearch(text: string): Promise<Results>;
  /** The start-up search of this engine run: once per run, then the same answer again (Rust backend/network.rs). */
  networkStartSearch(): Promise<StartSearch>;
  /** The hosts the engine asks when it searches (its index peers). */
  indexPeers(): Promise<IndexPeer[]>;
  indexPeerAdd(hostport: string): Promise<IndexPeer[]>;
  indexPeerRemove(hostport: string): Promise<IndexPeer[]>;
  linkOpen(uri: string): Promise<LinkCard>;
  linkGet(uri: string, title: string): Promise<void>;
  links(): Promise<LinkModel[]>;
  linkReveal(uri: string): Promise<void>;
  linkRemove(uri: string): Promise<void>;
  chatsList(modelId: string): Promise<ChatMeta[]>;
  chatLoad(id: string): Promise<SavedChat>;
  chatSave(chat: SavedChat): Promise<void>;
  chatDelete(id: string): Promise<void>;
  addPeer(hostport: string): Promise<void>;
  openUrl(url: string): Promise<void>;
  /** Look for a newer AInode; never downloads. */
  updateCheck(): Promise<UpdateInfo>;
  /** Download the update the last check found, stop the engine, install it, restart. */
  updateInstall(): Promise<void>;
  /** The OS version, the machine, its chip and memory, for Send feedback. */
  systemInfo(): Promise<SystemInfo>;
  /** A BTX node on this computer, if one answers on loopback. Nothing before consent. */
  neighbour(): Promise<Neighbour>;
}

export const inTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in (window as unknown as Record<string, unknown>);

/**
 * Calls `cb` with whether the window is minimized, now and on every change.
 * WebView2 (Windows) keeps `document.hidden` false while its window is
 * minimized, so the page cannot see it by itself; Tauri can. Outside Tauri it
 * reports nothing.
 */
export async function watchMinimized(cb: (minimized: boolean) => void): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const w = getCurrentWindow();
  const check = () => w.isMinimized().then(cb, () => {});
  // A restore can report its new size before the window stops saying it is
  // minimized: look again a moment later, and whenever the focus moves.
  const soon = () => {
    void check();
    setTimeout(() => void check(), 400);
  };
  await w.onResized(soon);
  await w.onFocusChanged(soon);
  await check();
}

/** Dev only: `?bridge` drives the REAL engine through src-tauri/examples/devbridge.rs. */
const bridge = !inTauri && typeof location !== "undefined" && new URLSearchParams(location.search).has("bridge");

/** A real engine (and a real local model server) is behind this UI. */
export const realEngine = inTauri || bridge;

function bridgeApi(): Api {
  const call = async <T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
    const r = await fetch(`http://127.0.0.1:1455/invoke/${cmd}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(args),
    });
    const j = await r.json();
    if (!r.ok) throw new Error(j.error ?? `bridge ${r.status}`);
    return j as T;
  };
  return {
    getState: () => call<AppState>("get_state"),
    acceptConsent: (totalGb, dataDir) => call<{ relaunch: boolean }>("accept_consent", { totalGb, dataDir }),
    places: () => call<Place[]>("places"),
    relaunch: () => call("relaunch"),
    community: () => fetchCommunity(),
    startEngine: () => call("start_engine"),
    restartWsl: (others) => call("restart_wsl", { others }),
    stopEngine: () => call("stop_engine"),
    catalog: () => call<CatalogModel[]>("catalog"),
    download: (uri) => call("download", { uri }),
    cancel: (uri) => call("cancel", { uri }),
    remove: (uri) => call("remove", { uri }),
    reveal: (uri) => call("reveal", { uri }),
    chatStart: (uri) => call<ChatPort>("chat_start", { uri }),
    chatStop: () => call("chat_stop"),
    setTheme: (t) => call("set_theme", { t }),
    setPref: (name, on) => call("set_pref", { name, on }),
    setFace: (face) => call("set_face", { face }),
    setModelPrefs: (id, prefs) => call("set_model_prefs", { id, prefs }),
    knowledgeList: () => call<OwnFile[]>("knowledge_list"),
    knowledgeAdd: (name, kind, passages) => call<OwnFile>("knowledge_add", { name, kind, passages }),
    knowledgeRemove: (id) => call("knowledge_remove", { id }),
    knowledgePassages: () => call("knowledge_passages"),
    networkBounties: () => call<Results>("network_bounties"),
    networkCampaigns: () => call<Results>("network_campaigns"),
    networkSearch: (text) => call<Results>("network_search", { text }),
    networkStartSearch: () => call<StartSearch>("network_start_search"),
    indexPeers: () => call<IndexPeer[]>("index_peers"),
    indexPeerAdd: (hostport) => call<IndexPeer[]>("index_peer_add", { hostport }),
    indexPeerRemove: (hostport) => call<IndexPeer[]>("index_peer_remove", { hostport }),
    linkOpen: (uri) => call<LinkCard>("link_open", { uri }),
    linkGet: (uri, title) => call("link_get", { uri, title }),
    links: () => call<LinkModel[]>("links"),
    linkReveal: (uri) => call("link_reveal", { uri }),
    linkRemove: (uri) => call("link_remove", { uri }),
    chatsList: (modelId) => call<ChatMeta[]>("chats_list", { modelId }),
    chatLoad: (id) => call<SavedChat>("chat_load", { id }),
    chatSave: (chat) => call("chat_save", { chat }),
    chatDelete: (id) => call("chat_delete", { id }),
    addPeer: (hostport) => call("add_peer", { hostport }),
    openUrl: (url) => call("open_url", { url }),
    // The bridge drives the engine, not the app bundle: nothing to update.
    updateCheck: async () => ({ version: null, notes: null, refused: null, by_itself: false, updated_from: null }),
    updateInstall: async () => { throw new Error("The dev bridge does not update"); },
    systemInfo: async () => ({ os: navigator.platform, machine: "", chip: "", ram_bytes: 0 }),
    neighbour: async () => ({ node_height: null, wsl_restart_at: null }),
  };
}

async function tauriApi(): Promise<Api> {
  const { invoke } = await import("@tauri-apps/api/core");
  return {
    getState: () => invoke<AppState>("get_state"),
    acceptConsent: (totalGb, dataDir) => invoke<{ relaunch: boolean }>("accept_consent", { totalGb, dataDir }),
    places: () => invoke<Place[]>("places"),
    relaunch: () => invoke("relaunch"),
    community: () => fetchCommunity(),
    startEngine: () => invoke("start_engine"),
    restartWsl: (others) => invoke("restart_wsl", { others }),
    stopEngine: () => invoke("stop_engine"),
    catalog: () => invoke<CatalogModel[]>("catalog"),
    download: (uri) => invoke("download", { uri }),
    cancel: (uri) => invoke("cancel", { uri }),
    remove: (uri) => invoke("remove", { uri }),
    reveal: (uri) => invoke("reveal", { uri }),
    chatStart: (uri) => invoke<ChatPort>("chat_start", { uri }),
    chatStop: () => invoke("chat_stop"),
    setTheme: (t) => invoke("set_theme", { t }),
    setPref: (name, on) => invoke("set_pref", { name, on }),
    setFace: (face) => invoke("set_face", { face }),
    setModelPrefs: (id, prefs) => invoke("set_model_prefs", { id, prefs }),
    knowledgeList: () => invoke<OwnFile[]>("knowledge_list"),
    knowledgeAdd: (name, kind, passages) => invoke<OwnFile>("knowledge_add", { name, kind, passages }),
    knowledgeRemove: (id) => invoke("knowledge_remove", { id }),
    knowledgePassages: () => invoke("knowledge_passages"),
    networkBounties: () => invoke<Results>("network_bounties"),
    networkCampaigns: () => invoke<Results>("network_campaigns"),
    networkSearch: (text) => invoke<Results>("network_search", { text }),
    networkStartSearch: () => invoke<StartSearch>("network_start_search"),
    indexPeers: () => invoke<IndexPeer[]>("index_peers"),
    indexPeerAdd: (hostport) => invoke<IndexPeer[]>("index_peer_add", { hostport }),
    indexPeerRemove: (hostport) => invoke<IndexPeer[]>("index_peer_remove", { hostport }),
    linkOpen: (uri) => invoke<LinkCard>("link_open", { uri }),
    linkGet: (uri, title) => invoke("link_get", { uri, title }),
    links: () => invoke<LinkModel[]>("links"),
    linkReveal: (uri) => invoke("link_reveal", { uri }),
    linkRemove: (uri) => invoke("link_remove", { uri }),
    chatsList: (modelId) => invoke<ChatMeta[]>("chats_list", { modelId }),
    chatLoad: (id) => invoke<SavedChat>("chat_load", { id }),
    chatSave: (chat) => invoke("chat_save", { chat }),
    chatDelete: (id) => invoke("chat_delete", { id }),
    addPeer: (hostport) => invoke("add_peer", { hostport }),
    openUrl: (url) => invoke("open_url", { url }),
    updateCheck: () => invoke<UpdateInfo>("update_check"),
    updateInstall: () => invoke("update_install"),
    systemInfo: () => invoke<SystemInfo>("system_info"),
    neighbour: () => invoke<Neighbour>("neighbour"),
  };
}

// ── Mock ────────────────────────────────────────────────────────────────────
// Behaves like the real thing at human speed: start takes a beat, a download
// advances over ~20 s, chat streams. `?mock=fresh|running|empty|error|bounty` picks a
// starting scene for screenshots.

const GiB = 1024 ** 3;

// The shape searchbounties returned on the regtest rehearsal (lab/evidence/2026-10-13/bounty-regtest).
const MOCK_BOUNTY = {
  bounty_id: "a".repeat(96), object_kind: "BOUNTY", title: "BTX Explainer",
  description: "A small open model that answers questions about BTX correctly, judged on 60 questions.",
  target_atoms: "500000000", trust_label: "COUNCIL_CUSTODIAL_AUTHORITY_WITH_INDIVIDUAL_REFUND_PATHS", wallet: false,
};

// The mock's engine: a version no release has, so a mock screen is never
// mistaken for what a build ships.
const MOCK_BUNDLED = { version: "v9.9.9", built_by_ainode: true };

// A bounty some other helper published, as a 0.34.16 engine returns it: twice, a
// result card and the raw record, neither with a prize (lab/evidence/2026-11-23/modelnet-netview/v16).
const MOCK_FOREIGN_BOUNTY = [
  { model_id: "b".repeat(96), uri: "btx://pq" + "q".repeat(83), name: "Small coding model", description: "An open model under 4 GB that passes a published coding test.", result_type: "PUBLIC_MODEL", search: { metadata_verified: true } },
  { model_id: "b".repeat(96), bounty_id: "b".repeat(96), object_kind: "BOUNTY", canonical_name: "Small coding model", display_name: "Small coding model",
    short_description: "An open model under 4 GB that passes a published coding test.", publisher_identity: "0d42d1d21daf89d8" + "0".repeat(80), signed_metadata: true },
];
// A release campaign as getfundablemodels returns it on 0.34.16: the numbers are under `release`.
const MOCK_CAMPAIGN = {
  model_id: "c".repeat(96), uri: "btx://pq0yfnde3hy6jqhanm3zdml3wjzuz30yv5sxgaqm7amrr09psy8umn5t6phn3hjtr7lvv4dfzgxje07pvr2mg", name: "Example Model 12B", description: "A finished model, encrypted. Its publisher releases the key when the goal is reached.",
  size_bytes: 7_400_000_000, result_type: "RELEASE_CAMPAIGN", lifecycle_state: "FUNDING", fundable_now: true, search: { metadata_verified: true },
  release: { release_id: "3d2b0dfa63231c49" + "0".repeat(80), state: "FUNDING", key_hash: "02b42f489691c7fa" + "0".repeat(48), target_atoms: 300000000, pledged_atoms: 0, funded_atoms: 0,
    confirmed_funded_atoms: 0, value_known: false, refund_height: 400000, funding_source: "OBSERVED_NETWORK_STATE", chain_observation: false, actions: ["VIEW_RELEASE", "FUND_RELEASE"] },
};

function mockApi(): Api {
  const scene = new URLSearchParams(location.search).get("mock") ?? "fresh";
  const st: AppState = {
    version: pkg.version,
    consent: scene === "fresh" ? null : { total_bytes: 25e9, engine_bytes: 11 * GiB, accepted_at: 0 },
    disk: { free_bytes: 266 * GiB, total_bytes: 926 * GiB },
    data_dir: "/Users/you/Library/Application Support/AInode",
    engine: scene === "running" || scene === "bounty" ? { state: "running", openssl: "OpenSSL 3.6.3 9 Jun 2026", pq1_ready: true, bundled: MOCK_BUNDLED }
      : { state: scene === "error" ? "error" : "stopped" },
    network: scene === "empty" ? { peers_configured: 0, peers_connected: 0 }
      : { peers_configured: 1, peers_connected: 0, active_host: "203.0.113.7:29447" },
    usage: { engine_used_bytes: 0, runnable_bytes: 0, own_bytes: 0, total_bytes: 0 },
    space: { min_gb: 4, max_gb: 228 },
    ram_bytes: 24 * GiB,
    theme: "system",
    autostart: true,
    think: true,
    face: "random",
    models: {},
    keep_chats: false,
    gpu: true,
    count_me: true,
    start_at_login: true,
    community_id: scene === "fresh" ? null : "0123456789abcdef0123456789abcdef",
    keeper: { restart_pending: false, checked_in_at: null, checkin_ok: null },
  };
  // `?mock=error&wslrestart`: the engine ended with WSL, as when easyNode restarts it.
  if (scene === "error") st.engine.error = new URLSearchParams(location.search).has("wslrestart")
    ? "The engine was stopped by signal 9" : "openssl mldsa44 certificate generation failed";
  // `?mock=error&wslstuck`: WSL stuck with other Linux running (Windows, 0.8.4): the button.
  if (scene === "error" && new URLSearchParams(location.search).has("wslstuck")) {
    st.engine.error = "Windows' Linux (WSL) has lost this PC's drives, as can happen after a graphics driver reset or a Windows update. Only a restart of it helps.";
    st.wsl = { restarting: false, easynode: false, stuck_others: ["Ubuntu", "AInode-debian13"], node_here: true };
  }
  const models: CatalogModel[] = [
    {
      id: "granite-4.2-3b-q4km", name: "Granite 4.2 3B", publisher: "IBM", licence: "Apache-2.0",
      licence_url: "https://www.apache.org/licenses/LICENSE-2.0",
      source_url: "https://huggingface.co/ibm-granite/granite-4.2-3b-GGUF", ram_gb: 4, offered: true,
      summary: "A small reasoning model for chat, writing and code. Runs on any Apple Silicon Mac and most PCs.",
      good_at: "Everyday chat, short writing, simple code",
      uri: "btx://pq4admq4hc7nnvddga9z0flrvg8rhxld390gxdjae3nt8gywty205nn7yhruydsxdkx8k9h87c9uj3z3wcs9h",
      size_bytes: 2244022910, local: scene === "running" ? "ready" : "none", progress: scene === "running" ? 1 : 0,
      runnable: scene === "running",
    },
    {
      id: "gpt-oss-20b-mxfp4", name: "gpt-oss 20B", publisher: "OpenAI", licence: "Apache-2.0",
      licence_url: "https://www.apache.org/licenses/LICENSE-2.0",
      source_url: "https://huggingface.co/ggml-org/gpt-oss-20b-GGUF", ram_gb: 24, offered: true,
      summary: "OpenAI's open model. Of the models tested on a Mac it knew the most and answered fastest.",
      good_at: "Knowledge, reasoning, code, other languages",
      uri: "btx://pqcjx57rargtrp5us2cgvfdttffzxrfnwfwyrd3m63u9n8wtjqpvs2zf326d025uwrya6wy8tx69mxuxfntv9",
      size_bytes: 12109577982, local: "none", progress: 0, runnable: false,
    },
  ];
  const timers = new Map<string, number>();
  let files: (OwnFile & { passages: Passage[] })[] = [];
  let links: LinkModel[] = [];
  const saved = new Map<string, SavedChat>();
  const usage = () => {
    const used = models.reduce((a, m) => a + m.size_bytes * m.progress, 0);
    const run = models.filter((m) => m.runnable).reduce((a, m) => a + m.size_bytes, 0);
    const own = files.reduce((a, f) => a + f.bytes, 0);
    st.usage = { engine_used_bytes: used, runnable_bytes: run, own_bytes: own, total_bytes: used + run + own };
    // Roughly state::consent_limits (the store's whole-GiB half is left out).
    st.space = {
      min_gb: Math.max(4, Math.ceil((used + run) / 1e9)),
      max_gb: Math.floor((st.disk.free_bytes * 0.8 + used + run) / 1e9),
    };
  };
  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
  // Engine runs and the one start-up search per run, as the backend keeps them.
  let runs = st.engine.state === "running" ? 1 : 0;
  let startSearch: StartSearch | null = null;
  let mockIndex: IndexPeer[] = [];
  const hostPort = (s: string) => {
    const t = s.trim();
    if (!/^(\[[0-9a-fA-F:.]+\]|[A-Za-z0-9.-]{1,253}):\d{1,5}$/.test(t)) throw new Error(`${t || "That"} is not a host and port, like host.example:29447`);
    return t.toLowerCase();
  };
  return {
    async getState() {
      usage();
      st.network.engine_run = st.engine.state === "running" ? runs : 0;
      // Like the engine: peers_connected counts open connections, 0 when idle.
      st.network.peers_connected = models.some((m) => m.local === "downloading") ? 8 : 0;
      return structuredClone(st);
    },
    async acceptConsent(totalGb, dataDir) {
      usage();
      if (dataDir && dataDir !== st.data_dir) {
        // The browser preview cannot relaunch: the choice just takes effect.
        st.data_dir = dataDir;
        if (dataDir.startsWith("/Volumes/Data")) st.disk = { free_bytes: 1800 * GiB, total_bytes: 2000 * GiB };
        usage();
      }
      if (totalGb < st.space.min_gb || totalGb > st.space.max_gb) throw new Error(`Choose ${st.space.min_gb} to ${st.space.max_gb} GB`);
      const total = totalGb * 1e9;
      st.consent = { total_bytes: total, engine_bytes: Math.floor(total / 2 / GiB) * GiB, accepted_at: Date.now() / 1000 };
      return { relaunch: false };
    },
    async places() {
      usage();
      const onData = st.data_dir.startsWith("/Volumes/Data");
      return [
        { root: "/", free_bytes: 266 * GiB, total_bytes: 926 * GiB, data_dir: "/Users/you/Library/Application Support/AInode", min_gb: 4, max_gb: 228, current: !onData },
        { root: "/Volumes/Data", free_bytes: 1800 * GiB, total_bytes: 2000 * GiB, data_dir: "/Volumes/Data/AInode", min_gb: 4, max_gb: 1546, current: onData },
      ];
    },
    async relaunch() {},
    async community() {
      // The browser preview: a plausible community, the user's own app in it when counted.
      const me = st.consent && st.count_me ? 1 : 0;
      return { online: 6 + me, platforms: { macos: 3, windows: 2 + me, linux: 1 }, enginesRunning: 5 + me, withModels: 4 + me, seenToday: 19 + me, at: Date.now() / 1000, btxNodes: { live: 46, countries: 15 } };
    },
    async startEngine() {
      if (!st.consent) throw new Error("consent required before the engine may write anything");
      st.engine = { state: "starting" };
      await wait(1200);
      st.engine = { state: "running", openssl: "OpenSSL 3.6.3 9 Jun 2026", pq1_ready: true, bundled: MOCK_BUNDLED };
      runs += 1;
    },
    async restartWsl(others) {
      st.wsl = { restarting: true, easynode: false, node_here: false };
      st.engine = { state: "starting" };
      await wait(2500);
      // `&notback`: the last one named did not start again.
      const notBack = new URLSearchParams(location.search).has("notback") ? others.slice(-1) : [];
      const again = { at: Date.now() / 1000, back: others.filter((d) => !notBack.includes(d)), not_back: notBack };
      st.wsl = { restarting: false, easynode: false, node_here: false, again };
      st.engine = { state: "running", openssl: "OpenSSL 3.6.3 9 Jun 2026", pq1_ready: true, bundled: MOCK_BUNDLED };
    },
    async stopEngine() {
      st.engine = { state: "stopped" };
      st.network.peers_connected = 0;
      for (const [uri, t] of timers) { clearInterval(t); timers.delete(uri); }
      for (const m of models) if (m.local === "downloading") m.local = "none";
    },
    async catalog() { return structuredClone(models); },
    async download(uri) {
      const m = models.find((x) => x.uri === uri);
      if (!m || timers.has(uri)) return;
      if (st.engine.state !== "running") throw new Error("Start AInode first");
      if (st.network.peers_configured === 0) throw new Error("No model host is configured yet");
      m.error = undefined;
      m.local = "downloading";
      timers.set(uri, window.setInterval(() => {
        m.progress = Math.min(1, m.progress + 0.05 + Math.random() * 0.04);
        if (m.progress >= 1) { m.local = "ready"; m.runnable = true; clearInterval(timers.get(uri)); timers.delete(uri); }
      }, 900));
    },
    async cancel(uri) {
      const m = models.find((x) => x.uri === uri);
      clearInterval(timers.get(uri)); timers.delete(uri);
      if (m) m.local = m.progress >= 1 ? "ready" : "none";
    },
    async remove(uri) {
      const m = models.find((x) => x.uri === uri);
      if (m) { m.local = "none"; m.progress = 0; m.runnable = false; }
    },
    async reveal() {},
    // Long enough to see the loading scene (a real start takes five to twenty seconds).
    async chatStart() { await wait(6000); return { port: 0, key: "" }; },
    async chatStop() {},
    async setTheme(t) { st.theme = t; },
    async setPref(name, on) { st[name] = on; },
    async setFace(face) { st.face = face; },
    async setModelPrefs(id, prefs) { st.models[id] = { ...prefs }; },
    async knowledgeList() { return files.map(({ passages: _, ...f }) => f); },
    async knowledgeAdd(name, kind, passages) {
      const chars = passages.reduce((a, p) => a + p.x.length, 0);
      const f = { id: Math.random().toString(16).slice(2, 18).padEnd(16, "0"), name, kind: kind as OwnFile["kind"], added: Date.now() / 1000, chars, count: passages.length, bytes: chars + 200, passages };
      files = [f, ...files.filter((x) => x.name !== name)];
      const { passages: _, ...meta } = f;
      return meta;
    },
    async knowledgeRemove(id) { files = files.filter((f) => f.id !== id); },
    async knowledgePassages() { return files.flatMap((f) => f.passages.map((p) => ({ f: f.name, ...p }))); },
    // The network as the live mainnet helper answered on 2026-10-13: nothing yet.
    // `?mock=bounty` adds what a network with content returns; `&nohost` a host that does not answer.
    async networkBounties() {
      await wait(600);
      const down = new URLSearchParams(location.search).has("nohost");
      return { schema_version: 1, scope: "NETWORK", results: scene === "bounty" && !down ? [MOCK_BOUNTY, ...MOCK_FOREIGN_BOUNTY] : [], responses_received: down ? 0 : 1, timed_out: down ? 1 : 0 };
    },
    async networkCampaigns() {
      await wait(300);
      const down = new URLSearchParams(location.search).has("nohost");
      const found = down ? [] : models.filter((m) => m.offered).map((m) => ({ name: m.name, publisher: { id: "47ae", display_name: m.publisher }, uri: m.uri, size_bytes: m.size_bytes, result_type: "PUBLIC_MODEL", search: { metadata_verified: true } }));
      // `&nosearch`: the models search failed while the campaign list answered.
      if (new URLSearchParams(location.search).has("nosearch")) {
        return { schema_version: 3, results: scene === "bounty" ? [MOCK_CAMPAIGN] : [], sort: "NEARLY_FUNDED", refresh: null, refresh_error: "engine did not answer searchmodels in time" };
      }
      return {
        schema_version: 3, results: scene === "bounty" && !down ? [MOCK_CAMPAIGN] : [], sort: "NEARLY_FUNDED",
        refresh: { schema_version: 3, scope: "NETWORK", results: found, coverage: { connected_peers_queried: down ? 0 : 1, index_peers_queried: 0, responses_received: down ? 0 : 1, timed_out: down ? 1 : 0, complete: false, global_complete: false } },
      };
    },
    async networkStartSearch() {
      if (st.engine.state !== "running") throw new Error("The engine is not running");
      if (startSearch?.run === runs) return startSearch;
      await wait(400);
      const down = new URLSearchParams(location.search).has("nohost");
      startSearch = { run: runs, at_ms: Date.now(), answer: { results: [], coverage: { connected_peers_queried: 1, index_peers_queried: mockIndex.length, responses_received: down ? 0 : 1 + mockIndex.length, timed_out: down ? 1 + mockIndex.length : 0 } } };
      return startSearch;
    },
    async indexPeers() { return structuredClone(mockIndex); },
    async indexPeerAdd(hostport) {
      const p = hostPort(hostport);
      if (mockIndex.some((x) => x.endpoint === p)) throw new Error(`${p} is in the list already`);
      mockIndex = [...mockIndex, { endpoint: p, builtin: false }];
      return structuredClone(mockIndex);
    },
    async indexPeerRemove(hostport) {
      const p = hostPort(hostport);
      mockIndex = mockIndex.filter((x) => x.endpoint !== p);
      return structuredClone(mockIndex);
    },
    async networkSearch(text) {
      await wait(500);
      return { schema_version: 3, text, scope: "NETWORK", results: models.filter((m) => m.name.toLowerCase().includes(text.toLowerCase())).map((m) => ({ name: m.name, publisher: m.publisher, uri: m.uri, size_bytes: m.size_bytes, family: "granite" })) };
    },
    async linkOpen(uri) {
      const m = models.find((x) => x.uri === uri.trim());
      if (!/^btx:\/\/[qpzry9x8gf2tvdw0s3jn54khce6mua7l]{85}\/?$/.test(uri.trim())) throw new Error("That is not a whole BTX model link: it has the wrong length");
      return { uri: uri.trim(), store_id: m?.id, card: { share: { uri, size_bytes: m?.size_bytes ?? 0, signed: false } }, show: m ? { name: m.name } : null };
    },
    async linkGet(uri, title) { links = [...links.filter((l) => l.uri !== uri), { uri, title, local: "downloading", progress: 0.3, bytes: 0 }]; },
    async links() { return structuredClone(links); },
    async linkReveal() {},
    async linkRemove(uri) { links = links.filter((l) => l.uri !== uri); },
    async chatsList(modelId) {
      return [...saved.values()].filter((c) => !modelId || c.model_id === modelId)
        .map((c) => ({ id: c.id, model_id: c.model_id, title: c.title, updated: c.updated ?? 0, count: c.messages.length }))
        .sort((a, b) => b.updated - a.updated);
    },
    async chatLoad(id) { const c = saved.get(id); if (!c) throw new Error("That chat is gone"); return structuredClone(c); },
    async chatSave(chat) { if (st.keep_chats && chat.messages.length) saved.set(chat.id, { ...structuredClone(chat), updated: Date.now() / 1000 }); },
    async chatDelete(id) { saved.delete(id); },
    async addPeer(hostport) { st.network.peers_configured = 1; st.network.active_host = hostport; },
    async openUrl(url) { window.open(url, "_blank", "noopener"); },
    // `?update`: a newer version on offer; `?update=refused`: a feed the app refuses.
    async updateCheck() {
      await wait(600);
      const u = new URLSearchParams(location.search).get("update");
      // `?update=ask`: one this computer cannot install by itself (a .deb); `?updated`: just updated.
      const from = new URLSearchParams(location.search).has("updated") ? "0.8.0" : null;
      if (u === null) return { version: null, notes: null, refused: null, by_itself: true, updated_from: from };
      if (u === "refused") return { version: null, notes: null, refused: "AInode 0.8.1 was refused: its darwin-aarch64 file is signed as AInode.app.tar.gz, not AInode_0.8.1_aarch64.app.tar.gz", by_itself: true, updated_from: from };
      return { version: "0.8.1", notes: "Fixes and small things.", refused: null, by_itself: u !== "ask", updated_from: from };
    },
    // After the "install" the page opens as the updated app would: `?updated`.
    async updateInstall() { await wait(1500); location.search = "?updated"; },
    async systemInfo() { return { os: "macOS 27.0.1", machine: "Mac17,2", chip: "Apple M5", ram_bytes: 24 * GiB }; },
    // `?node`: a BTX node on this computer; `?wslrestart`: easyNode just restarted WSL.
    async neighbour() {
      const q = new URLSearchParams(location.search);
      return { node_height: q.has("node") ? 249862 : null, wsl_restart_at: q.has("wslrestart") ? Math.floor(Date.now() / 1000) - 20 : null };
    },
  };
}

let api: Api | null = null;
export async function getApi(): Promise<Api> {
  if (!api) api = inTauri ? await tauriApi() : bridge ? bridgeApi() : mockApi();
  return api;
}
