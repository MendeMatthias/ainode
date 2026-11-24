// A PDF's text, page by page, read in the webview with pdf.js (Mozilla,
// Apache-2.0), bundled: nothing is fetched, the file never leaves the Mac. The
// legacy build, because the app supports macOS 13 (Safari 16). Loaded only
// when a PDF is dropped, so the chat does not carry it otherwise.
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";

/** At most this many pages are read (a long book); the rest are left out and said so. */
export const MAX_PAGES = 3000;

export async function pdfPages(data: ArrayBuffer): Promise<{ pages: string[]; total: number }> {
  const pdfjs = await import("pdfjs-dist/legacy/build/pdf.mjs");
  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
  // Text only: no font faces, no XFA. pdf.js 6 has no eval path at all (the
  // app's content policy forbids eval anyway).
  const task = pdfjs.getDocument({ data: new Uint8Array(data), disableFontFace: true, useSystemFonts: false, enableXfa: false, stopAtErrors: false });
  const doc = await task.promise;
  try {
    const pages: string[] = [];
    const n = Math.min(doc.numPages, MAX_PAGES);
    for (let i = 1; i <= n; i++) {
      const page = await doc.getPage(i);
      const content = await page.getTextContent();
      // Items carry their own line ends (hasEOL); a gap in height starts a paragraph.
      let text = "";
      let lastY: number | null = null;
      for (const it of content.items) {
        if (!("str" in it)) continue;
        const y = it.transform[5];
        if (lastY !== null && Math.abs(y - lastY) > (it.height || 10) * 1.8) text += "\n\n";
        text += it.str + (it.hasEOL ? "\n" : "");
        lastY = y;
      }
      pages.push(text);
      page.cleanup();
    }
    return { pages, total: doc.numPages };
  } finally {
    await task.destroy();
  }
}
