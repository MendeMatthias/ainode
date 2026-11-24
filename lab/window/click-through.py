"""Click through the REAL AInode window, end to end, with screenshots.

Drives the installed app through tauri-driver (WebDriver into the app's own
webview: WebKitGTK on Linux), against the real engine and the public host:

  consent -> Start -> Store -> Get the 3B -> Chat (loading scene, an answer)
  -> Clear -> a dropped PDF answered from -> the settings sheet -> Network
  -> Status (Check for updates, Send feedback) -> kept chats -> Remove

  python3 lab/window/click-through.py /usr/bin/ainode SHOTS_DIR [--pdf FILE]

tauri-driver must listen on 127.0.0.1:4444 (see lab/window/README.md), or where
TAURI_DRIVER_URL says, so two of these can run at once (one per platform).
Every step prints how long it took; a failure saves SHOTS_DIR/fail.png.
"""

import argparse
import json
import os
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from webdriver import Driver, WebDriverError  # noqa: E402

# Windows consoles default to cp1252; a model's answer can hold any character.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

CATALOG = pathlib.Path(__file__).resolve().parents[2] / "app/src-tauri/resources/catalog.json"
MODEL = "granite-4.2-3b-q4km"

ap = argparse.ArgumentParser()
ap.add_argument("app")
ap.add_argument("shots")
ap.add_argument("--pdf", help="a text PDF to add to the chat")
ap.add_argument("--size", default="25", help="the consent preset, in GB")
ap.add_argument("--where", help="a drive root to pick on the first screen (Change under Where), e.g. 'D:\\\\'; the app relaunches")
args = ap.parse_args()
shots = pathlib.Path(args.shots)
shots.mkdir(parents=True, exist_ok=True)
uri = next(m["uri"] for m in json.loads(CATALOG.read_text())["models"] if m["id"] == MODEL)
card = f'[data-uri="{uri}"]'

d = Driver(os.environ.get("TAURI_DRIVER_URL", "http://127.0.0.1:4444"))
t0 = time.time()
n = 0


def step(name):
    global n
    n += 1
    print(f"[{time.time() - t0:7.1f}s] {n:02d} {name}", flush=True)


def shot(name):
    d.screenshot(str(shots / f"{n:02d}-{name}.png"))


def visible(sel):
    # Not offsetParent: it is null for the position: fixed sheets.
    return f"const e=document.querySelector({json.dumps(sel)}); return !!e && !e.closest('[hidden]') && e.getClientRects().length > 0"


def text_of(sel):
    return d.js(f"const e=document.querySelector({json.dumps(sel)}); return e ? e.innerText : ''")


def ask(q, timeout=300):
    """Type, wait until the app lets it be sent (the model is loaded), send, and
    wait for the reply to finish: the send button is "Send" again, not "Stop",
    and the reply has its actions row."""
    before = d.js("return document.querySelectorAll('#thread .msg.bot').length")
    d.type("#prompt", q)
    d.wait("return !document.getElementById('send').disabled", 180, 1, what="the model to be ready")
    d.click("#send")
    d.wait(f"return document.querySelectorAll('#thread .msg.bot').length > {before}", 30, what="the reply to start")
    return d.wait("const s=document.getElementById('send'); const b=[...document.querySelectorAll('#thread .msg.bot')].pop();"
                  " return s.getAttribute('aria-label') === 'Send' && b && b.querySelector('.msg-actions') ? b.innerText : ''",
                  timeout, 1, what=f"the answer to {q!r}")


