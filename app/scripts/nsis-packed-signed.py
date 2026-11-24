"""Is the ainode.exe inside an NSIS installer signed by a given certificate?

  python -I nsis-packed-signed.py <setup.exe> <ainode.exe> <cert.der>

Tauri signs ainode.exe, packs it, then puts the unsigned build back in
target\\release (seen 2026-10-19: rewritten 42 ms after the installer), so the
file to check is the packed copy. This reads the installer's solid LZMA
payload (nothing is run or installed), finds the packed copy by the build's
code (the bytes after the first page, which signing never changes), and
looks for the certificate (DER, as Windows reports the installer's signer)
in the copy's Authenticode certificate table. Exit 0: signed by it.
Standard library only (release-windows.ps1 runs it).
"""
import lzma
import struct
import sys

setup, built, cert_path = sys.argv[1:4]
data = open(setup, 'rb').read()
at = data.find(b'\xef\xbe\xad\xdeNullsoftInst')
if at < 4:
    sys.exit(f'{setup}: not an NSIS installer')
stream = data[at - 4 + 28:]
props = stream[0]
dict_size, = struct.unpack_from('<I', stream, 1)
lzma1 = {"id": lzma.FILTER_LZMA1, "dict_size": dict_size, "lc": props % 9, "lp": (props // 9) % 5, "pb": props // 45}
try:
    blob = lzma.LZMADecompressor(format=lzma.FORMAT_RAW, filters=[lzma1]).decompress(stream[5:], max_length=1 << 30)
except lzma.LZMAError as e:
    sys.exit(f'{setup}: the payload is not solid LZMA ({e})')

code = open(built, 'rb').read()[4096:8192]
k = blob.find(code)
if len(code) < 4096 or k < 4096:
    sys.exit(f'{built}: its packed copy is not in {setup}')
pe0 = k - 4096
if blob[pe0:pe0 + 2] != b'MZ':
    sys.exit('the packed copy has no PE header')
pe = pe0 + struct.unpack_from('<I', blob, pe0 + 0x3c)[0]
magic, = struct.unpack_from('<H', blob, pe + 24)
security = pe + 24 + (112 if magic == 0x20b else 96) + 4 * 8
off, size = struct.unpack_from('<II', blob, security)
if not size:
    sys.exit('the packed ainode.exe is NOT signed')
table = blob[pe0 + off: pe0 + off + size]
if open(cert_path, 'rb').read() not in table:
    sys.exit("the packed ainode.exe is signed, but not by the installer's certificate")
print(f'the packed ainode.exe is signed by the same certificate ({size}-byte certificate table)')
