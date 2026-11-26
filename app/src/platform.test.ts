import { execFileSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import { systemPrompt } from "./logic";
import { deviceFor, osFrom, platformFor, playsVideo } from "./platform";

// User agents the three webviews send (Tauri 2): WKWebView, WebKitGTK 2.4x,
// WebView2.
const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const LINUX = "Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

describe("platform", () => {
  it("reads the webview's platform from its user agent", () => {
    expect(osFrom(MAC)).toBe("mac");
    expect(osFrom(LINUX)).toBe("linux");
    expect(osFrom(WINDOWS)).toBe("windows");
    // Node (tests) and anything unknown read as a Mac, the first platform.
    expect(osFrom("Node.js/22")).toBe("mac");
    expect(osFrom("")).toBe("mac");
    expect(osFrom("Mozilla/5.0 (Linux; Android 14)")).toBe("mac");
  });
  it("names the computer the way each platform does", () => {
    expect([deviceFor("mac"), deviceFor("linux"), deviceFor("windows")]).toEqual(["Mac", "computer", "PC"]);
    expect([platformFor("mac"), platformFor("linux"), platformFor("windows")]).toEqual(["Mac", "Linux", "Windows"]);
  });
  it("plays the chat scene's face as a video everywhere but on Linux", () => {
    // WebKitGTK without the GStreamer plugin it needs (the AppImage) loses the
    // page to a <video>; Linux shows the still.
    expect([playsVideo("mac"), playsVideo("windows"), playsVideo("linux")]).toEqual([true, true, false]);
  });
});

describe("the instruction on each platform", () => {
  const mac = systemPrompt("Granite 4.2 3B", "IBM", "mac");
  const linux = systemPrompt("Granite 4.2 3B", "IBM", "linux");
  const windows = systemPrompt("Granite 4.2 3B", "IBM", "windows");
  it("leaves no placeholder behind", () => {
    for (const p of [mac, linux, windows]) expect(p).not.toMatch(/\{(device|platform|model|maker|knowledge)\}/);
  });
  it("is the measured Mac text on a Mac", () => {
    expect(mac).toContain("running in AInode on the user's Mac.");
    expect(mac).toContain("AInode is a Mac app, in preview");
    expect(systemPrompt("Granite 4.2 3B", "IBM")).toBe(mac); // Node reads as a Mac
  });
  it("says computer and PC elsewhere, never that it runs on a Mac", () => {
    expect(linux).toContain("running in AInode on the user's computer.");
    expect(linux).toContain("AInode is a Linux app, in preview");
    expect(windows).toContain("running on this PC in AInode");
    expect(windows).toContain("AInode is a Windows app, in preview");
    for (const p of [linux, windows]) expect(p).not.toMatch(/(this|your|user's) Mac\b/);
  });
  it("fits the window on every platform", () => {
    // The Mac text keeps logic.test.ts's 7,000 (about 1,800 tokens). "computer"
    // is five letters longer than "Mac" in nine places: 7,036 characters on
    // Linux, about 9 tokens more, inside the ~690 tokens the 8,192 window
    // keeps spare in logic.test.ts's worst case.
    expect(mac.length).toBeLessThan(7_000);
    for (const p of [linux, windows]) expect(p.length).toBeLessThan(7_100);
  });
  it("is what lab/replay-chat.py sends for that platform", () => {
    const tool = (os: string) =>
      execFileSync(process.platform === "win32" ? "python" : "python3", ["-I", "../lab/replay-chat.py", "print"], { encoding: "utf8", env: { ...process.env, AINODE_OS: os } });
    expect(tool("linux")).toBe(linux);
    expect(tool("windows")).toBe(windows);
    // Two Python starts: see logic.test.ts for why this has a longer limit.
  }, 60_000);
});
