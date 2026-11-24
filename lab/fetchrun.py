#!/usr/bin/env python3
"""Time one FREE_ONLY retrieve through a fetcher helper and summarise it.

    fetchrun.py <fetcher.sock> <btx://uri> [--timeout 3600] [--trace trace.jsonl]

Measures from the moment getmodel is called:
  ttfb_s      first poll where bytes_committed > 0 (first verified piece, not first byte
              on the wire: the helper only commits pieces whose SHA-384 matched)
  done_s      job status 'done'
  mbps        payload bytes / (done_s - ttfb_s), in MB/s (10^6)
plus peer_retries, the longest stall seen, and the job's final result.
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rpc import call, RpcError  # noqa: E402


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("sock")
    ap.add_argument("uri")
    ap.add_argument("--timeout", type=float, default=3600)
    ap.add_argument("--poll", type=float, default=0.25)
    ap.add_argument("--trace", default="")
    a = ap.parse_args()

    trace = open(a.trace, "w", buffering=1) if a.trace else None
    t0 = time.monotonic()
    first = call(a.sock, "getmodel", [a.uri, "FREE_ONLY"], timeout=60)
    job_id = first.get("job_id")
    if not job_id:
        print(json.dumps({"error": "no job_id", "reply": first}))
        return 2

    ttfb = None
    last = {}
    max_stall = 0
    while True:
        el = time.monotonic() - t0
        if el > a.timeout:
            print(json.dumps({"error": "timeout", "elapsed_s": round(el, 1), "last": last}))
            return 3
        try:
            jobs = call(a.sock, "getmodeljob", [], timeout=30).get("jobs", [])
        except (RpcError, OSError) as e:
            jobs = []
            if trace:
                trace.write(json.dumps({"t": round(el, 3), "rpc_error": str(e)}) + "\n")
        job = next((j for j in jobs if j.get("job_id") == job_id), None)
        if job:
            last = job
            b = job.get("bytes_committed", 0) or 0
            if ttfb is None and b > 0:
                ttfb = el
            max_stall = max(max_stall, job.get("stalled_for_ms", 0) or 0) if job.get("status") == "running" else max_stall
            if trace:
                trace.write(json.dumps({"t": round(el, 3), "bytes": b, "pieces": job.get("pieces_committed"),
                                        "inflight": job.get("inflight"), "retries": job.get("peer_retries"),
                                        "stall_ms": job.get("stalled_for_ms"), "status": job.get("status")}) + "\n")
            if job.get("status") != "running":
                break
        time.sleep(a.poll)

    done = time.monotonic() - t0
    payload = last.get("bytes_committed", 0) or 0
    span = done - (ttfb or 0)
    out = {
        "status": last.get("status"),
        "result": (last.get("result") or {}).get("status"),
        "error": last.get("error") or (last.get("result") or {}).get("error"),
        "bytes": payload,
        "ttfb_s": round(ttfb, 2) if ttfb is not None else None,
        "done_s": round(done, 2),
        "mbps": round(payload / span / 1e6, 2) if span > 0 and payload else None,
        "peer_retries": last.get("peer_retries"),
        "max_stall_ms": max_stall,
        "last_peer": last.get("last_peer"),
    }
    print(json.dumps(out))
    return 0 if out["result"] == "retrieved" else 1


if __name__ == "__main__":
    sys.exit(main())
