"""A minimal W3C WebDriver client (stdlib only) for driving the REAL AInode
window through tauri-driver: WebKitWebDriver on Linux, msedgedriver on
Windows. Enough for find, click, type, run a script and take a screenshot.
"""

import base64
import http.client
import json
import time
import urllib.error
import urllib.request

ELEMENT = "element-6066-11e4-a52e-4f735466cecf"


class WebDriverError(Exception):
    pass


class Driver:
    def __init__(self, url="http://127.0.0.1:4444"):
        self.url = url.rstrip("/")
        self.sid = None

    def _req(self, method, path, body=None, timeout=60):
        # WebKitWebDriver now and then drops a connection right after the
        # model has kept every core busy (observed once on a screenshot).
        # Read-only requests are asked again; a click or typing never is.
        safe = method == "GET" or path.endswith(("/elements", "/execute/sync"))
        for attempt in range(3 if safe else 1):
            try:
                return self._req_once(method, path, body, timeout)
            except (ConnectionError, http.client.RemoteDisconnected, urllib.error.URLError) as e:
                if attempt == 2 or not safe or isinstance(e, urllib.error.HTTPError):
                    raise
                time.sleep(1)

    def _req_once(self, method, path, body=None, timeout=60):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.url + path, data=data, method=method, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                return json.loads(r.read() or b"{}").get("value")
        except urllib.error.HTTPError as e:
            raw = e.read()
            try:
                v = json.loads(raw).get("value", {})
                raise WebDriverError(f"{method} {path}: {v.get('error')}: {v.get('message')}") from None
            except json.JSONDecodeError:
                raise WebDriverError(f"{method} {path}: HTTP {e.code} {raw[:200]!r}") from None

    def start(self, application, args=None, timeout=120):
        opts = {"application": application}
        if args:
            opts["args"] = args
        caps = {"capabilities": {"alwaysMatch": {"tauri:options": opts}}}
        v = self._req("POST", "/session", caps, timeout=timeout)
        self.sid = v["sessionId"]
        return v

    def quit(self):
        if self.sid:
            try:
                self._req("DELETE", f"/session/{self.sid}", timeout=30)
            finally:
                self.sid = None

    def s(self, path):
        return f"/session/{self.sid}{path}"

    # ── elements ────────────────────────────────────────────────────────────
    def find(self, css, timeout=10.0, visible=True):
        """The first element matching `css` (and displayed, by default), waiting up to `timeout`."""
        end = time.time() + timeout
        last = None
        while True:
            try:
                els = self._req("POST", self.s("/elements"), {"using": "css selector", "value": css})
                for e in els or []:
                    # W3C's key; older drivers answer {"ELEMENT": id}.
                    eid = e.get(ELEMENT) or e.get("ELEMENT") or next(iter(e.values()))
                    if not visible or self._req("GET", self.s(f"/element/{eid}/displayed")):
                        return eid
            except WebDriverError as e:
                last = e
            if time.time() > end:
                raise WebDriverError(f"no {'visible ' if visible else ''}element {css!r} after {timeout}s ({last})")
            time.sleep(0.25)

    def click(self, css, timeout=10.0):
        eid = self.find(css, timeout)
        self._req("POST", self.s(f"/element/{eid}/click"), {})
        return eid

    def type(self, css, text, timeout=10.0):
        eid = self.find(css, timeout)
        self._req("POST", self.s(f"/element/{eid}/value"), {"text": text})
        return eid

    def text(self, css, timeout=10.0):
        eid = self.find(css, timeout)
        return self._req("GET", self.s(f"/element/{eid}/text"))

    def js(self, script, *args):
        return self._req("POST", self.s("/execute/sync"), {"script": script, "args": list(args)})

    def wait(self, script, timeout=30.0, every=0.5, what=None):
        """Run `script` (a JS expression body returning a value) until it is truthy."""
        end = time.time() + timeout
        while True:
            v = self.js(script)
            if v:
                return v
            if time.time() > end:
                raise WebDriverError(f"timed out after {timeout}s waiting for {what or script}")
            time.sleep(every)

    def screenshot(self, path):
        png = base64.b64decode(self._req("GET", self.s("/screenshot")))
        with open(path, "wb") as f:
            f.write(png)
        return path
