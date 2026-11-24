// How the Network view reads upstream's cards (searchbounties, getfundablemodels,
// searchmodels, getmodelsharecard). Pure and lenient: these are upstream's JSON,
// whose schema versions move (search.md documents 2, the live helper answers 3),
// so every field is optional and nothing here throws.
//
// The copy rules are upstream's own (docs/modelnet/5-discovery.md, section 5):
// a list is "this node's current network view", never the whole network; a
// funded figure is shown only when the engine says it is known from the chain
// (`value_known` with `funding_source: CHAIN_OBSERVATION`, or a bounty's
// `funding_progress_known`); a pledge is never shown as funded; where the
// engine cannot know, the view says "not known here" and never "0".
// The shapes were captured from real engines on 2026-11-23
// (lab/evidence/2026-11-23/modelnet-netview/).
//
// Every string on a card came from some host. It is shown escaped (network.ts)
// and it reaches the clipboard only through safe.ts: an id, a hash or a link
// that is not exactly in upstream's form is not offered for copying.
import type { Coverage } from "./chip";
import { fmtBytes } from "./logic";
import { blockHeight, hash64, id96, modelLink } from "./safe";

type Obj = Record<string, unknown>;
const str = (v: unknown): string => (typeof v === "string" ? v : typeof v === "number" ? String(v) : "");
const num = (v: unknown): number => (typeof v === "number" ? v : typeof v === "string" && /^\d+$/.test(v) ? Number(v) : 0);
const obj = (v: unknown): Obj => (v && typeof v === "object" && !Array.isArray(v) ? (v as Obj) : {});
const first = (...v: unknown[]): string => v.map(str).find((s) => s !== "") ?? "";

/** BTX amounts arrive as atoms, a string or a number; 1 BTX = 100,000,000 atoms. */
export function btxOf(atoms: unknown): string {
  const a = num(atoms);
  if (!a) return "";
  const b = a / 1e8;
  return `${b >= 100 ? Math.round(b).toLocaleString("en-US") : Number(b.toFixed(4)).toString()} BTX`;
}

/** upstream's trust labels in words a person can weigh. Unknown ones pass through, readable. */
export function trustText(label: unknown): string {
  const l = str(label);
  if (l === "COUNCIL_CUSTODIAL_AUTHORITY_WITH_INDIVIDUAL_REFUND_PATHS") {
    return "A council decides who wins. Each funder can take their part back if nobody is awarded in time.";
  }
  return l ? l.toLowerCase().replace(/_/g, " ") : "";
}

export const NOT_KNOWN_HERE = "Raised: not known here";
export const WHOSE_VIEW = "This is this AInode's current network view, not a complete directory.";
export const BOUNTY_TERMS_NOT_CARRIED = "Prize and deadlines are not carried by the network yet.";
export const ID_NOT_IN_FORM = "id not in the expected form";

/** The first characters of a long id, for a card. The whole id is what Copy gives. */
export function shortId(id: string, n = 16): string {
  return id.length > n ? `${id.slice(0, n)}…` : id;
}

export interface BountyView {
  /** The bounty id, only if it has upstream's form (96 hex); else "". */
  id: string;
  /** The card carried an id that is not in that form. It is neither shown nor copied. */
  idBad: boolean;
  title: string; prize: string; trust: string; text: string;
  /**
   * The signer the record names, only if it has upstream's form. The bounty
   * search hands on what a host sent without checking its signature
   * (upstream v0.34.16 helper.cpp:6799, :6827), so this is a name, not a proof.
   */
  signer: string;
  /** "Raised: …" line. A figure only when the engine knows it from the chain. */
  raised: string;
  /** True when the card carries no prize: a bounty some other helper published. */
  termsMissing: boolean;
}

export function bountyView(r: Obj): BountyView {
  const terms = obj(r.terms);
  const econ = obj(r.economy);
  const prize = btxOf(r.target_atoms ?? terms.target_atoms);
  const known = econ.funding_progress_known === true && num(econ.confirmed_atoms) > 0;
  const goal = btxOf(econ.target_atoms ?? r.target_atoms ?? terms.target_atoms);
  // A bounty's model id is its bounty id (upstream's publish step sets both).
  const rawId = first(r.bounty_id, r.model_id);
  return {
    id: id96(rawId),
    idBad: rawId !== "" && id96(rawId) === "",
    title: first(r.title, terms.title, r.display_name, r.canonical_name, r.name) || "Untitled bounty",
    prize,
    trust: trustText(r.trust_label),
    text: first(r.description, terms.description, r.short_description),
    // Named only by a record that says it is signed, and only in upstream's form.
    signer: r.signed_metadata === true || r.verified === true ? id96(first(r.publisher_identity, r.signer_id)) : "",
    raised: known ? `Raised: ${btxOf(econ.confirmed_atoms)}${goal ? ` of ${goal}` : ""}` : NOT_KNOWN_HERE,
    termsMissing: !prize,
  };
}

