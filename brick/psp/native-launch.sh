#!/bin/sh
# Card-only test build. Supply zlib to the PPSSPP core on existing Slot OS images.
set -eu
system=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export LD_LIBRARY_PATH="$system/psp-libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$system/slot-native" "$@"
