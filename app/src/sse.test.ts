import { describe, expect, it } from "vitest";
import { parseSse } from "./sse";

function streamOf(chunks: string[]): ReadableStream<Uint8Array> {
  const enc = new TextEncoder();
  return new ReadableStream({
    start(c) {
      for (const ch of chunks) c.enqueue(enc.encode(ch));
      c.close();
    },
  });
}

async function all(chunks: string[]): Promise<string[]> {
  const out: string[] = [];
  for await (const d of parseSse(streamOf(chunks))) out.push(d);
  return out;
}

describe("parseSse", () => {
  it("reads whole events", async () => {
    expect(await all(['data: {"a":1}\n\n', "data: [DONE]\n\n"])).toEqual(['{"a":1}', "[DONE]"]);
  });
  it("survives a chunk boundary inside a line and inside the separator", async () => {
    expect(await all(['data: {"a"', ':1}\n', '\ndata: {"b":2}\n\n'])).toEqual(['{"a":1}', '{"b":2}']);
  });
  it("handles CRLF and a final event with no trailing blank line", async () => {
    expect(await all(["data: x\r\n\r\ndata: y"])).toEqual(["x", "y"]);
  });
  it("splits a multi-byte character across chunks without corrupting it", async () => {
    const bytes = new TextEncoder().encode("data: é\n\n");
    const s = new ReadableStream<Uint8Array>({
      start(c) { c.enqueue(bytes.slice(0, 7)); c.enqueue(bytes.slice(7)); c.close(); },
    });
    const out: string[] = [];
    for await (const d of parseSse(s)) out.push(d);
    expect(out).toEqual(["é"]);
  });
  it("ignores comments and other fields", async () => {
    expect(await all([": keepalive\n\nevent: x\ndata: 1\n\n"])).toEqual(["1"]);
  });
});
