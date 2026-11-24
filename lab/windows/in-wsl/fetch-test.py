# Inside the probe distro: start the engine with a given modeldir, fetch the
# 350M from the public host, export it, check its sha256. Times every step.
import hashlib, json, os, socket, subprocess, sys, time
E = os.environ.get("AINODE_ENGINE", "/mnt/c/Repos/ainode/wslprobe/engine")  # a copy of a staged Linux engine on the Windows drive
md, sock = sys.argv[1], sys.argv[2]
URI = "btx://pq33nsv5hxj5mgrs4r4256h80g0y6n7uqhkv0j56cs3nrfq2c9jt686pm507apg2ygtvhd7uc43maj58gc8hu"
SHA = "771c588a49607f274a2bba3185733607ebe6f74b996ab90e2d6bee0d98bcec52"
def rpc(m, p=[]):
    s = socket.socket(socket.AF_UNIX); s.connect(sock)
    s.sendall((json.dumps({"jsonrpc": "1.0", "id": 1, "method": m, "params": p}) + "\n").encode()); s.shutdown(socket.SHUT_WR)
    b = b""
    while b"\n" not in b:
        c = s.recv(1 << 16)
        if not c: break
        b += c
    r = json.loads(b.split(b"\n")[0])
    if r.get("error"): raise RuntimeError(f"{m}: {r['error']}")
    return r["result"]
os.makedirs(os.path.dirname(sock), exist_ok=True); os.chmod(os.path.dirname(sock), 0o700)
env = {"HOME": "/root", "PATH": "/usr/bin:/bin", "LD_LIBRARY_PATH": f"{E}/lib", "BTX_OPENSSL": f"{E}/openssl.sh"}
t0 = time.time()
p = subprocess.Popen([f"{E}/btx-modeld", f"-modeldir={md}", "-modelstorage=2GiB", "-modelseed=manual", "-modelfollowpeers=0",
                      f"-modelrpcsocket={sock}", "-modelpeer=20.196.8.98:29447"], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
while True:
    try: rpc("hello"); break
    except OSError: time.sleep(0.1)
    if time.time() - t0 > 30: sys.exit("no start")
print(f"{md}: engine up in {time.time()-t0:.1f} s")
try:
    t = time.time(); job = rpc("getmodel", [URI, "FREE_ONLY"]); print("getmodel:", {k: job.get(k) for k in ("job_id", "status")})
    while True:
        m = [x for x in rpc("listmodels")["models"] if x["uri"] == URI]
        if m and m[0].get("complete"): break
        if time.time() - t > 600: raise RuntimeError("fetch over 10 min")
        time.sleep(1)
    dt = time.time() - t; print(f"fetched 236997118 bytes in {dt:.1f} s ({236997118/dt/1e6:.1f} MB/s)")
    t = time.time(); ex = rpc("exportmodelpath", [URI]); d = ex["path"]; print(f"exportmodelpath in {time.time()-t:.2f} s -> {d}")
    g = [os.path.join(d, f) for f in os.listdir(d) if f.endswith(".gguf")][0]
    h = hashlib.sha256(open(g, "rb").read()).hexdigest()
    print("sha256", "MATCHES" if h == SHA else f"DIFFERS {h}")
finally:
    try: rpc("stop")
    except Exception: pass
    p.wait(15)