/**
 * The bounties of one searchbounties answer, each once. The engine returns a
 * bounty some other helper published twice: as a result card (model_id and
 * name, no bounty_id) and as the raw record (bounty_id, canonical_name). For a
 * bounty the two ids are the same.
 *
 * Nothing is mixed between the two: everything shown for a bounty comes from
 * ONE result. A result that names a signer wins over one that names none;
 * among equals the first wins. So a signer is never shown beside a title, a
 * text or a prize that came from another result. A result without an id in
 * upstream's form cannot be matched to anything and stays on its own.
 */
export function mergeBounties(results: Obj[]): BountyView[] {
  const at = new Map<string, number>();
  const out: BountyView[] = [];
  for (const r of results) {
    const v = bountyView(r);
    const i = v.id ? at.get(v.id) : undefined;
    if (i === undefined) {
      if (v.id) at.set(v.id, out.length);
      out.push(v);
    } else if (!out[i].signer && v.signer) {
      out[i] = v;
    }
  }
  return out;
}

/** A search record that is a bounty, not a model: kept out of model lists. */
export function isBounty(r: Obj): boolean {
  return r.object_kind === "BOUNTY" || str(r.bounty_id) !== "";
}

/**
 * A search result that is only this computer's own copy: the engine adds its
 * local store to a network search, and a model it holds with no signed record
 * comes back as "retrieved" with no publisher. That is not something the
 * network returned, so it stays out of the network lists.
 */
export function isOwnCopyOnly(r: Obj): boolean {
  return obj(r.local).known === true && obj(r.search).metadata_verified !== true && r.signed_metadata !== true && obj(r.share).signed !== true;
}

export interface ModelView {
  /** The model's link, only if the backend's check would accept it; else "". */
  uri: string;
  name: string;
  /** The publisher's name as the record gives it. Self-chosen: anyone can sign a record that says "IBM". */
  by: string;
  size: string; text: string;
  /** The engine checked the record's signature (model search and campaign cards are built from its own checked index). */
  signed: boolean;
  /** The id of the key that signed, if the card names it in upstream's form. */
  signerId: string;
}

/** A searchmodels result card, a getfundablemodels card, or a share card. */
export function modelView(r: Obj): ModelView {
  const share = obj(r.share);
  const size = num(r.size_bytes) || num(share.size_bytes) || num(r.bytes);
  const signed = r.signed_metadata === true || share.signed === true || obj(r.search).metadata_verified === true;
  return {
    uri: modelLink(str(r.uri)) || modelLink(str(share.uri)),
    name: str(r.name) || str(r.title) || str(obj(r.release).name) || "",
    // A search card's publisher is { id, display_name } (0.34.15); older shapes a string.
    by: str(r.publisher) || str(obj(r.publisher).display_name) || str(r.family) || str(share.family),
    size: size ? fmtBytes(size) : "",
    text: str(r.description),
    signed,
    signerId: signed ? id96(first(obj(r.publisher).id, r.publisher_identity, r.signer_id)) : "",
  };
}

/**
 * The line under a model's name: who it says it is from, and what was checked.
 * A signature proves which key signed the record, not that the name on it is
 * true, so a name from the network is always marked "name as given".
 * `trusted`: the name comes from AInode's own store, not from the network.
 */
export function byLine(m: Pick<ModelView, "by" | "signed" | "signerId">, trusted = false): string {
  if (trusted) return m.by;
  const what = m.signed ? (m.signerId ? `signed by key ${shortId(m.signerId, 8)}` : "signed record") : "not signed";
  return m.by ? `${m.by} (name as given) · ${what}` : what[0].toUpperCase() + what.slice(1);
}

