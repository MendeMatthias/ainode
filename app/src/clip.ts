// Put a text on the clipboard: a model's btx:// link, a bounty's id, a command
// for a node owner. The async clipboard first; where a webview does not offer
// it (WebKitGTK on some Linux desktops), the old selection way.
export async function copyText(text: string): Promise<void> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return;
    }
  } catch {
    // Fall through to the selection way.
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.setAttribute("readonly", "");
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.appendChild(ta);
  ta.select();
  const ok = document.execCommand("copy");
  ta.remove();
  if (!ok) throw new Error("Could not copy. Select the text and copy it by hand.");
}
