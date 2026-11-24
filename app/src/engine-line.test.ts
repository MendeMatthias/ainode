// What Status says about the bundled engine ("Network"). 0.9.0's first build
// said "BTX v0.34.15, unmodified" there, from a line written by hand in
// main.ts, while the engine was v0.34.16 and built by AInode. The line now
// comes from the engine folder's manifest (Rust engine.rs `bundled`), which
// the staging scripts write from the tag they pin (a Rust test holds them to
// it). These tests hold the interface to it.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { engineLine } from "./logic";

const app = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p: string) => readFileSync(join(app, p), "utf8");

describe("the engine line in Status", () => {
  it("says the version and where the helper comes from, as the manifest has it", () => {
    expect(engineLine({ state: "running", bundled: { version: "v1.2.3", built_by_ainode: true } }))
      .toBe("BTX v1.2.3 engine, built by AInode from upstream's source");
    expect(engineLine({ state: "stopped", bundled: { version: "v1.2.3", built_by_ainode: false } }))
      .toBe("BTX v1.2.3 engine, upstream's file unmodified");
  });

  it("names no version when the engine folder does not say one", () => {
    expect(engineLine({ state: "running" })).toBe("BTX engine");
  });

  it("is the only place the interface speaks of the engine's version", () => {
    // No release number of the engine written by hand in what the user sees:
    // the screens (main.ts and what it draws with) and the page itself.
    for (const f of ["src/main.ts", "src/logic.ts", "src/chip.ts", "src/network.ts", "src/chat.ts", "index.html"]) {
      const text = read(f).split("\n").filter((l) => !/^\s*(\/\/|\*|\/\*)/.test(l)).join("\n");
      expect(text.match(/BTX v\d+\.\d+(\.\d+)?/g) ?? [], f).toEqual([]);
      expect(text.match(/\bv?0\.3\d\.\d+/g) ?? [], f).toEqual([]);
    }
    expect(read("src/main.ts")).toContain("[CHIP_NAME, engineLine(s.engine)]");
  });

  it("the three staging scripts pin one release, and the mock shows none that exists", () => {
    const pin = (f: string) => /^BTX_TAG=(v\d+\.\d+\.\d+)$/m.exec(read(f))?.[1];
    expect(pin("scripts/stage-engine.sh")).toBeTruthy();
    expect(pin("scripts/stage-engine-linux.sh")).toBe(pin("scripts/stage-engine.sh"));
    expect(read("src/api.ts")).toContain('MOCK_BUNDLED = { version: "v9.9.9"');
  });
});
