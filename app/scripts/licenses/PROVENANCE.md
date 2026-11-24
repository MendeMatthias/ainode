# Where these licence texts come from

Collected on 2026-11-23. The staging scripts copy them into the engine folder
(`stage.sh` here) and stop if a file's sha256 is not the one in `SHA256SUMS`.
Upstream's Linux release archive carries no licence texts for the files AInode
takes from it (checked: `btx-0.34.16-x86_64-linux-gnu.tar.gz` has none for
BTX, OpenSSL, SQLite, libgomp, zlib or zstd), so they come from these sources.

| File | Taken from | Checked |
|---|---|---|
| `LICENSE-btx.txt` | `COPYING` in btxchain/btx at v0.34.16 (`074dac167c26c1e091198f691ed5727817dba88a`) | byte-identical to the file the Mac build writes from the same commit |
| `LICENSE-openssl.txt` | `LICENSE.txt` in `openssl-3.5.9.tar.gz` (sha256 `603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a`) | byte-identical (`cmp`). The Linux archive's OpenSSL is 3.5.5 (`strings` on its `libcrypto.so.3`); its licence was not compared byte for byte, it is Apache-2.0 as well |
| `secp256k1-COPYING.txt` | `src/secp256k1/COPYING` at that commit | `git show` |
| `leveldb-LICENSE.txt` | `src/leveldb/LICENSE` at that commit | `git show` |
| `libbitcoinpqc-LICENSE.txt` | `src/libbitcoinpqc/LICENSE` | `git show` |
| `libbitcoinpqc-dilithium-LICENSE.txt` | `src/libbitcoinpqc/dilithium/LICENSE` | `git show` |
| `libbitcoinpqc-sphincsplus-LICENSE.txt` and `-0BSD`, `-CC0-1.0`, `-LicenseRef-SPHINCS-PLUS-Public-Domain`, `-MIT-0`, `-MIT` | `src/libbitcoinpqc/sphincsplus/LICENSE` and `LICENSES/*.txt` | `git show` |
| `zlib-copyright.txt` | `usr/share/doc/zlib1g/copyright` in `ubuntu-base-24.04.5-base-amd64.tar.gz` (sha256 `e77b6f10c2590cef872b33ee9f635a0e3fd1f57fb074c0e52b5c7f56147a0c86`, the tarball `stage-engine-windows.sh` pins) | extracted with `tar` |
| `zstd-copyright.txt` | `usr/share/doc/libzstd1/copyright` in the same tarball | extracted with `tar` |
| `gcc-libgomp-copyright.txt` | `usr/share/doc/gcc-14-base/copyright` in the same tarball | extracted with `tar` |
| `GPL-3.txt` | `usr/share/common-licenses/GPL-3` in the same tarball | extracted with `tar` |

Which libraries are inside the helper was measured on the Mac helper only
(`nm`: secp256k1, LevelDB, UniValue, libbitcoinpqc, SQLite, OpenSSL; no
crc32c, minisketch, Boost or libevent symbols). Upstream's Linux helper is
built from the same source; its contents were not inspected.

The three Debian-format `copyright` files are Ubuntu's, for Ubuntu's builds of
these libraries. Upstream's archive carries Ubuntu builds of at least zstd
(`strings` on `libzstd.so.1` shows the package `libzstd 1.5.7+dfsg-3`), of a
newer Ubuntu than 24.04; the licences of the libraries are the same.
