import sys, time
for p in sys.argv[1:]:
    t = time.perf_counter(); n = 0
    with open(p, "rb", buffering=0) as f:
        while True:
            b = f.read(8 << 20)
            if not b: break
            n += len(b)
    dt = time.perf_counter() - t
    print(f"{p}: {n/2**20:.0f} MiB in {dt:.2f} s = {n/2**20/dt:.0f} MiB/s")
