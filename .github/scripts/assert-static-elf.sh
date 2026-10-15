#!/usr/bin/env bash
# Fails unless every one of the seven Linux release programs in <dir> is statically linked: no
# program interpreter (PT_INTERP) and no shared library it needs (DT_NEEDED). The Linux archives
# are built for musl so that they run on any distribution, Alpine included, with no particular
# libc; a dynamic build would install fine on the glibc CI runners and then fail on the user's
# machine. Rust's musl output is usually static-pie, which `file` calls "static-pie linked", not
# "statically linked", so this reads the ELF headers with readelf instead.
#
#   bash .github/scripts/assert-static-elf.sh rust/target/x86_64-unknown-linux-musl/release
set -euo pipefail

dir="${1:?usage: assert-static-elf.sh <directory holding the built programs>}"
if ! command -v readelf >/dev/null 2>&1; then
  echo "::error::readelf was not found (it comes with binutils)"
  exit 1
fi

status=0
for program in jev-claude jev-statusline jev-explain jev-legend jev-check jev-update-check jev-proxy-host; do
  file="$dir/$program"
  if [ ! -f "$file" ]; then
    echo "::error::$file does not exist"
    status=1
    continue
  fi
  # Captured first: `readelf | grep -q` under pipefail can fail on SIGPIPE when grep stops early.
  headers="$(readelf -lW "$file")"
  dynamic="$(readelf -dW "$file")"
  if grep -q 'Requesting program interpreter' <<<"$headers" || grep -q '(NEEDED)' <<<"$dynamic"; then
    echo "::error::$program is dynamically linked:"
    grep -E 'Requesting program interpreter|\(NEEDED\)' <<<"$headers$dynamic" || true
    status=1
  else
    echo "static: $program"
  fi
done
exit "$status"
