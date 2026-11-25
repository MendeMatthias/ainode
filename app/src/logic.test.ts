import { execFileSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import type { AppState, CatalogModel, WslState } from "./api";
import { WSL_RESTART_LINE, wslBarText, wslAgainStatus, WSL_AGAIN_OK_S, WSL_AGAIN_WARN_S, btxNodesOf, btxNodesText, loginNoteText, easyNodeRestarted, feedbackUrl, gpuNoteText, nodeHereText, HIDDEN_POLL_MS, pollDue, systemLine, updateDue, updateNote, updateOfferable, UPDATE_EVERY_MS, autoInstallNow, awayNow } from "./logic";
import { actionFor, ainodesText, countDue, newerCommunity, chatErrorText, cleanAnswer, loopCut, stopIntended, STOP_GRACE_MS, engineErrorText, plainMath, etaText, fmtBytes, fmtPct, friendlyError, PRESETS, readSize, roomLeft, startSize, statusOf, SYSTEM_PROMPT, KNOWLEDGE, fitHistory, mdBlocks, systemPrompt, prefsFor, lengthPlan, TEMPERATURES } from "./logic";

const GiB = 1024 ** 3;

function state(over: Partial<AppState> = {}): AppState {
  return {
    version: "0.1.0",
    consent: { total_bytes: 25e9, engine_bytes: 11 * GiB, accepted_at: 0 },
    disk: { free_bytes: 200 * GiB, total_bytes: 500 * GiB },
    engine: { state: "running" },
    network: { peers_configured: 1, peers_connected: 1 },
    usage: { engine_used_bytes: 0, runnable_bytes: 0, total_bytes: 0 },
    space: { min_gb: 4, max_gb: 156 },
    ram_bytes: 24 * GiB,
    theme: "system",
    autostart: true,
    think: false,
    ...over,
  };
}

const model: CatalogModel = {
  id: "m", name: "M", publisher: "P", licence: "Apache-2.0", licence_url: "", source_url: "", summary: "",
  uri: "btx://x", size_bytes: 2_244_022_910, local: "none", progress: 0, runnable: false, ram_gb: 0,
};

/** A status line with its non-breaking spaces read as plain ones. */
const plain = (line: string) => line.replace(/\u00a0/g, " ");

describe("fmtBytes", () => {
  it("uses decimal units like Finder", () => {
    expect(fmtBytes(2_244_022_910)).toBe("2.2 GB");
    expect(fmtBytes(236_997_118)).toBe("237 MB");
    expect(fmtBytes(25 * 1e9)).toBe("25 GB");
    expect(fmtBytes(0)).toBe("0 GB");
  });
});

describe("fmtPct", () => {
  it("floors, so 99.9% never shows as done", () => {
    expect(fmtPct(0.999)).toBe("99%");
    expect(fmtPct(1)).toBe("100%");
    expect(fmtPct(-1)).toBe("0%");
  });
});

describe("the AInodes online", () => {
  const community = { online: 7, platforms: { macos: 3, windows: 3, linux: 1 }, enginesRunning: 6, withModels: 5, seenToday: 20, at: 0 };
  it("puts the count before the hosts when the engine runs", () => {
    expect(plain(statusOf(state(), false, community).line)).toBe("7 AInodes · 1 host");
    expect(plain(statusOf(state(), false, { ...community, online: 1 }).line)).toBe("1 AInode · 1 host");
    expect(plain(statusOf(state(), false, null).line)).toBe("1 host");
    // The BTX nodes keeping the chain (easybtx.com's census, in the same count) come last.
    expect(plain(statusOf(state(), false, { ...community, btxNodes: { live: 46, countries: 15 } }).line)).toBe("7 AInodes · 1 host · 46 BTX nodes");
    // A narrow window wraps between the figures, never inside one.
    expect(statusOf(state(), false, { ...community, btxNodes: { live: 46, countries: 15 } }).line).toBe("7\u00a0AInodes · 1\u00a0host · 46\u00a0BTX\u00a0nodes");
    expect(plain(statusOf(state(), false, { ...community, btxNodes: null }).line)).toBe("7 AInodes · 1 host");
    // The BTX nodes are a link to easybtx.com's census of them; nothing else is.
    expect(statusOf(state(), false, { ...community, btxNodes: { live: 46, countries: 15 } }).link).toEqual({ text: "46\u00a0BTX\u00a0nodes", url: "https://easybtx.com/nodes" });
    expect(statusOf(state(), false, community).link).toBeUndefined();
    expect(statusOf(state(), true, { ...community, btxNodes: { live: 46, countries: 15 } }).link).toBeUndefined();
    expect(statusOf(state(), true, { ...community, btxNodes: { live: 46, countries: 15 } }).line).toBe("Fetching and verifying · 1 host");
    expect(statusOf(state(), true, community).line).toBe("Fetching and verifying · 1 host");
    expect(ainodesText(25)).toBe("25 AInodes");
  });
  it("reads the public count every minute (Rust checks in)", () => {
    const t = 10 * 60_000;
    expect(countDue(t, 0)).toBe(true);
    expect(countDue(t, t - 30_000)).toBe(false);
    expect(countDue(t, t - 60_000)).toBe(true);
  });
  it("says easyNode is restarting WSL when its note came just before the stop", () => {
    const crashed = state({ autostart: true, engine: { state: "error", error: "The engine stopped by signal 15" } });
    expect(statusOf(crashed, false, null, true, true).line).toBe("easyNode is restarting WSL to get its graphics card back. AInode starts again by itself");
    expect(statusOf(crashed, false, null, true, true).tone).toBe("busy");
    // Once AInode gave up, the Retry button is the user's: the error shows, whoever caused it.
    expect(statusOf(crashed, false, null, false, true).line).toMatch(/Press to start it again/);
    const now = 1_791_997_286_000;
    expect(easyNodeRestarted(now / 1000 - 20, now)).toBe(true);
    expect(easyNodeRestarted(now / 1000 - 300, now)).toBe(true);
    expect(easyNodeRestarted(now / 1000 - 301, now)).toBe(false);
    expect(easyNodeRestarted(now / 1000 + 120, now)).toBe(false);
    expect(easyNodeRestarted(null, now)).toBe(false);
  });
  it("never lets an older cached count replace a newer one", () => {
    const at = (online: number, a: number) => ({ ...community, online, at: a });
    expect(newerCommunity(null, at(2, 100))?.online).toBe(2);
    expect(newerCommunity(at(3, 200), at(2, 100))?.online).toBe(3);
    expect(newerCommunity(at(2, 100), at(3, 160))?.online).toBe(3);
    expect(newerCommunity(at(2, 100), null)?.online).toBe(2);
  });
});

describe("statusOf", () => {
  it("asks for consent before anything else", () => {
    expect(statusOf(state({ consent: null })).line).toMatch(/space/);
  });
  it("is honest when no public host exists", () => {
    const s = statusOf(state({ network: { peers_configured: 0, peers_connected: 0 } }));
    expect(s.tone).toBe("warn");
    expect(s.line).toMatch(/No public host/);
  });
  it("counts listed hosts with the right plural, not open connections", () => {
    expect(plain(statusOf(state()).line)).toBe("1 host");
    // peers_connected is 0 whenever nothing downloads: it must not read as "unreachable"
    expect(statusOf(state({ network: { peers_configured: 3, peers_connected: 0 } })).line).toBe("3\u00a0hosts");
    expect(statusOf(state(), true).line).toBe("Fetching and verifying · 1 host");
  });
  it("shows the engine error in plain words", () => {
    const s = statusOf(state({ engine: { state: "error", error: "openssl mldsa44 certificate generation failed" } }));
    expect(s.tone).toBe("error");
    expect(s.line).toBe("Could not start. Its post-quantum identity could not be created. Reinstall AInode.");
    expect(engineErrorText("The engine was stopped by signal 9")).toMatch(/stopped unexpectedly/);
    expect(engineErrorText("something new")).toBe("something new");
  });
  it("never mentions earning", () => {
    const scenes = [state(), state({ consent: null }), state({ engine: { state: "stopped" } })];
    for (const sc of scenes) expect(statusOf(sc).line).not.toMatch(/earn|reward|income|pay/i);
  });
});

describe("actionFor", () => {
  it("offers Get only when the engine runs and a host exists", () => {
    expect(actionFor(model, state()).kind).toBe("get");
    expect(actionFor(model, state({ engine: { state: "stopped" } })).hint).toBe("Start first");
    expect(actionFor(model, state({ network: { peers_configured: 0, peers_connected: 0 } })).hint).toBe("No host yet");
  });
  it("refuses a model the engine's half of the budget cannot hold (review finding: eviction)", () => {
    // 6 GB consent -> 2 GiB engine store; the 2.24 GB model fits 2x in 6 GB but not in 2 GiB.
    const six = state({ consent: { total_bytes: 6e9, engine_bytes: 2 * GiB, accepted_at: 0 } });
    expect(actionFor(model, six).hint).toBe("Not enough space");
  });
  it("counts a model twice against the budget (pieces + runnable copy)", () => {
    const tight = state({ consent: { total_bytes: 4e9, engine_bytes: 1 * GiB, accepted_at: 0 } });
    expect(actionFor(model, tight).hint).toBe("Not enough space");
  });
  it("offers Retry after a failure and Resume after a cancel", () => {
    expect(actionFor({ ...model, error: "disk quota" }, state()).label).toBe("Retry");
    expect(actionFor({ ...model, progress: 0.3 }, state()).label).toBe("Resume");
  });
  it("shows progress and chat", () => {
    expect(actionFor({ ...model, local: "downloading", progress: 0.42 }, state()).label).toBe("42%");
    expect(actionFor({ ...model, local: "ready" }, state()).kind).toBe("chat");
  });
});

describe("a model kept only for removal", () => {
  it("offers Remove, never Chat: Mende opened the dropped 350M and it looped", () => {
    expect(actionFor({ ...model, offered: false, local: "ready", runnable: true }, state())).toEqual({ kind: "remove", label: "Remove" });
    expect(actionFor({ ...model, offered: true, local: "ready" }, state()).kind).toBe("chat");
  });
});

describe("stopIntended", () => {
  it("a double click on Send sends once and does not stop what it sent", () => {
    expect(stopIntended(2, 80)).toBe(false);
    expect(stopIntended(2, 900)).toBe(false);
  });
  it("a click right after sending is not a Stop", () => {
    expect(stopIntended(1, 0)).toBe(false);
    expect(stopIntended(1, STOP_GRACE_MS - 1)).toBe(false);
  });
  it("a single click on Stop later stops the reply", () => {
    expect(stopIntended(1, STOP_GRACE_MS)).toBe(true);
    expect(stopIntended(1, 30_000)).toBe(true);
    // A keyboard or scripted click reports no click count.
    expect(stopIntended(0, 2_000)).toBe(true);
  });
});

describe("loopCut", () => {
  // The 350M's own loop, as it streamed in Mende's chat.
  const cycle = "I am an AI model that IBM developed. I am an AI model that IBM developed. I am IBM's own AI model model. " +
    "I am a model that runs on the BTX blockchain and peer-to-peer model network. ";
  it("cuts a looping answer after its first repetition", () => {
    const text = "Hello! " + cycle.repeat(12);
    const cut = loopCut(text);
    expect(cut).not.toBeNull();
    expect(cut!).toBeLessThan(7 + cycle.length * 2 + 1);
    expect(text.slice(0, cut!)).toContain("peer-to-peer model network");
  });
  it("leaves real answers alone: short ones, varied ones, code with a repeated line", () => {
    expect(loopCut(cycle.repeat(2))).toBeNull();
    const prose = Array.from({ length: 40 }, (_, i) => `Point ${i}: something different about step ${i * 7}.`).join(" ");
    expect(loopCut(prose)).toBeNull();
    const code = "def f(x):\n" + Array.from({ length: 30 }, (_, i) => `    total += values[${i}] * weights[${i}]  # accumulate\n`).join("");
    expect(loopCut(code)).toBeNull();
  });
});

describe("memory", () => {
  // hw.memsize reports binary bytes: a "24 GB" Mac has 24 GiB.
  const mac16 = state({ ram_bytes: 16 * GiB });
  it("offers Get only on a Mac with enough memory", () => {
    expect(actionFor({ ...model, ram_gb: 16 }, mac16).kind).toBe("get");
    const a = actionFor({ ...model, ram_gb: 24 }, mac16);
    expect(a).toEqual({ kind: "blocked", label: "Get", hint: "Needs a Mac with 24 GB of memory" });
  });
  it("still lets a model already on the Mac be used, and ignores an unknown size", () => {
    expect(actionFor({ ...model, ram_gb: 24, local: "ready" }, mac16).kind).toBe("chat");
    expect(actionFor({ ...model, ram_gb: 24 }, state({ ram_bytes: 0 })).kind).toBe("get");
  });
});

describe("storage", () => {
  // The limits come from Rust (state.space, state::consent_limits); these only
  // pick a starting point inside them and read what the user types.
  it("starts on the current size, else Medium, else the biggest preset that fits", () => {
    expect(startSize(4, 156, 40)).toBe(40);
    expect(startSize(4, 156, null)).toBe(25);
    expect(startSize(4, 20, null)).toBe(10);
    expect(startSize(30, 500, null)).toBe(100);
    expect(startSize(4, 6, null)).toBe(6); // no preset fits: the most allowed
    expect(startSize(7, 161, 5)).toBe(25); // an old size now below the minimum
    expect(startSize(10, 8, null)).toBeNull(); // disk full
  });
  it("offers Minimal, Medium and Big", () => {
    expect(PRESETS.map((p) => `${p.name} ${p.gb}`)).toEqual(["Minimal 10", "Medium 25", "Big 100"]);
  });
  it("reads a typed size leniently and checks it against the limits", () => {
    expect(readSize("200", 4, 500)).toEqual({ gb: 200 });
    expect(readSize(" 200 GB ", 4, 500)).toEqual({ gb: 200 });
    expect(readSize("200gb", 4, 500)).toEqual({ gb: 200 });
    expect(readSize("200", 4, 156)).toEqual({ error: "At most 156 GB: that is what this Mac can spare now." });
    expect(readSize("3", 7, 156)).toEqual({ error: "At least 7 GB." });
    for (const bad of ["", "abc", "12.5", "-5", "1e3", "20 TB"]) expect(readSize(bad, 4, 500)).toHaveProperty("error");
  });
  it("shows the chosen number back unchanged", () => {
    expect(fmtBytes(25 * 1e9)).toBe("25 GB");
  });
  it("roomLeft subtracts what is used", () => {
    expect(roomLeft(state({ usage: { engine_used_bytes: 1, runnable_bytes: 1, total_bytes: 5e9 } }))).toBe(20e9);
  });
});

describe("friendlyError", () => {
  // The exact strings the engine produced in the lab (docs/experiments.md).
  it("says plainly when the host lacks the model (seen on the first public host)", () => {
    expect(friendlyError("manifest failed")).toMatch(/does not have this model/);
  });
  it("explains a changed host identity without alarming wording", () => {
    expect(friendlyError("peer cert pin mismatch (TOFU)")).toMatch(/identity changed/);
  });
  it("maps the quota, 429 and connection errors seen in E15, E8, E10", () => {
    expect(friendlyError("disk quota")).toMatch(/Not enough room/);
    expect(friendlyError("missing FreeGrant HTTP 429")).toMatch(/limiting uploads/);
    expect(friendlyError("tls io ssl_error=1 errno=0 error:0A000126:SSL routines::unexpected eof while reading")).toMatch(/verified pieces are kept/);
  });
  it("passes anything unknown through rather than guessing", () => {
    expect(friendlyError("something new")).toBe("Stopped: something new");
  });
});

describe("etaText", () => {
  const size = 2_244_022_910;
  it("says nothing until there is a real rate", () => {
    expect(etaText([], size, 0)).toBeNull();
    expect(etaText([{ t: 0, p: 0 }, { t: 2000, p: 0 }], size, 2000)).toBeNull();
  });
  it("matches the measured 20 Mbit/s run: ~2.4 MB/s, about 15 minutes", () => {
    // E5b: 2.24 GB at ~2.37 MB/s. 30 s in, 3.2% verified.
    const s = [{ t: 0, p: 0 }, { t: 30_000, p: 0.0317 }];
    expect(etaText(s, size, 30_000)).toBe("2.4 MB/s · 16 min left");
  });
  it("uses KB/s and hours on a slow line", () => {
    const s = [{ t: 0, p: 0.1 }, { t: 30_000, p: 0.1005 }];
    expect(etaText(s, size, 30_000)).toMatch(/^\d+ KB\/s · [\d.]+ h left$/);
  });
  it("ignores samples older than the window", () => {
    const s = [{ t: 0, p: 0 }, { t: 100_000, p: 0.5 }, { t: 110_000, p: 0.6 }];
    expect(etaText(s, size, 110_000)).toBe("22.4 MB/s · under a minute left");
  });
});

describe("systemPrompt", () => {
  // Replayed 2026-10-12 (lab/replay-chat.py): without the pack Granite 4.2 3B
  // said it runs on IBM's servers, called BTX a motherboard form factor and
  // argued when told it is a blockchain; with it, it gets all three right.
  const p = systemPrompt("Granite 4.2 3B", "IBM");
  it("names the model and its maker, carries the pack, and leaves no placeholder", () => {
    expect(p).toContain("Your name is Granite 4.2 3B");
    expect(p).toContain("I'm Granite 4.2 3B, an open AI model made by IBM");
    expect(p).toContain(KNOWLEDGE.split("\n")[0]);
    expect(p).not.toMatch(/\{(model|maker|knowledge)\}/);
    expect(p).not.toContain("<!--");
  });
  it("knows what BTX is, where it runs, and that it is part of a decentralized network", () => {
    expect(p).toContain("BTX is an open-source blockchain");
    expect(p).toContain("old Intel motherboard form factor");
    expect(p).toContain("this copy of you runs here, on this Mac");
    expect(p).toContain("Are you decentralized?");
  });
  it("states the network's size as a goal, not as today's count", () => {
    expect(p).toContain("The goal is thousands");
    expect(p).toContain("Few so far");
  });
  it("stays a general assistant: everyday examples in, no 'say you do not know' rule", () => {
    // Without these the 3B refused "Who wrote Hamlet?" in 6 of 12 fresh chats.
    expect(p).toContain("Q: What is the capital of Japan?");
    expect(p).toContain("Remember: you are a general assistant.");
    expect(p).not.toMatch(/say you do not know/i);
  });
  it("tells it not to argue, repeat or invent, and never says anyone earns", () => {
    expect(p).toContain("Never argue with the user");
    expect(p).toContain("Never repeat an earlier answer");
    expect(p).toContain("Never invent facts");
    expect(p).toContain("nobody pays or earns anything");
    expect(p).not.toMatch(/\u2014/);
  });
  it("carries Mende's answer rules: engineer, short or detailed, ask, honest, basics only", () => {
    expect(p).toContain("Think like a friendly engineer");
    expect(p).toContain("A simple question gets a short answer");
    expect(p).toContain("ask one short question first");
    expect(p).toContain("say honestly that you do not know it yet");
    expect(p).toContain("say kindly you keep those private; never claim you have none");
    expect(p).toContain("Never say you were told, given or trained to say something, or that the user shared it");
  });
  it("fits beside the conversation in the 8,192-token window", () => {
    // 6,363 characters measured 1,627 tokens on the 3B (3.9 a token). 7,000
    // keeps it under ~1,800; with up to three notes (~520 tokens), 12,000
    // characters of history (~3,100) and a 2,048-token answer including
    // thinking, that is ~7,500 of 8,192.
    expect(p.length).toBeLessThan(7_000);
  });
});

describe("fitHistory", () => {
  const m = (n: number) => ({ role: "user", content: "x".repeat(n) });
  it("keeps everything that fits", () => {
    const h = [m(10), m(10), m(10)];
    expect(fitHistory(h, 100)).toEqual(h);
  });
  it("drops the oldest first", () => {
    const h = [m(50), m(40), m(30)];
    expect(fitHistory(h, 69)).toEqual([h[2]]);
    expect(fitHistory(h, 70)).toEqual([h[1], h[2]]);
    expect(fitHistory(h, 120)).toEqual(h);
  });
  it("always keeps the latest message, even when it alone is too long", () => {
    const h = [m(10), m(500)];
    expect(fitHistory(h, 100)).toEqual([h[1]]);
    expect(fitHistory([], 100)).toEqual([]);
  });
});

describe("lab/replay-chat.py", () => {
  // Every claim about how a model behaves comes from replays with this tool,
  // so it must send exactly what the app sends. It once read the instruction
  // out of this file with a pattern that stopped at the first semicolon, and
  // silently dropped every rule.
  it("sends the model the same instruction as the app, to the character", () => {
    // Windows names its interpreter python (python3 is the Store stub there).
    const tool = execFileSync(process.platform === "win32" ? "python" : "python3", ["-I", "../lab/replay-chat.py", "print"], { encoding: "utf8" });
    expect(tool).toBe(systemPrompt("Granite 4.2 3B", "IBM"));
    // Starting Python took 9 s once on a Windows runner (2026-11-25); the
    // default limit is 5 s.
  }, 60_000);
});

describe("mdBlocks tables", () => {
  it("turns a Markdown table into rows of cells, ruler dropped", () => {
    const b = mdBlocks("Seven chakras:\n| # | Chakra | Colour |\n|---|:---:|---|\n| 1 | **Muladhara** | red |\n| 4 | Anahata | green |\nThat is all.");
    expect(b[0].k).toBe("line");
    expect(b[1]).toEqual({ k: "table", rows: [
      [[{ t: "text", v: "#" }], [{ t: "text", v: "Chakra" }], [{ t: "text", v: "Colour" }]],
      [[{ t: "text", v: "1" }], [{ t: "b", v: "Muladhara" }], [{ t: "text", v: "red" }]],
      [[{ t: "text", v: "4" }], [{ t: "text", v: "Anahata" }], [{ t: "text", v: "green" }]],
    ] });
    expect(b[2].k).toBe("line");
  });
  it("leaves a line with a lone pipe alone", () => {
    expect(mdBlocks("a | b")).toEqual([{ k: "line", parts: [{ t: "text", v: "a | b" }] }]);
  });
});

describe("chatErrorText", () => {
  it("says plainly when the Mac ran out of memory for the model", () => {
    for (const m of ["Insufficient Memory (00000008:kIOGPUCommandBufferCallbackErrorOutOfMemory)", "Compute error.", "failed to allocate buffer"]) {
      expect(chatErrorText(500, m)).toMatch(/^This Mac ran out of memory for the model\./);
    }
  });
  it("names a too-long conversation, and passes anything else through", () => {
    expect(chatErrorText(400, "the request exceeds the available context size")).toMatch(/too long/);
    expect(chatErrorText(500, "weird")).toBe("The model stopped: weird");
    expect(chatErrorText(502)).toBe("The model stopped (local model answered 502).");
  });
});

describe("plainMath", () => {
  it("turns the LaTeX gpt-oss writes into plain symbols", () => {
    expect(plainMath("There are \\(6 \\times 6 = 36\\) outcomes, so \\(\\frac{6}{36} = \\dfrac{1}{6}\\).")).toBe("There are 6 × 6 = 36 outcomes, so 6/36 = 1/6.");
    expect(plainMath("\\[ x \\le \\sqrt{2} \\approx 1.41 \\]")).toBe(" x ≤ √2 ≈ 1.41 ");
    expect(plainMath("\\text{area} = \\pi r^{2}")).toBe("area = π r^2");
  });
  it("leaves ordinary text and unknown commands alone", () => {
    expect(plainMath("C:\\Users\\mende and \\unknown")).toBe("C:\\Users\\mende and \\unknown");
    expect(plainMath("17 x 3 = 51")).toBe("17 x 3 = 51");
  });
  it("is applied to text lines but never inside code", () => {
    expect(mdBlocks("```\nprint('\\times')\n```")).toEqual([{ k: "code", v: "print('\\times')" }]);
    expect(mdBlocks("a \\times b")).toEqual([{ k: "line", parts: [{ t: "text", v: "a × b" }] }]);
  });
});

describe("cleanAnswer", () => {
  it("keeps what follows a stray </think>, the 3B's second and real answer", () => {
    expect(cleanAnswer("Quantum bits dance in code, </think> Quantum gates may reshape the chain.")).toBe("Quantum gates may reshape the chain.");
    expect(cleanAnswer("draft </think>\n\n  final")).toBe("final");
  });
  it("leaves a normal answer alone", () => {
    expect(cleanAnswer("51")).toBe("51");
    expect(cleanAnswer("")).toBe("");
  });
});

describe("mdBlocks", () => {
  it("splits bold, italics and code out of a line", () => {
    const [b] = mdBlocks("That is **not** a *form factor*, see `btx://x`.");
    expect(b).toEqual({ k: "line", parts: [
      { t: "text", v: "That is " }, { t: "b", v: "not" }, { t: "text", v: " a " },
      { t: "i", v: "form factor" }, { t: "text", v: ", see " }, { t: "code", v: "btx://x" }, { t: "text", v: "." },
    ] });
  });
  it("turns bullets and headings into marks, keeps other lines as typed", () => {
    const blocks = mdBlocks("## Facts\n- one\n  * two\n2 * 3 = 6\nsnake_case_name");
    expect(blocks[0]).toEqual({ k: "line", parts: [{ t: "text", v: "Facts" }], head: true });
    expect(blocks[1]).toMatchObject({ bullet: "\u2022 ", parts: [{ t: "text", v: "one" }] });
    expect(blocks[2]).toMatchObject({ bullet: "  \u2022 " });
    expect(blocks[3]).toEqual({ k: "line", parts: [{ t: "text", v: "2 * 3 = 6" }] });
    expect(blocks[4]).toEqual({ k: "line", parts: [{ t: "text", v: "snake_case_name" }] });
  });
  it("keeps fenced code verbatim, also while the fence is still open", () => {
    expect(mdBlocks("```python\nprint(**x**)\n```")).toEqual([{ k: "code", v: "print(**x**)" }]);
    expect(mdBlocks("Code:\n```\na = 1")).toEqual([
      { k: "line", parts: [{ t: "text", v: "Code:" }] }, { k: "code", v: "a = 1" },
    ]);
  });
  it("passes markup through as text, never as HTML", () => {
    const [b] = mdBlocks("<img src=x onerror=alert(1)> **<b>hi</b>**");
    expect(b).toEqual({ k: "line", parts: [
      { t: "text", v: "<img src=x onerror=alert(1)> " }, { t: "b", v: "<b>hi</b>" },
    ] });
  });
});

describe("per-model settings", () => {
  it("a model without its own settings follows Think as last set, at the defaults", () => {
    expect(prefsFor({ think: false, models: {} }, "x")).toEqual({ think: false, length: "normal", temperature: 0.6 });
    expect(prefsFor(null, "x").think).toBe(true);
    const own = { think: true, length: "short" as const, temperature: 0.2 };
    expect(prefsFor({ think: false, models: { x: own } }, "x")).toEqual(own);
  });
  it("answer length stays inside the 8,192-token window", () => {
    for (const l of ["short", "normal", "long"] as const) {
      const p = lengthPlan(l);
      // instruction ~1,800 tokens, up to three notes (~700: two of the user's
      // files at 900 characters), history at ~3.5 characters a token (3.9
      // measured on the 3B), the answer
      expect(1800 + 700 + p.historyChars / 3.5 + p.maxTokens).toBeLessThanOrEqual(8192);
    }
    expect(lengthPlan("normal").note).toBe("");
    expect(lengthPlan("short").maxTokens).toBeLessThan(lengthPlan("long").maxTokens);
  });
  it("the default temperature is one of the presets", () => {
    expect(TEMPERATURES.some((t) => t.value === 0.6)).toBe(true);
    for (const t of TEMPERATURES) expect(t.value).toBeGreaterThanOrEqual(0), expect(t.value).toBeLessThanOrEqual(1.5);
  });
});

describe("updates", () => {
  const m = (local: "none" | "downloading" | "ready") => ({ local } as unknown as Parameters<typeof updateOfferable>[0][number]);
  it("installs by itself at a quiet moment, once per check, never a .deb", () => {
    const u = { version: "0.8.2", notes: null, refused: null, by_itself: true, updated_from: null };
    expect(autoInstallNow(u, null, true, false)).toBe(true);
    expect(autoInstallNow(u, null, false, false)).toBe(false); // a chat, a download or a panel is open
    expect(autoInstallNow(u, "0.8.2", true, false)).toBe(false); // tried this round: wait for the next check
    expect(autoInstallNow(u, "0.8.1", true, false)).toBe(true); // a newer one than the last try
    expect(autoInstallNow(u, null, true, true)).toBe(false); // already installing
    expect(autoInstallNow({ ...u, by_itself: false }, null, true, false)).toBe(false); // a .deb asks for a password
    expect(autoInstallNow({ ...u, version: null }, null, true, false)).toBe(false);
    expect(autoInstallNow(null, null, true, false)).toBe(false);
  });
  it("checks at start, then every hour", () => {
    expect(UPDATE_EVERY_MS).toBe(60 * 60 * 1000);
    const t = 1_800_000_000_000;
    expect(updateDue(t, 0)).toBe(true);
    expect(updateDue(t, t - UPDATE_EVERY_MS + 1000)).toBe(false);
    expect(updateDue(t, t - UPDATE_EVERY_MS)).toBe(true);
  });
  it("offers the restart only when it cuts nothing", () => {
    expect(updateOfferable([m("ready"), m("none")], false)).toBe(true);
    expect(updateOfferable([m("ready"), m("downloading")], false)).toBe(false);
    expect(updateOfferable([m("ready")], true)).toBe(false);
  });
  it("says what the last check found, a refusal as a refusal", () => {
    expect(updateNote({ version: "0.8.1", notes: null, refused: null }, null, false, "0.8.0")).toMatch(/^AInode 0\.8\.1 is ready/);
    expect(updateNote({ version: null, notes: null, refused: null }, null, false, "0.8.0")).toBe("AInode 0.8.0 is the latest.");
    const r = updateNote({ version: null, notes: null, refused: "AInode 0.8.1 was refused: x" }, null, false, "0.8.0");
    expect(r).toMatch(/refused/);
    expect(r).not.toMatch(/latest/);
    expect(updateNote(null, "The update check failed: offline", false, "0.8.0")).toMatch(/easybtx\.com\/ai/);
    for (const s of [r, updateNote(null, null, true, "0.8.0"), updateNote(null, null, false, "0.8.0")]) expect(s).not.toMatch(/\u2014/);
  });
});

describe("send feedback", () => {
  const info = { os: "macOS 27.0.1", machine: "Mac17,2", chip: "Apple M5", ram_bytes: 24 * 1024 ** 3 };
  it("fills in the version and the system, nothing else unless asked", () => {
    const u = new URL(feedbackUrl("bug", "0.8.0", "Mac", info));
    expect(u.origin + u.pathname).toBe("https://github.com/MendeMatthias/ainode-releases/issues/new");
    expect(u.searchParams.get("template")).toBe("bug.yml");
    expect(u.searchParams.get("version")).toBe("AInode 0.8.0 for Mac");
    expect(u.searchParams.get("system")).toBe("macOS 27.0.1 · Mac17,2 · Apple M5 · 24 GB memory");
    expect([...u.searchParams.keys()].sort()).toEqual(["system", "template", "version"]);
    expect(u.toString()).not.toMatch(/\s/);
  });
  it("adds model names only when the box is ticked", () => {
    const u = new URL(feedbackUrl("model", "0.8.0", "Linux", info, ["Granite 4.2 3B", "Qwen3 8B"]));
    expect(u.searchParams.get("template")).toBe("model.yml");
    expect(u.searchParams.get("models")).toBe("Granite 4.2 3B, Qwen3 8B");
  });
  it("leaves out what it does not know", () => {
    expect(systemLine({ os: "Windows 11 (10.0.26100)", machine: "", chip: "", ram_bytes: 0 })).toBe("Windows 11 (10.0.26100)");
  });
});

describe("pollDue", () => {
  it("polls every tick while the window is visible", () => {
    expect(pollDue(false, 1_000, 999)).toBe(true);
  });
  it("polls once a minute while it is hidden, so check-ins, the engine's restart and updates go on", () => {
    expect(pollDue(true, 10_000, 0)).toBe(false);
    expect(pollDue(true, HIDDEN_POLL_MS, 0)).toBe(true);
    expect(pollDue(true, HIDDEN_POLL_MS + 5_000, HIDDEN_POLL_MS)).toBe(false);
  });
});

describe("btxNodesOf", () => {
  it("reads the BTX nodes from easybtx.com's count", () => {
    expect(btxNodesOf({ btxNodes: { live: 46, countries: 15 } })).toEqual({ live: 46, countries: 15 });
    expect(btxNodesOf({ btxNodes: { live: 3, countries: 0 } })).toEqual({ live: 3, countries: 0 });
  });
  it("shows nothing for an older count without them, a census it could not read, or no node live", () => {
    expect(btxNodesOf(null)).toBeNull();
    expect(btxNodesOf({})).toBeNull();
    expect(btxNodesOf({ btxNodes: null })).toBeNull();
    expect(btxNodesOf({ btxNodes: { live: 0, countries: 0 } })).toBeNull();
  });
  it("names one node and many", () => {
    expect(btxNodesText(1)).toBe("1 BTX node");
    expect(btxNodesText(46)).toBe("46 BTX nodes");
  });
});

describe("a BTX node on this computer (neighbour.rs)", () => {
  it("shows its height and where it was read, or nothing", () => {
    expect(nodeHereText(249862)).toBe("At block 249,862 · easyNode on this computer, read from its witness on 127.0.0.1:3081");
    expect(nodeHereText(null)).toBeNull();
    expect(nodeHereText(0)).toBeNull();
  });
  it("the graphics card note names the node only when it is there and the card is in use", () => {
    expect(gpuNoteText(true, true, "this PC")).toContain("easyNode on this computer checks BTX blocks on it too");
    expect(gpuNoteText(true, false, "this PC")).toBe("Chats run on the graphics card when AInode finds one: faster answers. Leave it free if it mines or runs a node.");
    expect(gpuNoteText(false, true, "this PC")).toBe("Chats run on this PC's processor only: slower answers, and the graphics card stays free for mining or a node.");
  });
  it("no reader-visible text has an em-dash", () => {
    for (const t of [nodeHereText(1), gpuNoteText(true, true, "x"), gpuNoteText(true, false, "x"), gpuNoteText(false, true, "x")]) expect(t?.includes(String.fromCharCode(0x2014))).toBe(false);
  });
});

describe("start with the computer (login.rs)", () => {
  it("says what happens, and what a failure means", () => {
    expect(loginNoteText(true, null, "this PC")).toBe("AInode opens by itself when this PC starts, minimized, and its engine starts with it: online without a click. Open AInode to see its window.");
    expect(loginNoteText(false, null, "this PC")).toBe("AInode opens only when you open it.");
    expect(loginNoteText(true, "could not add AInode to the programs that start with Windows (error 5)", "this PC"))
      .toBe("Could not add AInode to the programs that start with Windows (error 5). AInode works the same; it just opens when you open it.");
  });
});

describe("awayNow: when the animations rest", () => {
  it("rests while hidden or minimized, on every system", () => {
    for (const os of ["mac", "windows", "linux"]) {
      expect(awayNow(true, false, os, true)).toBe(true);
      // WebView2 keeps document.hidden false for a minimized window (found on Windows, 0.8.2).
      expect(awayNow(false, true, os, true)).toBe(true);
      expect(awayNow(false, false, os, true)).toBe(false);
    }
  });
  it("rests without the focus only on Linux", () => {
    expect(awayNow(false, false, "linux", false)).toBe(true);
    expect(awayNow(false, false, "windows", false)).toBe(false);
    expect(awayNow(false, false, "mac", false)).toBe(false);
  });
});

describe("a stuck WSL (Windows, 0.8.4)", () => {
  const w = (over: Partial<WslState> = {}): WslState => ({ restarting: false, easynode: false, node_here: false, ...over });
  it("names every distribution the restart would stop, and the BTX node", () => {
    expect(wslBarText(null)).toBeNull();
    expect(wslBarText(w())).toBeNull();
    expect(wslBarText(w({ stuck_others: ["Ubuntu"] }))).toBe("Restart Windows' Linux: this also stops Ubuntu.");
    expect(wslBarText(w({ stuck_others: ["Ubuntu", "AInode-debian13"], node_here: true })))
      .toBe("Restart Windows' Linux: this also stops Ubuntu and AInode-debian13. A BTX node runs on this PC: it stops too.");
    expect(wslBarText(w({ stuck_others: ["Ubuntu", "Debian", "Kali"] }))).toBe("Restart Windows' Linux: this also stops Ubuntu, Debian and Kali.");
    // While it restarts there is nothing to ask.
    expect(wslBarText(w({ stuck_others: ["Ubuntu"], restarting: true }))).toBeNull();
    for (const t of [wslBarText(w({ stuck_others: ["Ubuntu"], node_here: true }))!, WSL_RESTART_LINE]) expect(t).not.toMatch(/\u2014/);
  });
  it("says what AInode is doing while it restarts WSL or waits for easyNode", () => {
    const starting = (x: WslState) => statusOf(state({ engine: { state: "starting" }, wsl: x })).line;
    expect(starting(w())).toBe("Starting the engine");
    expect(starting(w({ restarting: true }))).toBe(WSL_RESTART_LINE);
    expect(starting(w({ easynode: true }))).toMatch(/^easyNode is restarting WSL/);
    // While the button asks, a retry coming does not hide why.
    const ask = state({ engine: { state: "error", error: "Windows' Linux (WSL) has lost this PC's drives." }, wsl: w({ stuck_others: ["Ubuntu"] }) });
    expect(statusOf(ask, false, null, true).line).toMatch(/^Could not start\. Windows' Linux/);
  });
  it("says what the restart started again, and what not, with the command", () => {
    const at = 1_792_600_000;
    const ms = (s: number) => (at + s) * 1000;
    const all = w({ again: { at, back: ["Ubuntu", "AInode-debian13"], not_back: [] } });
    expect(wslAgainStatus(all, ms(10))).toEqual({ tone: "ok", line: "Windows' Linux restarted. Ubuntu and AInode\u2011debian13 run again." });
    expect(wslAgainStatus(w({ again: { at, back: ["Ubuntu"], not_back: [] } }), ms(10))!.line).toBe("Windows' Linux restarted. Ubuntu runs again.");
    expect(wslAgainStatus(all, ms(WSL_AGAIN_OK_S + 1))).toBeNull();
    const some = w({ again: { at, back: ["Ubuntu"], not_back: ["Kali"] } });
    expect(wslAgainStatus(some, ms(60))).toEqual({ tone: "warn", line: "Windows' Linux restarted. Ubuntu runs again. Kali did not start again: start it with wsl -d Kali" });
    expect(wslAgainStatus(w({ again: { at, back: [], not_back: ["Ubuntu", "Kali"] } }), ms(60))!.line)
      .toBe("Windows' Linux restarted. Ubuntu and Kali did not start again: start them with wsl -d Ubuntu, wsl -d Kali");
    expect(wslAgainStatus(some, ms(WSL_AGAIN_WARN_S + 1))).toBeNull();
    expect(wslAgainStatus(w(), ms(1))).toBeNull();
    expect(wslAgainStatus(w({ again: { at, back: [], not_back: [] } }), ms(1))).toBeNull();
    // The running home line says it first, then the figures again.
    const run = (x: WslState, s: number) => statusOf(state({ wsl: x }), false, null, false, false, ms(s)).line;
    expect(run(all, 10)).toBe("Windows' Linux restarted. Ubuntu and AInode\u2011debian13 run again.");
    // The names stay whole on the line; the command to copy keeps its plain hyphen.
    expect(wslAgainStatus(w({ again: { at, back: [], not_back: ["AInode-debian13"] } }), ms(60))!.line)
      .toBe("Windows' Linux restarted. AInode\u2011debian13 did not start again: start it with wsl -d AInode-debian13");
    expect(run(all, WSL_AGAIN_OK_S + 1)).not.toMatch(/restarted/);
    for (const t of [run(all, 10), wslAgainStatus(some, ms(60))!.line]) expect(t).not.toMatch(/\u2014/);
  });
});
