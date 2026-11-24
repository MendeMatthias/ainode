# Windows: time llama-server.exe from start to /health 200 for a gguf path,
# then one short completion, then stop. CPU build only (never the GPU here).
import json, socket, subprocess, sys, time, urllib.request
exe, gguf = sys.argv[1], sys.argv[2]
extra = sys.argv[3:]
s = socket.socket(); s.bind(("127.0.0.1", 0)); port = s.getsockname()[1]; s.close()
t0 = time.perf_counter()
p = subprocess.Popen([exe, "-m", gguf, "--host", "127.0.0.1", "--port", str(port), "-c", "2048", "--no-webui", *extra],
                     stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
ok = False
while time.perf_counter() - t0 < 300:
    if p.poll() is not None: break
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=2) as r:
            if r.status == 200: ok = True; break
    except Exception: pass
    time.sleep(0.1)
load = time.perf_counter() - t0
if ok:
    body = json.dumps({"messages": [{"role": "user", "content": "Say hello in one word."}], "max_tokens": 8, "temperature": 0}).encode()
    t = time.perf_counter()
    with urllib.request.urlopen(urllib.request.Request(f"http://127.0.0.1:{port}/v1/chat/completions", body, {"Content-Type": "application/json"}), timeout=120) as r:
        ans = json.load(r)["choices"][0]["message"]["content"]
    print(f"{gguf}\n  ready in {load:.2f} s, first answer {time.perf_counter()-t:.2f} s: {ans!r} {' '.join(extra)}")
else:
    print(f"{gguf}\n  FAILED after {load:.1f} s: {p.stderr.read()[-600:]!r}")
p.terminate(); p.wait(10)
