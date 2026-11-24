// The count on the chip without opening the view: when to ask the backend
// for the engine run's one start-up search, and what to do with its answer.
// Kept apart from main.ts so it can be tested as it runs (startsearch.test.ts).
//
// The backend keeps to one search per engine run whatever asks
// (backend/network.rs network_start_search), and the Tauri command refuses
// while the window is not shown (commands.rs). The page adds its own rule: it
// asks only while the engine runs and the window is shown, and only once it
// knows whether the window is minimized (a login launch minimizes it before
// the page has heard so).
import type { StartSearch } from "./api";
import { startSearchDue, type Coverage } from "./chip";
import { coverageOf } from "./netview";

export interface StartSearchDeps {
  /** The backend call (api.networkStartSearch). */
  ask: () => Promise<StartSearch>;
  /** The engine run the app's state reports now. */
  currentRun: () => number;
  /** The run's counters arrived. */
  onCoverage: (c: Coverage, run: number) => void;
  onError?: (e: unknown) => void;
  /** The clock (tests). */
  now?: () => number;
}

/** How the backend begins a refusal in which nothing went to the network (backend NOT_SENT). */
export const NOT_SENT = "Not sent: ";
/** After such a refusal: ask again this long after it, while still shown, or at the next show. */
export const RETRY_MS = 5_000;
/** At most this many such retries in one run without the window being hidden and shown again. */
export const RETRIES = 3;

const message = (e: unknown) => (e instanceof Error ? e.message : typeof e === "string" ? e : "");

export interface Window {
  /** The page is visible (document.hidden is false). */
  visible: boolean;
  /** Whether the window is minimized, once known; null before the first answer. */
  minimized: boolean | null;
}

/** Shown: visible, and known not to be minimized. Unknown counts as not shown. */
export function windowShown(w: Window): boolean {
  return w.visible && w.minimized === false;
}

export function createStartSearch(deps: StartSearchDeps) {
  const now = deps.now ?? Date.now;
  let asked = 0;
  // A refusal that sent nothing (the window not shown after all, the engine
  // starting or busy): the run keeps its search. It is asked for again once
  // the window has been hidden and shown, or RETRY_MS later, RETRIES times.
  let refused = { run: 0, at: 0, tries: 0, sawHidden: false };
  return {
    /** Call on every state update and whenever the window may have come up. */
    maybe(engine: string, run: number, w: Window): boolean {
      const shown = windowShown(w);
      if (refused.run === run && refused.run !== 0) {
        if (!shown) refused.sawHidden = true;
        const again = refused.sawHidden || (refused.tries <= RETRIES && now() - refused.at >= RETRY_MS);
        if (!again) return false;
      }
      if (!startSearchDue(engine, run, asked, shown)) return false;
      asked = run;
      void deps.ask().then((r) => {
        if (refused.run === run) refused = { run: 0, at: 0, tries: 0, sawHidden: false };
        // An answer for a run that is over describes nothing now.
        if (r.run !== deps.currentRun()) return;
        const c = coverageOf(r.answer, r.at_ms);
        if (c) deps.onCoverage(c, r.run);
      }).catch((e) => {
        if (message(e).startsWith(NOT_SENT) && asked === run) {
          // Nothing went out: this run's one search is still to come.
          asked = 0;
          const tries = refused.run === run && !refused.sawHidden ? refused.tries + 1 : 1;
          refused = { run, at: now(), tries, sawHidden: false };
        }
        deps.onError?.(e);
      });
      return true;
    },
    /** The last run asked for (tests). */
    get asked() { return asked; },
  };
}
