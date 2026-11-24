// Reproduce the shipped app's network situation in WebKit: a page served from
// tauri://localhost (a WKURLSchemeHandler, as Tauri does on macOS) calls the
// local llama-server at http://127.0.0.1:PORT with its key. Prints what the page
// saw, so CORS and mixed-content behaviour are checked in the real engine.
//
//   swift lab/webkit-tauri-probe.swift PORT KEY
import AppKit
import WebKit

let args = CommandLine.arguments
guard args.count == 3, let port = Int(args[1]) else { print("usage: PORT KEY"); exit(2) }
let key = args[2]

let page = """
<!doctype html><html><body><script>
window.__probe = "pending";
fetch("http://127.0.0.1:\(port)/v1/chat/completions", {
  method: "POST",
  headers: { "Content-Type": "application/json", Authorization: "Bearer \(key)" },
  body: JSON.stringify({ messages: [{ role: "user", content: "Say OK." }], max_tokens: 8, stream: false })
}).then(r => r.json().then(j => { window.__probe = "status " + r.status + ": " + JSON.stringify(j.choices ? j.choices[0].message.content : j).slice(0, 80); }))
  .catch(e => { window.__probe = "FAILED: " + e; });
</script></body></html>
"""

final class TauriScheme: NSObject, WKURLSchemeHandler {
    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        let data = page.data(using: .utf8)!
        task.didReceive(HTTPURLResponse(url: task.request.url!, statusCode: 200, httpVersion: "HTTP/1.1",
                                        headerFields: ["Content-Type": "text/html"])!)
        task.didReceive(data)
        task.didFinish()
    }
    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {}
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let cfg = WKWebViewConfiguration()
cfg.setURLSchemeHandler(TauriScheme(), forURLScheme: "tauri")
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 200, height: 200), configuration: cfg)
let win = NSWindow(contentRect: web.frame, styleMask: [.borderless], backing: .buffered, defer: false)
win.contentView = web
web.load(URLRequest(url: URL(string: "tauri://localhost/index.html")!))
func poll(_ n: Int) {
    web.evaluateJavaScript("window.__probe") { r, _ in
        let s = (r as? String) ?? "nil"
        if s != "pending" && s != "nil" || n > 60 { print("tauri://localhost -> \(s)"); exit(s.hasPrefix("status 200") ? 0 : 1) }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { poll(n + 1) }
    }
}
DispatchQueue.main.asyncAfter(deadline: .now() + 1) { poll(0) }
app.run()
