// The ModelNet chip on the home screen, and the line on a model's card. Pure:
// what the chip says for a given state. It shows only what the engine
// reported. The number of hosts answering comes from the counters of the last
// network answer the Network view asked for; the chip asks the network
// nothing itself. Upstream's rule: counts are "this node's observations, not
// a global census" (HUMANS.md), so no number is ever made up or kept stale.

/** How many hosts a network call asked and how many answered, by the engine's own counters. */
export interface Coverage {
  /** Hosts and index peers the engine asked. */
  asked: number;
  /** Of those, how many answered. */
  answered: number;
  /** Of `asked`, how many were index peers. */
  indexPeers: number;
  /** When the answer came (ms since epoch). */
  atMs: number;
}

/** A coverage figure older than this is not shown as "answering" any more. */
export const COVERAGE_FRESH_MS = 10 * 60_000;

export type ChipTone = "off" | "on" | "busy" | "warn";

export interface Chip {
  tone: ChipTone;
  /** Words before the name: "Powered by", "Fetching over", or none. */
  lead: string;
  /** Words after the name, shown after a middle dot; empty for none. */
  detail: string;
}

export const CHIP_NAME = "BTX ModelNet";

/** The first-run sentence (plan section 6): the network by its name, and what AInode does with it. */
export function firstLede(device: string): string {
  return `Open AI models over ${CHIP_NAME}. Checked piece by piece, run on ${device}. No account.`;
}

export interface ChipInput {
  /** The engine's state as the app reports it (AppState.engine.state). */
  engine: string;
  /** Hosts the engine was given (AppState.network.peers_configured). */
  hosts: number;
  /** A download is running. */
  fetching: boolean;
  /** A running download's host has stopped answering (the engine's stalled job). */
  waiting: boolean;
  /** The last network answer's counters from this engine run, or null. */
  coverage: Coverage | null;
  nowMs: number;
}

export function hostsAnswering(n: number): string {
  return `${n} host${n === 1 ? "" : "s"} answering`;
}

/** What the chip shows. Every word is backed by something the engine reported. */
export function chipOf(i: ChipInput): Chip {
  // Not running, or running with no host to reach: the plain name, grey.
  if (i.engine !== "running" || i.hosts < 1) return { tone: "off", lead: "", detail: "" };
  if (i.fetching) {
    return i.waiting
      ? { tone: "warn", lead: "", detail: "no host answered" }
      : { tone: "busy", lead: "Fetching over", detail: "each piece checked" };
  }
  const c = i.coverage;
  const fresh = !!c && i.nowMs - c.atMs >= 0 && i.nowMs - c.atMs <= COVERAGE_FRESH_MS;
  if (fresh && c.asked > 0 && c.answered === 0) return { tone: "warn", lead: "", detail: "no host answered" };
  if (fresh && c.answered > 0) return { tone: "on", lead: "Powered by", detail: hostsAnswering(c.answered) };
  return { tone: "on", lead: "Powered by", detail: "" };
}

/**
 * Whether the page should ask the backend for the start-up search now (the
 * count on the chip without opening the view). Once per engine run: `run` is
 * the backend's count of engine starts, `asked` the last run the page asked
 * for. Only while the engine runs and the window is shown. The backend keeps
 * to one search per run whatever the page asks (backend/network.rs).
 */
export function startSearchDue(engine: string, run: number, asked: number, shown: boolean): boolean {
  return engine === "running" && run > 0 && run !== asked && shown;
}

/** The chip as one sentence, for the tooltip and for screen readers. */
export function chipLabel(c: Chip): string {
  return `${c.lead ? `${c.lead} ` : ""}${CHIP_NAME}${c.detail ? ` · ${c.detail}` : ""}`;
}

/** The chip's tooltip: what a click does, and whose view the number is. */
export function chipTitle(c: Chip): string {
  const base = "Open the BTX ModelNet view";
  if (c.detail.endsWith("answering")) return `${base}. Hosts that answered this AInode's last network request; not a count of the whole network.`;
  if (c.detail === "no host answered") return `${base}. No host answered this AInode's last request. Verified pieces are kept.`;
  return base;
}

/**
 * The line on the card of a model that is on this computer. "Verified" is
 * about the bytes: every piece matched the link and the finished file matched
 * the publisher's checksum. It says nothing about what the model is or does.
 */
export const CARD_LINE = "Bytes verified · over BTX ModelNet";
export const CARD_LINE_TITLE =
  "Every piece was checked against the model's btx:// link, and the finished file against the publisher's checksum. Checked means intact, not safe or correct.";
