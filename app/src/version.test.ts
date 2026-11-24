// The version shows under the icons (main.ts, #ver) so Mende can see each
// update land. It lives in five files; a bump that misses one ships a build
// that says one number and is another (Cargo.lock never follows Cargo.toml by
// itself). Bump all five together: minor for features, patch for fixes.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const app = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p: string) => readFileSync(join(app, p), "utf8");

describe("version", () => {
  it("is the same in every file that carries it", () => {
    const pkg = JSON.parse(read("package.json")).version as string;
    const lock = JSON.parse(read("package-lock.json"));
    expect(pkg).toMatch(/^\d+\.\d+\.\d+$/);
    expect({
      "package-lock.json": lock.version,
      'package-lock.json packages[""]': lock.packages[""].version,
      "tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
      "Cargo.toml": /^version = "([^"]+)"/m.exec(read("src-tauri/Cargo.toml"))?.[1],
      "Cargo.lock": /name = "ainode"\nversion = "([^"]+)"/.exec(read("src-tauri/Cargo.lock"))?.[1],
    }).toEqual({
      "package-lock.json": pkg,
      'package-lock.json packages[""]': pkg,
      "tauri.conf.json": pkg,
      "Cargo.toml": pkg,
      "Cargo.lock": pkg,
    });
  });
});
