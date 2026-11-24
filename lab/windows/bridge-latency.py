# Windows side: the cost of one JSON-RPC call to the helper inside WSL,
# (1) a wsl.exe per call, (2) one persistent wsl.exe relay.
import json, subprocess, statistics, sys, time
WSL = r"C:\Windows\System32\wsl.exe"
DISTRO, USER, SOCK = sys.argv[1], sys.argv[2], sys.argv[3]
req = json.dumps({"jsonrpc": "1.0", "id": 1, "method": "hello", "params": []}).encode() + b"\n"
def ms(xs): return f"median {statistics.median(xs):.1f} ms, p90 {sorted(xs)[int(len(xs)*0.9)-1]:.1f} ms, n={len(xs)}"
# (1) per call
per = []
for _ in range(20):
    t = time.perf_counter()
    r = subprocess.run([WSL, "-d", DISTRO, "-u", USER, "--cd", "/", "--", "python3", "/mnt/c/Repos/ainode/ainode/lab/windows/oneshot.py", SOCK], input=req, capture_output=True)
    per.append((time.perf_counter() - t) * 1000)
    assert b'"result"' in r.stdout, r
print("per-call wsl.exe + python3:", ms(per))
# (2) persistent relay
p = subprocess.Popen([WSL, "-d", DISTRO, "-u", USER, "--cd", "/", "--", "python3", "-u", "/mnt/c/Repos/ainode/ainode/lab/windows/relay.py", SOCK], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
t = time.perf_counter(); p.stdin.write(req); p.stdin.flush(); first = p.stdout.readline(); start = (time.perf_counter() - t) * 1000
rel = []
for method in ["hello"] * 200 + ["getmodelnetworkinfo"] * 100 + ["listmodels"] * 100:
    q = json.dumps({"jsonrpc": "1.0", "id": 1, "method": method, "params": []}).encode() + b"\n"
    t = time.perf_counter(); p.stdin.write(q); p.stdin.flush(); line = p.stdout.readline(); rel.append((time.perf_counter() - t) * 1000)
    assert b'"result"' in line, line
p.stdin.close(); p.wait(5)
print(f"persistent relay: first call (starts wsl.exe) {start:.0f} ms; then", ms(rel))
print("reply sample:", first[:120])
