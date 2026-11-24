// The UI and the Rust side meet only at invoke(name, args). A renamed command or
// argument fails at runtime, silently, in a window nobody is looking at. This
// test reads both sides as text and fails the build instead.
//
// Tauri maps a JS argument key in camelCase to the Rust parameter in snake_case
// (totalGb -> total_gb), and the State<> parameter is injected, not sent.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const api = readFileSync(join(here, "api.ts"), "utf8");
const commandsRs = readFileSync(join(here, "../src-tauri/src/commands.rs"), "utf8");
const libRs = readFileSync(join(here, "../src-tauri/src/lib.rs"), "utf8");

const snake = (s: string) => s.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);

/** invoke calls in api.ts: name -> argument keys */
function uiCalls(): Map<string, string[]> {
  const out = new Map<string, string[]>();
  const re = /invoke(?:<[^>]*>)?\("([a-z_]+)"(?:,\s*\{([^}]*)\})?\)/g;
  for (const m of api.matchAll(re)) {
    const keys = (m[2] ?? "").split(",").map((k) => k.trim().split(":")[0].trim()).filter(Boolean);
    out.set(m[1], keys);
  }
  return out;
}

/** #[tauri::command] fns in commands.rs: name -> parameter names, minus what Tauri injects (State, AppHandle, the calling window) */
function rustCommands(): Map<string, string[]> {
  const out = new Map<string, string[]>();
  const re = /#\[tauri::command\]\s*pub async fn ([a-z_]+)\(([^)]*)\)/g;
  for (const m of commandsRs.matchAll(re)) {
    const params = m[2]
      .split(",")
      .map((p) => p.trim())
      .filter(Boolean)
      .filter((p) => !/:\s*B<|State<|AppHandle|WebviewWindow/.test(p))
      .map((p) => p.split(":")[0].trim());
    out.set(m[1], params);
  }
  return out;
}

describe("UI <-> Rust command contract", () => {
  const ui = uiCalls();
  const rs = rustCommands();

  it("found the calls on both sides", () => {
    expect(ui.size).toBeGreaterThanOrEqual(14);
    expect(rs.size).toBe(ui.size);
  });

  it("every invoke names a command that exists, with the same arguments", () => {
    for (const [name, keys] of ui) {
      expect(rs.has(name), `UI calls "${name}" but Rust has no such command`).toBe(true);
      expect(keys.map(snake).sort(), `arguments of "${name}"`).toEqual([...(rs.get(name) ?? [])].sort());
    }
  });

  it("every command is registered with the invoke handler", () => {
    for (const name of rs.keys()) {
      expect(libRs, `commands::${name} missing from generate_handler!`).toContain(`commands::${name},`);
    }
  });
});