try:
    d.start(args.app)
    step("first launch: the space AInode may use")
    d.wait(visible("#consent"), 30, what="the consent screen")
    time.sleep(1.5)
    print("   os:", d.js("return document.documentElement.dataset.os"), "|", text_of("#consentLede"))
    shot("consent")
    if args.where:
        print("   where:", text_of("#wherePath"))
        d.click("#whereChange")
        d.wait(visible("#places button"), 10, what="the drives")
        time.sleep(0.5)
        shot("places")
        d.click(f'#places button b:is(:not(:empty))' if False else f"#places button[data-dir^={json.dumps(args.where)}]")
        time.sleep(0.5)
        print("   where now:", text_of("#wherePath"), "|", text_of("#consentFine"))
    d.click(f'#choices button[data-g="{args.size}"]')
    time.sleep(0.5)
    print("   ", text_of("#consentFine"))
    d.click("#consentGo")
    if args.where:
        # The app relaunches to read its paths again: the session is gone with
        # it, and the relaunched app (a real one, outside WebDriver) is up and
        # would turn a second instance away. Close it and start our own.
        time.sleep(8)
        try:
            d.quit()
        except Exception:
            pass
        import subprocess
        exe = pathlib.Path(args.app).name
        if sys.platform == "win32":
            subprocess.run(["taskkill", "/IM", exe], capture_output=True)
            for _ in range(60):
                if exe.lower() not in subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {exe}"], capture_output=True, text=True).stdout.lower():
                    break
                time.sleep(0.5)
            else:
                subprocess.run(["taskkill", "/F", "/IM", exe], capture_output=True)
        else:
            subprocess.run(["pkill", "-x", exe.removesuffix(".exe")], capture_output=True)
            time.sleep(5)
        d = Driver(os.environ.get("TAURI_DRIVER_URL", "http://127.0.0.1:4444"))
        d.start(args.app)
        print("   relaunched by the app, closed, opened again by the test; consent shown:", d.js("return !document.getElementById('consent').hidden"))

    # A test install is never counted among the AInodes online: "count me" off at once (since
    # 0.8.4 the app checks in from Rust, 30 s after launch, whether or not the window runs).
    d.js("window.__TAURI_INTERNALS__.invoke('set_pref', {name: 'count_me', on: false})")

    step("Start: the engine comes up and finds the host")
    d.wait("return document.getElementById('core').dataset.state === 'running'", 90, what="the engine to run")
    time.sleep(3)
    print("   status:", text_of("#statusText"))
    shot("running")

    step("Store")
    d.click("#storeBtn")
    d.wait(visible(f"#storeList button.act{card}"), 20, what="the 3B in the store")
    time.sleep(1)
    shot("store")

    step("Get the 3B (2.2 GB over BTX from the public host)")
    d.click(f'#storeList button.act[data-do="get"]{card}')
    d.wait(f"return !!document.querySelector('#storeList button.act[data-do=\"progress\"]{card}')", 60, what="the download to start")
    time.sleep(3)
    shot("downloading")
    d.click("#storeBack")
    last = ""
    end = time.time() + 1800
    while not d.js(f"return !!document.querySelector('#models button.act[data-do=\"chat\"]{card}')"):
        p = d.js(f"const b=document.querySelector('#models button.act{card}'); return b ? b.innerText : ''")
        if p != last and p:
            print(f"    {time.time() - t0:7.1f}s {p.strip()}", flush=True)
            last = p
        if time.time() > end:
            raise WebDriverError("the 3B did not finish downloading in 30 minutes")
        time.sleep(5)
    time.sleep(1)
    print("   meter:", text_of("#meterText"))
    shot("ready-on-home")

    step("Chat: the loading scene, then an answer from the model on this computer")
    d.click(f'#models button.act[data-do="chat"]{card}')
    d.wait(visible("#chat"), 10)
    time.sleep(1.2)
    shot("loading-scene")
    d.wait("return /ready/i.test(document.getElementById('thread').innerText)", 180, 1, what="the model to load")
    time.sleep(2.5)
    shot("model-ready")
    a = ask("What is BTX, in one sentence?")
    print("   answer:", a.replace("\n", " ")[:300])
    shot("answer")

    step("Clear (two clicks): conversation and memory gone, the scene again")
    d.click("#chatClear")
    time.sleep(0.4)
    d.click("#chatClear")
    d.wait("return document.querySelectorAll('#thread .msg.user').length === 0", 15, what="the thread to clear")
    time.sleep(2)
    shot("cleared")

    if args.pdf:
        step("Your own knowledge: a PDF added with +")
        d.js("const i=document.getElementById('attachInput'); i.hidden=false; i.style.display='block';")
        d.type("#attachInput", str(pathlib.Path(args.pdf).resolve()))
        d.js("const i=document.getElementById('attachInput'); i.hidden=true; i.style.display='';")
        added = d.wait("const n=[...document.querySelectorAll('#thread .msg.sys')].pop(); return n && /Added|scan|could not/i.test(n.innerText) ? n.innerText : ''", 60, what="the file to be read")
        print("   ", added.replace("\n", " ")[:200])
        a = ask("What is the access code in my file?")
        print("   answer:", a.replace("\n", " ")[:300])
        shot("answer-from-pdf")

    step("The model's settings sheet")
    d.click("#chatBack")
    d.click(f'#models button[data-do="settings"]{card}')
    d.wait(visible("#modelSheet"), 10)
    time.sleep(0.8)
    shot("settings-sheet")
    d.click("#modelSheetBack")

    step("Network: bounties, campaigns, links, search")
    d.click("#networkBtn")
    d.wait(visible("#network"), 10)
    d.wait("return document.getElementById('netBounties').innerText.trim().length > 0", 60, what="the bounties to load")
    time.sleep(1)
    shot("network")
    d.click("#networkBack")

    step("Status, then keep chats on and leave the graphics card free")
    d.click("#aboutBtn")
    d.wait(visible("#about"), 10)
    time.sleep(0.8)
    print("   ", text_of("#kv").replace("\n", " | ")[:400])
    shot("status")
    print("   version:", text_of("#ver"))
    # The live feed: a release no newer than this one says "is the latest".
    d.click("#updateCheck")
    note = d.wait("const t=document.getElementById('updateNote').innerText; return /Looking for/.test(t) ? '' : t", 60, 1, what="the update check")
    print("   updates:", note)
    # tauri-plugin-updater looks up this platform's file before it compares versions, so a feed
    # with no entry for it fails the check even when nothing newer is out (Mac-only feeds).
    if "were found in the response" in note:
        print("   (the live feed has no entry for this platform yet: not up to date, not an error of this build)")
    elif " is the latest." not in note:  # logic.ts updateNote: nothing newer on offer
        raise WebDriverError(f"the update check did not say up to date: {note!r}")
    d.js("document.getElementById('updateNote').scrollIntoView({block: 'center'})")
    time.sleep(0.6)
    shot("status-updates")
    # Send feedback: the link the app hands to the browser, caught on its way out. Tauri 2's
    # invoke is read-only, but its IPC is a fetch to the ipc protocol with the command's body.
    d.js("window.__opened=[]; const f=window.fetch; window.fetch=function(u,o){ try { if (/open_url/.test(String((u && u.url) || u)))"
         " window.__opened.push(JSON.parse(o.body).url); } catch (e) {} return f.apply(this, arguments); }")
    d.js("document.getElementById('feedbackRow').scrollIntoView({block: 'center'})")
    d.click('#feedbackRow button[data-fb="bug"]')
    url = d.wait("return (window.__opened||[])[0] || ''", 20, what="the feedback link")
    print("   feedback:", url)
    if not ("/MendeMatthias/ainode-releases/issues/new?" in url and "template=bug.yml" in url and "system=" in url):
        raise WebDriverError(f"the feedback link is not a filled-in issue: {url!r}")
    time.sleep(1.5)
    print("   toast:", d.js("const t=document.getElementById('toast'); return t && !t.hidden ? t.innerText : ''"))
    shot("status-feedback")
    d.click('#chatsSeg button[data-k="on"]')
    time.sleep(0.8)
    # The next chat starts llama-server again, on the processor only.
    d.click('#gpuSeg button[data-gpu="off"]')
    d.wait("return document.querySelector('#gpuSeg button[data-gpu=\"off\"]').getAttribute('aria-pressed') === 'true'", 10, what="the graphics card left free")
    time.sleep(0.8)
    print("   graphics card:", text_of("#gpuNote"))
    shot("status-keep-chats")
    d.click("#aboutBack")

    step("A kept chat, listed under History")
    d.click(f'#models button.act[data-do="chat"]{card}')
    d.wait(visible("#chat"), 10)
    a = ask("Name one prime number.")
    print("   answer:", a.replace("\n", " ")[:120])
    d.click("#chatClear")
    time.sleep(0.4)
    d.click("#chatClear")
    time.sleep(1)
    d.click("#chatHistory")
    d.wait("return document.getElementById('historyList').innerText.trim().length > 0", 15, what="a kept chat")
    time.sleep(0.6)
    shot("history")
    d.click("#historyClose")
    d.click("#chatBack")

    step("Remove: the copy goes, the space comes back")
    before = text_of("#meterText")
    d.click(f'#models button[data-do="remove"]{card}')
    time.sleep(0.4)
    d.click(f'#models button[data-do="remove"]{card}')
    d.wait(f"return !document.querySelector('#models button.act[data-do=\"chat\"]{card}')", 60, what="the model to go")
    time.sleep(6)
    after = text_of("#meterText")
    print(f"   meter: {before!r} -> {after!r}")
    shot("removed")
    print(f"PASS in {time.time() - t0:.0f}s, screenshots in {shots}")
except Exception as e:
    print(f"FAIL at step {n}: {e}", flush=True)
    try:
        d.screenshot(str(shots / "fail.png"))
    except Exception:
        pass
    sys.exit(1)
finally:
    d.quit()
