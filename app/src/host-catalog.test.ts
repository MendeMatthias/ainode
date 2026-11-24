// The public host must serve every model the app offers. On 2026-10-12 it
// served only the 3B, and "Get" on the 350M failed with "manifest failed".
// This test reads host/remote-setup.sh's MODELS list and the app's catalog and
// fails the build if they drift apart.
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const catalog = JSON.parse(readFileSync(join(here, "../src-tauri/resources/catalog.json"), "utf8")) as {
  models: { id: string; uri: string; files: { path: string; sha256: string }[] }[];
};
// Two scripts carry the list: the VPS one and the vast.ai container one.
const SCRIPTS = ["host/remote-setup.sh", "host/vast/vast-host.sh"];
// The host's scripts are the maintainer's and not in the public repository:
// without them these tests are skipped, with them they run as before.
const present = (script: string) => existsSync(join(here, "../..", script));

/** MODELS=( "dir|gguf|url|sha256|uri" ... ) */
function hostModels(script: string): { gguf: string; url: string; sha: string; uri: string }[] {
  if (!present(script)) return [];
  const setup = readFileSync(join(here, "../..", script), "utf8");
  const block = setup.match(/^MODELS=\(\n([\s\S]*?)\n\)/m);
  if (!block) return [];
  return [...block[1].matchAll(/"([^"]+)"/g)].map((m) => {
    const [, gguf, url, sha, uri] = m[1].split("|");
    return { gguf, url, sha, uri };
  });
}

for (const script of SCRIPTS) {
  describe.skipIf(!present(script))(`${script} serves the app's catalog`, () => {
    const host = hostModels(script);

    it("found the host's model list", () => {
      expect(host.length).toBeGreaterThanOrEqual(1);
    });

    for (const m of catalog.models) {
      it(`hosts ${m.id} with the catalog's link and hash`, () => {
        const gguf = m.files.find((f) => f.path.endsWith(".gguf"))!;
        const h = host.find((x) => x.uri === m.uri);
        expect(h, `${m.id} is in the catalog but not in ${script} MODELS`).toBeDefined();
        expect(h!.gguf).toBe(gguf.path);
        expect(h!.sha).toBe(gguf.sha256);
        expect(h!.url.startsWith("https://huggingface.co/") && h!.url.endsWith("/" + gguf.path)).toBe(true);
      });
    }
  });
}

it.skipIf(!SCRIPTS.every(present))("the two host scripts carry the same list", () => {
  expect(hostModels(SCRIPTS[1])).toEqual(hostModels(SCRIPTS[0]));
});
