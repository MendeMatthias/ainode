// Render a page in WebKit (the engine Tauri uses on macOS) offscreen and save a
// PNG, so the app's UI can be checked in its real engine without screen-capture
// permission: this captures only its own view.
//
//   swift lab/webkit-snap.swift URL OUT.png [width height delay_s dark|light [js]]
//   WEBKIT_SNAP_LIVE=1 swift lab/webkit-snap.swift ...   (a visible window)
//
// `js`, if given, runs once the page has loaded (e.g. to open a panel); the
// snapshot is taken `delay_s` after that.
//
// Offscreen, WebKit runs no animation frames: canvases stay black and anything
// driven by requestAnimationFrame never moves, so CSS animations are frozen at
// their end state for the picture. WEBKIT_SNAP_LIVE=1 shows the window on
// screen instead (borderless, for the length of the run) and lets everything
// animate, for canvas scenes like the chat's loading scene.
import AppKit
import WebKit

let args = CommandLine.arguments
guard args.count >= 3, let url = URL(string: args[1]) else {
    FileHandle.standardError.write("usage: webkit-snap URL OUT.png [w h delay dark|light]\n".data(using: .utf8)!)
    exit(2)
}
let out = URL(fileURLWithPath: args[2])
let w = args.count > 3 ? Double(args[3]) ?? 460 : 460
let h = args.count > 4 ? Double(args[4]) ?? 760 : 760
let delay = args.count > 5 ? Double(args[5]) ?? 2.5 : 2.5
let dark = args.count > 6 ? args[6] != "light" : true
let script = args.count > 7 ? args[7] : ""
let live = ProcessInfo.processInfo.environment["WEBKIT_SNAP_LIVE"] == "1"

final class Snapper: NSObject, WKNavigationDelegate {
    let web: WKWebView
    let window: NSWindow
    init(frame: NSRect, dark: Bool) {
        let config = WKWebViewConfiguration()
        // Collect script errors so a broken page is reported, not just pictured.
        let probe = "window.__errs=[];addEventListener('error',e=>__errs.push(e.message+' @'+e.filename+':'+e.lineno));addEventListener('unhandledrejection',e=>__errs.push('rejection: '+(e.reason&&e.reason.stack||e.reason)));"
        config.userContentController.addUserScript(WKUserScript(source: probe, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        // An offscreen view never advances animations; freeze them at their end
        // state so the picture shows what a person sees after they finish.
        let still = "addEventListener('DOMContentLoaded',()=>{const s=document.createElement('style');s.textContent='*,*::before,*::after{animation-duration:0s!important;animation-delay:0s!important;transition:none!important}';document.head.appendChild(s)});"
        if !live { config.userContentController.addUserScript(WKUserScript(source: still, injectionTime: .atDocumentStart, forMainFrameOnly: true)) }
        web = WKWebView(frame: frame, configuration: config)
        window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
        super.init()
        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        window.contentView = web
        web.navigationDelegate = self
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        // Give the app a moment to boot, then run the optional script.
        DispatchQueue.main.asyncAfter(deadline: .now() + (script.isEmpty ? 0 : 1.5)) {
            if !script.isEmpty { webView.evaluateJavaScript(script) { _, _ in } }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + delay + (script.isEmpty ? 0 : 1.5)) {
          webView.evaluateJavaScript("JSON.stringify({errors: window.__errs, consentHidden: document.getElementById('consent') && document.getElementById('consent').hidden, consentOpacity: getComputedStyle(document.getElementById('consent')).opacity, anims: document.getAnimations().map(a=>a.playState+':'+(a.currentTime|0))})") { result, _ in
            if let r = result as? String { print("page: \(r)") }
            let cfg = WKSnapshotConfiguration()
            webView.takeSnapshot(with: cfg) { image, error in
                guard let image, let tiff = image.tiffRepresentation,
                      let rep = NSBitmapImageRep(data: tiff),
                      let png = rep.representation(using: .png, properties: [:]) else {
                    FileHandle.standardError.write("snapshot failed: \(String(describing: error))\n".data(using: .utf8)!)
                    exit(1)
                }
                try? png.write(to: out)
                print(out.path)
                exit(0)
            }
          }
        }
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        FileHandle.standardError.write("load failed: \(error)\n".data(using: .utf8)!); exit(1)
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        FileHandle.standardError.write("load failed: \(error)\n".data(using: .utf8)!); exit(1)
    }
}

let app = NSApplication.shared
app.setActivationPolicy(live ? .accessory : .prohibited)
let snapper = Snapper(frame: NSRect(x: 0, y: 0, width: w, height: h), dark: dark)
if live { snapper.window.orderFrontRegardless() }
snapper.web.load(URLRequest(url: url))
DispatchQueue.main.asyncAfter(deadline: .now() + 30) { FileHandle.standardError.write("timeout\n".data(using: .utf8)!); exit(3) }
app.run()
