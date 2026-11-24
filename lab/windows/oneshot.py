# Inside WSL: one request on one connection, the per-call bridge.
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.connect(sys.argv[1])
s.sendall(sys.stdin.buffer.read()); s.shutdown(socket.SHUT_WR)
buf = b""
while b"\n" not in buf:
    c = s.recv(65536)
    if not c: break
    buf += c
sys.stdout.buffer.write(buf.split(b"\n", 1)[0] + b"\n")
