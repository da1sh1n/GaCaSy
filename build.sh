#!/usr/bin/env bash
# Builds and signs all three Romzeta programs.
#
# One command for the whole thing. It does two jobs `cargo build` cannot:
#
#   1. Makes sure a signing key exists. listener/build.rs refuses to compile a
#      listener with no trust anchor, because such a listener would reject every
#      cartridge in existence.
#
#   2. Delegates to `xtask release`, which runs the four stages in the one order
#      that works — build, sign, build the installer around the signed binaries,
#      sign the installer. See SIGNING.md.
#
# Everything lands in target/release/.
#
#   ./build.sh              build and sign
#   ./build.sh --clean      remove target/ first
#   ./build.sh --no-keygen  fail instead of generating a dev key (for CI)
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

clean=0
no_keygen=0
for arg in "$@"; do
    case "$arg" in
        --clean)     clean=1 ;;
        --no-keygen) no_keygen=1 ;;
        -h|--help)   sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *)           echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

step() {
    echo ">> $1"
    shift
    "$@"
}

# A listener is built to trust keys/romzeta.pub and keys/dev.pub, and needs at
# least one of them to exist. A fresh clone has neither: romzeta.pub arrives only
# with a published release, and dev.pub is gitignored because it is yours.
anchors=()
for key in keys/romzeta.pub keys/dev.pub; do
    [ -f "$key" ] && anchors+=("$key")
done

if [ ${#anchors[@]} -eq 0 ]; then
    if [ "$no_keygen" -eq 1 ]; then
        echo "No trust anchor in keys/ and --no-keygen was given." >&2
        echo "Run \`cargo run -p xtask -- keygen\` first." >&2
        exit 1
    fi
    echo "No signing key yet — generating a dev key (once per machine)."
    step 'keygen' cargo run -p xtask -- keygen
else
    echo "Trust anchors: ${anchors[*]}"
fi

if [ "$clean" -eq 1 ]; then
    step 'cargo clean' cargo clean
fi

# Everything below here — including the build order — lives in xtask/src/release.rs.
step 'building and signing launcher, listener, installer' cargo run -p xtask -- release

echo
echo "Done. Signed binaries are in target/release/."
