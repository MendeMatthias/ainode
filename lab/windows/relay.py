# Inside WSL: one persistent process that relays newline-framed JSON-RPC
# requests from stdin to the helper's unix socket, one connection per request
# (the helper's framing), and writes each reply line to stdout.
import socket, sys
sock_path = sys.argv[1]
out = sys.stdout.buffer
for line in sys.stdin.buffer:
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(sock_path)
        s.sendall(line if line.endswith(b"\n") else line + b"\n")
        s.shutdown(socket.SHUT_WR)
        buf = b""
        while b"\n" not in buf:
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
        s.close()
        reply = buf.split(b"\n", 1)[0]
    except OSError as e:
        reply = ('{"result":null,"error":{"code":"RELAY","message":"%s"}}' % str(e).replace('"', "'")).encode()
    out.write(reply + b"\n")
    out.flush()
