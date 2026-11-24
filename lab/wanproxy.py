#!/usr/bin/env python3
"""A userspace WAN emulator for one TCP hop, no root needed.

Sits between a fetcher and a host:  fetcher -> 127.0.0.1:LISTEN -> proxy -> HOST

    wanproxy.py --listen 127.0.0.1:39447 --to 127.0.0.1:29447 \
        --rtt-ms 130 --mbit 20 --log conns.jsonl

What it emulates, per direction, per connection:
  - one-way delay of rtt/2 on every chunk (a FIFO delay line, so ordering and
    throughput under latency behave like a long pipe, not like sleep-per-read)
  - a token-bucket bandwidth cap shared by ALL connections in that direction
    (a home line is one pipe, however many sockets the client opens)
  - optional chaos: --drop-after-mb N cuts a connection after N MiB,
    --blackhole-at S --blackhole-for D silently stalls every byte for D seconds
    starting S seconds after the proxy starts (a dead route, not a reset)

It logs one JSON line per connection (open/close, bytes each way, duration) so a
run can be judged by connection churn as well as throughput.

Not emulated: packet loss and reordering (TCP hides those as latency anyway),
MTU and middlebox behaviour. Those need a real network, which is Phase 0's job.
"""
from __future__ import annotations

import argparse
import asyncio
import json
import time

T0 = time.monotonic()


class Bucket:
    """Token bucket in bytes/s, shared across connections.

    The bucket may go into debt: a chunk larger than the burst is sent, and the
    sender then waits until the debt is repaid. An earlier version waited for
    the whole chunk to fit inside a 50 ms burst, which can never happen once a
    chunk exceeds it (12.5 KB at 2 Mbit/s), so slow links deadlocked in the
    proxy and looked like a BTX failure. See docs/experiments.md, E5c.
    """

    def __init__(self, rate_bps: float):
        self.rate = rate_bps
        self.burst = rate_bps * 0.05
        self.tokens = self.burst
        self.last = time.monotonic()
        self.lock = asyncio.Lock()

    async def take(self, n: int) -> None:
        if self.rate <= 0:
            return
        async with self.lock:
            now = time.monotonic()
            self.tokens = min(self.burst, self.tokens + (now - self.last) * self.rate)
            self.last = now
            self.tokens -= n
            if self.tokens < 0:
                await asyncio.sleep(-self.tokens / self.rate)


class Chaos:
    def __init__(self, args):
        self.drop_after = int(args.drop_after_mb * 1024 * 1024) if args.drop_after_mb else 0
        self.bh_at = args.blackhole_at
        self.bh_for = args.blackhole_for

    async def maybe_blackhole(self) -> None:
        if self.bh_for <= 0:
            return
        t = time.monotonic() - T0
        if self.bh_at <= t < self.bh_at + self.bh_for:
            await asyncio.sleep(self.bh_at + self.bh_for - t)


async def pump(reader, writer, delay_s, bucket, chaos, counter, key, conn_total):
    """Read, then deliver each chunk delay_s later, in order, through the bucket."""
    q: asyncio.Queue = asyncio.Queue()

    async def deliver():
        while True:
            due, data = await q.get()
            if data is None:
                break
            wait = due - time.monotonic()
            if wait > 0:
                await asyncio.sleep(wait)
            for off in range(0, len(data), 16384):
                part = data[off:off + 16384]
                await chaos.maybe_blackhole()
                await bucket.take(len(part))
                writer.write(part)
                await writer.drain()
        try:
            writer.write_eof()
        except (OSError, RuntimeError):
            pass

    d = asyncio.create_task(deliver())
    try:
        while True:
            data = await reader.read(65536)
            if not data:
                break
            counter[key] += len(data)
            if chaos.drop_after and conn_total() > chaos.drop_after:
                counter["dropped"] = True
                break
            await q.put((time.monotonic() + delay_s, data))
    except (ConnectionError, OSError):
        pass
    await q.put((0, None))
    try:
        await d
    except (ConnectionError, OSError):
        pass


async def handle(cr, cw, args, up_bucket, down_bucket, chaos, log, seq):
    n = next(seq)
    opened = time.monotonic()
    host, port = args.to.rsplit(":", 1)
    host = host.strip("[]")  # "[::1]:29447" -> "::1"
    stats = {"up": 0, "down": 0, "dropped": False}
    try:
        hr, hw = await asyncio.open_connection(host, int(port))
    except OSError as e:
        log({"conn": n, "event": "upstream_fail", "err": str(e)})
        cw.close()
        return
    delay = args.rtt_ms / 2000.0
    total = lambda: stats["up"] + stats["down"]
    await asyncio.gather(
        pump(cr, hw, delay, up_bucket, chaos, stats, "up", total),
        pump(hr, cw, delay, down_bucket, chaos, stats, "down", total),
    )
    for w in (cw, hw):
        try:
            w.close()
        except OSError:
            pass
    log({"conn": n, "t_open": round(opened - T0, 3), "dur_s": round(time.monotonic() - opened, 3),
         "up": stats["up"], "down": stats["down"], "dropped": stats["dropped"]})


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--listen", default="127.0.0.1:39447")
    p.add_argument("--to", default="127.0.0.1:29447")
    p.add_argument("--rtt-ms", type=float, default=130)
    p.add_argument("--mbit", type=float, default=20, help="per direction; 0 = unlimited")
    p.add_argument("--up-mbit", type=float, default=None, help="client->host cap if different")
    p.add_argument("--drop-after-mb", type=float, default=0)
    p.add_argument("--blackhole-at", type=float, default=0)
    p.add_argument("--blackhole-for", type=float, default=0)
    p.add_argument("--log", default="")
    args = p.parse_args()

    fh = open(args.log, "a") if args.log else None

    def log(obj):
        line = json.dumps(obj)
        if fh:
            fh.write(line + "\n")
            fh.flush()

    down = Bucket(args.mbit * 125000)
    up = Bucket((args.up_mbit if args.up_mbit is not None else args.mbit) * 125000)
    chaos = Chaos(args)
    seq = iter(range(1, 1 << 30))
    host, port = args.listen.rsplit(":", 1)
    host = host.strip("[]")
    server = await asyncio.start_server(
        lambda r, w: handle(r, w, args, up, down, chaos, log, seq), host, int(port))
    log({"event": "listening", "listen": args.listen, "to": args.to, "rtt_ms": args.rtt_ms,
         "mbit": args.mbit})
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
