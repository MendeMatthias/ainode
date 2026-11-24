// Server-sent events reader for llama.cpp's streaming endpoint.
// Yields each event's `data:` payload. Tolerates chunks that split a line or
// an event anywhere, CRLF line ends, and multi-line data fields.

export async function* parseSse(body: ReadableStream<Uint8Array>): AsyncGenerator<string> {
  const reader = body.getReader();
  const dec = new TextDecoder();
  let buf = "";
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      yield* drain();
    }
    buf += dec.decode();
    buf += "\n\n";
    yield* drain();
  } finally {
    reader.releaseLock();
  }

  function* drain(): Generator<string> {
    buf = buf.replace(/\r\n/g, "\n");
    let i: number;
    while ((i = buf.indexOf("\n\n")) >= 0) {
      const event = buf.slice(0, i);
      buf = buf.slice(i + 2);
      const data = event
        .split("\n")
        .filter((l) => l.startsWith("data:"))
        .map((l) => l.slice(5).replace(/^ /, ""))
        .join("\n");
      if (data) yield data;
    }
  }
}
