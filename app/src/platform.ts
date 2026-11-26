// Which computer AInode runs on, for the words it uses and its title bar.
// The webview says it in its user agent: WebKit on macOS, WebKitGTK on Linux,
// WebView2 on Windows. Anything else (the browser preview on a Mac, tests in
// Node) reads as a Mac, the app's first platform.

export type Os = "mac" | "linux" | "windows";

export function osFrom(userAgent: string): Os {
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Linux|X11/i.test(userAgent) && !/Android/i.test(userAgent)) return "linux";
  return "mac";
}

export const OS: Os = osFrom(typeof navigator === "undefined" ? "" : navigator.userAgent ?? "");

/** "this Mac", "this computer" (Linux) or "this PC" (Windows): what every line says. */
export function deviceFor(os: Os): string {
  return os === "mac" ? "Mac" : os === "windows" ? "PC" : "computer";
}

/** What the app is called a "... app" for: Mac, Linux or Windows. */
export function platformFor(os: Os): string {
  return os === "mac" ? "Mac" : os === "windows" ? "Windows" : "Linux";
}

/**
 * Whether the chat's loading scene may play a face as a video. Not on Linux:
 * WebKitGTK plays video through GStreamer, and where a plugin it needs is
 * missing (the AppImage carries none) the page stops answering once the video
 * element exists. There the face is its still picture.
 */
export function playsVideo(os: Os): boolean {
  return os !== "linux";
}

export const DEVICE = deviceFor(OS);
export const PLATFORM = platformFor(OS);
export const THIS = `this ${DEVICE}`;

/**
 * The static text in index.html says "this Mac". On Linux and Windows,
 * rewrite it once at start, in text and in the titles and labels, so the
 * HTML stays one file for every platform.
 */
export function localizeStatic(root: ParentNode, os: Os = OS): void {
  if (os === "mac") return;
  const device = deviceFor(os);
  const fix = (s: string) => s.replace(/\bthis Mac\b/g, `this ${device}`).replace(/\bThis Mac\b/g, `This ${device}`);
  const walker = document.createTreeWalker(root as Node, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (n.nodeValue && /\b[Tt]his Mac\b/.test(n.nodeValue)) n.nodeValue = fix(n.nodeValue);
  }
  for (const el of Array.from(root.querySelectorAll("[title], [aria-label], [placeholder]"))) {
    for (const a of ["title", "aria-label", "placeholder"]) {
      const v = el.getAttribute(a);
      if (v && /\b[Tt]his Mac\b/.test(v)) el.setAttribute(a, fix(v));
    }
  }
}
