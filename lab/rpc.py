#!/usr/bin/env python3
"""Minimal client for btx-modeld's unix JSON-RPC socket.

Same framing as upstream's contrib/modelnet/btx-model: one JSON object per
connection, newline terminated, write side shut, read until the first newline.
Kept deliberately tiny so lab scripts depend on nothing but the standard library.

    rpc.py <socket> <method> [json-param ...]
    rpc.py /tmp/x.sock getmodel '"btx://..."' '"FREE_ONLY"'
"""
from __future__ import annotations

import json
import os
import socket
import sys
from typing import Any


class RpcError(Exception):
    def __init__(self, method: str, error: Any):
        super().__init__(f"{method}: {json.dumps(error)}")
        self.method = method
        self.error = error


def call(sock: str, method: str, params: list[Any] | None = None, timeout: float = 30.0) -> Any:
    payload = {"jsonrpc": "1.0", "id": 1, "method": method, "params": params or []}
    line = (json.dumps(payload, separators=(",", ":")) + "\n").encode()
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    try:
        s.connect(sock)
        s.sendall(line)
        try:
            s.shutdown(socket.SHUT_WR)
        except OSError:
            pass
        data = b""
        while b"\n" not in data:
            chunk = s.recv(65536)
            if not chunk:
                break
            data += chunk
    finally:
        s.close()
    if not data:
        raise RpcError(method, "empty reply")
    reply = json.loads(data.decode())
    if reply.get("error") not in (None, {}, []):
        raise RpcError(method, reply["error"])
    return reply.get("result")


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    params = [json.loads(p) for p in sys.argv[3:]]
    try:
        # hostmodel on a slow box hashes for minutes; the helper allows 24 h.
        timeout = float(os.environ.get("RPC_TIMEOUT", "120"))
        print(json.dumps(call(sys.argv[1], sys.argv[2], params, timeout=timeout), indent=2))
    except RpcError as e:
        print(f"error: {e}", file=sys.stderr)
        sys.exit(1)