/** A lifecycle label in plain words. Labels are the engine's view, not consensus. */
export function stateText(label: unknown): string {
  switch (str(label)) {
    case "FUNDING": return "Open for funding";
    case "FUNDING_FROZEN": return "Funding is paused";
    case "FUNDED_AWAITING_RELEASE": return "Funded, waiting for its publisher to release it";
    case "SECRET_DISCLOSED": case "PUBLIC_RELEASED": return "Released";
    case "REFUND_AVAILABLE": return "Not released in time: funders can take their part back";
    case "PUBLIC": case "": return "";
    default: return str(label).toLowerCase().replace(/_/g, " ");
  }
}

export interface CampaignView extends ModelView {
  /** The release id, only if it has upstream's form (96 hex); else "". */
  id: string;
  /** The card carried a release id that is not in that form. It is neither shown nor copied. */
  idBad: boolean;
  goal: string;
  state: string;
  /** "Raised: …" line. A figure only when the engine knows it from the chain. */
  raised: string;
  /** The block after which funders can take their part back; 0 if the card names none or one out of range. */
  refundHeight: number;
  /** The campaign's key hash, only if it has upstream's form (64 hex); else "". */
  keyHash: string;
}

/** A campaign card: getfundablemodels, or a searchmodels card with a release. The numbers are under `release`. */
export function campaignView(r: Obj): CampaignView {
  const rel = obj(r.release);
  const target = rel.target_atoms ?? r.release_target_atoms ?? r.target_atoms;
  const goal = btxOf(target);
  // Upstream: funded is chain-backed only when value_known and CHAIN_OBSERVATION.
  // Pledges (pledged_atoms, pledged_percent) are never shown as raised.
  const known = rel.value_known === true && (rel.chain_observation === true || rel.funding_source === "CHAIN_OBSERVATION");
  const confirmed = num(rel.confirmed_funded_atoms);
  const t = num(target);
  const pct = known && t > 0 ? ` (${Math.min(100, Math.floor((confirmed / t) * 100))}%)` : "";
  const rawId = first(rel.release_id, rel.id, r.release_id);
  return {
    ...modelView(r),
    id: id96(rawId),
    idBad: rawId !== "" && id96(rawId) === "",
    goal,
    state: stateText(first(rel.state, r.lifecycle_state, r.release_state)),
    raised: known ? `Raised: ${confirmed ? btxOf(confirmed) : "0 BTX"}${goal ? ` of ${goal}` : ""}${pct}` : NOT_KNOWN_HERE,
    refundHeight: blockHeight(rel.refund_height ?? r.refund_height),
    keyHash: hash64(first(rel.key_hash, r.key_hash)),
  };
}

/** True for a card that is a release campaign, in a model search or a campaign list. */
export function isCampaign(r: Obj): boolean {
  const rel = obj(r.release);
  return r.result_type === "RELEASE_CAMPAIGN" || str(rel.release_id) !== "" || num(rel.target_atoms) > 0;
}

/**
 * The campaigns of the campaign list and of the model search, each once. Two
 * cards are the same campaign only if they share a release id in upstream's
 * form or a model link the backend accepts; a card with neither is kept as it
 * is, since nothing says it repeats another.
 */
export function mergeCampaigns(cards: Obj[]): CampaignView[] {
  const seen = new Set<string>();
  const out: CampaignView[] = [];
  for (const r of cards) {
    const c = campaignView(r);
    const key = c.id || c.uri;
    if (key && seen.has(key)) continue;
    if (key) seen.add(key);
    out.push(c);
  }
  return out;
}

/** The `results` list of an upstream reply, whatever else it carries. */
export function resultsOf(reply: unknown): Obj[] {
  const r = obj(reply).results;
  return Array.isArray(r) ? r.filter((x): x is Obj => !!x && typeof x === "object") : [];
}

/**
 * The engine's own counters of one network answer: searchmodels and
 * getmodelfeed carry them under `coverage`, searchbounties at the top.
 * Null when the answer carries none (a local call, an error).
 */
export function coverageOf(reply: unknown, atMs: number): Coverage | null {
  const top = obj(reply);
  const c = "coverage" in top ? obj(top.coverage) : top;
  if (!("responses_received" in c) && !("timed_out" in c)) return null;
  const answered = num(c.responses_received);
  const asked = Math.max(answered + num(c.timed_out), num(c.connected_peers_queried) + num(c.index_peers_queried));
  // The engine counts an index peer in index_peers_queried only when it
  // answered (upstream v0.34.16 helper.cpp, the search fan-out), and asks every
  // configured one; a host that is both the model host and an index peer is
  // asked once, as an index peer. So the index peers asked are the configured
  // ones, up to the number of hosts asked (measured 2026-11-24 with one that
  // does not exist: configured 2, queried 1, responses 1, timed out 1).
  const indexPeers = Math.min(asked, Math.max(num(c.index_peers_queried), num(c.index_peers_configured)));
  return { asked, answered, indexPeers, atMs };
}

