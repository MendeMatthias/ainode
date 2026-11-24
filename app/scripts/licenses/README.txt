Licence texts for the engine files AInode ships
===============================================

LICENSE-btx.txt, LICENSE-openssl.txt (beside the helper)
  BTX's btx-modeld (MIT) and the OpenSSL it uses for its post-quantum TLS
  (Apache-2.0). On the Mac OpenSSL 3.5.9 is linked into the helper; on Linux
  and Windows upstream's release ships it as lib/libssl.so.3 and
  lib/libcrypto.so.3 (OpenSSL 3.5.5). The licence is the same.

In this folder, on every platform: libraries from BTX's own source tree that
are compiled into btx-modeld
  secp256k1-COPYING.txt            secp256k1 (MIT)
  leveldb-LICENSE.txt              LevelDB (BSD-3-Clause)
  libbitcoinpqc-LICENSE.txt        libbitcoinpqc (MIT), with the code it carries:
  libbitcoinpqc-dilithium-LICENSE.txt     the Dilithium reference code
  libbitcoinpqc-sphincsplus-*.txt         the SPHINCS+ reference code; its
                                          LICENSE names the others
  UniValue has no licence file of its own in that tree; it falls under
  LICENSE-btx.txt. SQLite, which the helper also uses, is in the public
  domain (https://sqlite.org/copyright.html) and has no licence text.

In this folder, on Linux and Windows only: libraries upstream's Linux release
ships beside the helper, which AInode passes on unchanged in lib/
  zlib-copyright.txt               libz.so.1 (zlib licence)
  zstd-copyright.txt               libzstd.so.1 (BSD-3-Clause, or GPL-2)
  gcc-libgomp-copyright.txt        libgomp.so.1, the GNU OpenMP runtime
                                   (GPL-3 or later with the GCC Runtime
                                   Library Exception, whose text is in the file)
  GPL-3.txt                        the licence that file refers to
  lib/libsqlite3.so.0 is SQLite: public domain, as above.

Where each text was taken from, and its sha256, is in
app/scripts/licenses/PROVENANCE.md in AInode's source.
