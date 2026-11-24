# Sourced by stage-engine.sh, stage-engine-linux.sh: the licence texts of
# what the engine folder ships, copied from this folder into $STAGE.
#
#   stage_licenses toplevel   LICENSE-btx.txt and LICENSE-openssl.txt beside the helper
#   stage_licenses common     licenses/: libraries compiled into btx-modeld
#   stage_licenses linux-libs licenses/: libraries upstream's Linux archive ships
# (one call may name several groups)
#
# Needs from the caller: $APP_DIR, $STAGE, sha256(), record(), die().
# Every text is checked against SHA256SUMS here first. Where each comes from:
# PROVENANCE.md.
LICENSES_DIR="$APP_DIR/scripts/licenses"

license_src() {  # license_src NAME -> prints the checked source path
  local name=$1 want got
  want=$(awk -v n="$name" '$2 == n {print $1}' "$LICENSES_DIR/SHA256SUMS")
  [ -n "$want" ] || die "licenses/SHA256SUMS does not name $name"
  [ -f "$LICENSES_DIR/$name" ] || die "licence text missing: scripts/licenses/$name"
  got=$(sha256 "$LICENSES_DIR/$name")
  [ "$got" = "$want" ] || die "scripts/licenses/$name is not the text SHA256SUMS names (got $got)"
  printf '%s\n' "$LICENSES_DIR/$name"
}

stage_licenses() {
  local group name src dest how names
  for group in "$@"; do
    case $group in
      toplevel) dest=""; names=(LICENSE-btx.txt LICENSE-openssl.txt) ;;
      common) dest="licenses/"
        names=(README.txt secp256k1-COPYING.txt leveldb-LICENSE.txt libbitcoinpqc-LICENSE.txt
               libbitcoinpqc-dilithium-LICENSE.txt libbitcoinpqc-sphincsplus-LICENSE.txt
               libbitcoinpqc-sphincsplus-0BSD.txt libbitcoinpqc-sphincsplus-CC0-1.0.txt
               libbitcoinpqc-sphincsplus-LicenseRef-SPHINCS-PLUS-Public-Domain.txt
               libbitcoinpqc-sphincsplus-MIT-0.txt libbitcoinpqc-sphincsplus-MIT.txt) ;;
      linux-libs) dest="licenses/"; names=(zlib-copyright.txt zstd-copyright.txt gcc-libgomp-copyright.txt GPL-3.txt) ;;
      *) die "stage_licenses: unknown group $group" ;;
    esac
    [ -z "$dest" ] || mkdir -p "$STAGE/licenses"
    for name in "${names[@]}"; do
      src=$(license_src "$name") || exit 1
      cp "$src" "$STAGE/$dest$name"
      chmod 644 "$STAGE/$dest$name"
      case $name in
        LICENSE-btx.txt) how="COPYING from upstream's tree at the pinned commit (MIT)" ;;
        LICENSE-openssl.txt) how="OpenSSL's LICENSE.txt (Apache-2.0), for the OpenSSL the helper uses" ;;
        README.txt) how="written for AInode: what the texts in this folder are for" ;;
        *-copyright.txt|GPL-3.txt) how="licence text of a library upstream's Linux archive ships in lib/" ;;
        *) how="licence text of a library from BTX's source tree that is compiled into btx-modeld" ;;
      esac
      record "$dest$name" "" "" "$how; source and check: app/scripts/licenses/PROVENANCE.md"
    done
  done
}