/**
 * The note over a list that is not empty although no host answered: the
 * engine answers a network search from the signed records it kept too, so
 * what is listed then came from earlier answers, not from this one.
 */
export function silentNote(c: Coverage | null): string {
  return c && c.asked > 0 && c.answered === 0 ? "No host answered this time. Shown: what this AInode's engine kept from earlier answers." : "";
}

/**
 * The line under the list of hosts this AInode asks when it searches. The
 * engine reports totals for a search, not which host answered (upstream
 * v0.34.16 search.h SearchCoverage), so the line says what it knows and says
 * that it cannot tell more.
 */
export function indexStatusText(listed: number, c: Coverage | null, nowMs: number): string {
  if (!c) return "Not asked yet in this engine run. The hosts are asked when a search runs.";
  const asked = coverageText(c);
  const when = agoText(c.atMs, nowMs);
  if (listed < 1) return `Last search, ${when}: ${asked}`;
  const which = c.indexPeers > 0 && c.asked > 1 ? " The engine does not say which host answered." : "";
  return `Last search, ${when}: ${asked}${which}`;
}

const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;

/**
 * Why the view has no model list this time, in words a person can act on.
 * The engine asks the hosts one after another and waits 1.5 s for each that
 * is down, and it answers one call at a time: right after an engine start the
 * view's search can wait behind the start-up search and run out of time
 * (measured 2026-11-24 with hosts that do not answer). The engine's own words
 * ("engine did not answer searchmodels in time") meant nothing to a user.
 */
export function searchFailText(why: string): string {
  if (/in time|timed? ?out|busy/i.test(why)) {
    return "No answer in time: the hosts may be down, or the engine was still busy with another search. Press Refresh to ask again.";
  }
  return `The network search did not answer: ${why}`;
}

/** "Asked 1 host and 1 index peer. 1 answered." From the counters, nothing else. */
export function coverageText(c: Coverage | null): string {
  if (!c) return "";
  if (c.asked < 1) return "No host was asked.";
  const hosts = c.asked - c.indexPeers;
  const who = [hosts > 0 ? plural(hosts, "host") : "", c.indexPeers > 0 ? plural(c.indexPeers, "index peer") : ""].filter(Boolean).join(" and ");
  return `Asked ${who}. ${c.answered === 0 ? "None answered." : `${c.answered} answered.`}`;
}

/** "just now", "12 seconds ago", "4 minutes ago": how old an answer is. */
export function agoText(atMs: number, nowMs: number): string {
  const s = Math.max(0, Math.round((nowMs - atMs) / 1000));
  if (s < 5) return "just now";
  if (s < 90) return `${s} seconds ago`;
  const m = Math.round(s / 60);
  return m < 90 ? `${m} minutes ago` : `${Math.round(m / 60)} hours ago`;
}

/** A list as a host last answered it with something in it, kept to show greyed when a refresh fails or comes back empty. */
export interface Kept<T> { items: T[]; atMs: number }
/** A kept answer older than this is dropped: it would no longer describe the network. */
export const KEEP_MS = 30 * 60_000;

/**
 * What to show for one list.
 * - The fresh answer when it has something. It becomes the kept answer, with
 *   this moment as its time, only if a host answered (`hostAnswered`): a list
 *   the engine filled from records it kept while no host answered is shown
 *   with the `quiet` note, and the kept answer keeps the time a host last
 *   answered.
 * - Else the kept answer, greyed and dated, when a refresh failed (`fresh`
 *   null) or came back empty. An empty answer from the one host can be wrong:
 *   on 2026-11-23 it answered a search empty and correctly minutes later.
 */
