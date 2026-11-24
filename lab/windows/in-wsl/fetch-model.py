# Inside the probe distro: start the engine on MODELDIR, fetch URI (unless the
# store already has it), export it, report sizes and times. Optionally just
# export (no peer) to time exportmodelpath on a copied store.
import hashlib, json, os, socket, subprocess, sys, time
E = os.environ.get("AINODE_ENGINE", "/mnt/c/Repos/ainode/wslprobe/engine")  # a copy of a staged Linux engine on the Windows drive
md, sock, uri, quota = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
peer = sys.argv[5] if len(sys.argv) > 5 else None
def rpc(m, p=[], t=900):
    s = socket.socket(socket.AF_UNIX); s.settimeout(t); s.connect(sock)
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
args = [f"{E}/btx-modeld", f"-modeldir={md}", f"-modelstorage={quota}GiB", "-modelseed=manual", "-modelfollowpeers=0", f"-modelrpcsocket={sock}"]
if peer: args.append(f"-modelpeer={peer}")
p = subprocess.Popen(args, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
t0 = time.time()
while True:
    try: rpc("hello", t=2); break
    except OSError: time.sleep(0.1)
    if time.time() - t0 > 30: sys.exit("no start")
try:
    m = [x for x in rpc("listmodels")["models"] if x["uri"] == uri]
    if not (m and m[0].get("complete")):
        t = time.time(); job = rpc("getmodel", [uri, "FREE_ONLY"], 120); print("getmodel", job.get("status"), flush=True)
        last = 0
        while True:
            m = [x for x in rpc("listmodels")["models"] if x["uri"] == uri]
            if m and m[0].get("complete"): break
            if time.time() - last > 60:
                last = time.time(); print(f"  {time.time()-t:6.0f}s ...", flush=True)
            time.sleep(2)
        dt = time.time() - t; print(f"fetched {m[0]['bytes']} bytes in {dt:.0f} s = {m[0]['bytes']/dt/1e6:.1f} MB/s", flush=True)
    t = time.time(); d = rpc("exportmodelpath", [uri])["path"]; dt = time.time() - t
    g = max((os.path.join(d, f) for f in os.listdir(d)), key=os.path.getsize)
    print(f"exportmodelpath {os.path.getsize(g)} bytes in {dt:.1f} s = {os.path.getsize(g)/dt/1e6:.0f} MB/s -> {g}", flush=True)
finally:
    try: rpc("stop", t=10)
    except Exception: pass
    p.wait(20)