export function settle<T>(
  fresh: T[] | null, kept: Kept<T> | null, now: number, hostAnswered = true, quiet = "",
): { items: T[]; stale: string; kept: Kept<T> | null } {
  const live = kept && now - kept.atMs <= KEEP_MS ? kept : null;
  if (fresh && fresh.length) {
    return hostAnswered ? { items: fresh, stale: "", kept: { items: fresh, atMs: now } } : { items: fresh, stale: quiet, kept: live };
  }
  if (!live) return { items: [], stale: "", kept: null };
  const why = fresh ? "Nothing came back this time." : "The engine did not answer this time.";
  return { items: live.items, stale: `${why} Shown: the answer from ${agoText(live.atMs, now)}.`, kept: live };
}

export interface FundHow {
  /** Plain sentences, in order. */
  lines: string[];
  /** A label and the text Copy gives. Every text passed safe.ts. */
  copies: { label: string; text: string }[];
}

const FUND_NEEDS_NODE = "Funding happens in a BTX node's wallet, not in AInode. AInode has no wallet and never asks for one.";
const FUND_ESCROW = "A funder pays into an escrow that holds their own refund key, so only their node can take the money back. bonuz and PQ Wallet cannot make that payment yet.";
const FUND_READ_FIRST = "Read the terms before you commit. A card is not a payment instruction.";
const NOT_COPIED = "This card's id is not in the form BTX gives its ids, so AInode offers nothing to copy from it. Do not type it into a terminal.";

/**
 * "How to fund this" for a campaign: what a person with a BTX node needs. The
 * command is upstream's own first step (v0.34.16 `preparefundmodelrelease`,
 * which "never signs or broadcasts"), filled with what the card carries. The
 * amount and the publisher's claim key are left for the person: AInode never
 * chooses an amount, and the network card does not carry that key.
 *
 * The command is built only from a release id of 96 hex, a key hash of 64 hex
 * and a block height: nothing else from the card can reach it, so nothing in
 * it can break out of its quotes in a shell.
 */
export function fundCampaign(c: CampaignView): FundHow {
  const command = c.id && c.keyHash && c.refundHeight
    ? `btx-cli preparefundmodelrelease "${c.id}" '{"key_hash":"${c.keyHash}","claimant":"PUBLISHERS_CLAIM_KEY","refund_height":${c.refundHeight},"amount_atoms":"YOUR_AMOUNT_IN_ATOMS"}'`
    : "";
  const copies = [
    c.id ? { label: "Copy release id", text: c.id } : null,
    command ? { label: "Copy first command", text: command } : null,
  ].filter((x): x is { label: string; text: string } => !!x);
  return {
    lines: [
      FUND_NEEDS_NODE,
      FUND_ESCROW,
      c.idBad
        ? NOT_COPIED
        : !c.id
          ? "This card carries no release id, so there is nothing to hand to a node."
          : command
            ? "On a BTX node with a wallet, the first step is the command below. It makes an unsigned plan and sends nothing. Two parts are yours to fill in: your amount (1 BTX is 100,000,000 atoms) and the publisher's claim key, which the network does not carry. Ask the publisher for it."
            : "This card's key hash or refund block is missing or not in the expected form, so AInode builds no command from it. The release id can be copied; a node owner needs the campaign's terms from its publisher.",
      c.refundHeight ? `If the model is not released, a funder's node can take its part back after block ${c.refundHeight.toLocaleString("en-US")}.` : "",
      FUND_READ_FIRST,
    ].filter(Boolean),
    copies,
  };
}

/** "How to fund this" for a bounty. For one some other helper published, the terms a node needs are not on the network. */
export function fundBounty(b: BountyView): FundHow {
  return {
    lines: [
      FUND_NEEDS_NODE,
      FUND_ESCROW,
      b.idBad ? NOT_COPIED : "",
      b.termsMissing
        ? "The network carries this bounty's name, text, named signer and id. It does not carry the terms a node needs to fund it: the council's keys, the deadlines, the prize. Ask the bounty's publisher for its terms file."
        : "A node funds one lot at a time from the bounty's terms: the council's keys, the deadlines and the funder's own refund key.",
      "With the terms, a BTX node with a wallet funds a lot with preparebountyfunding, then signbountyfunding, then submitbountyfunding. `btx-cli help preparebountyfunding` on the node shows the fields.",
      FUND_READ_FIRST,
    ].filter(Boolean),
    copies: b.id ? [{ label: "Copy bounty id", text: b.id }] : [],
  };
}

/** A link as typed or pasted, trimmed; the backend checks it fully (validate::model_uri). */
export function looksLikeLink(s: string): boolean {
  return /^\s*btx:\/\/\S+\s*$/.test(s);
}
