#!/usr/bin/env bash
# Run Cargo normally, but expose compiler output through the Checks API when a
# restricted CI client cannot download the GitHub Actions log archive.
set -o pipefail

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

"$@" 2>&1 | tee "$log"
status=${PIPESTATUS[0]}

if [[ $status -ne 0 && ${GITHUB_ACTIONS:-} == true ]]; then
    message="$(tail -c 50000 "$log")"
    message=${message//'%'/'%25'}
    message=${message//$'\r'/'%0D'}
    message=${message//$'\n'/'%0A'}
    printf '::error title=Rust build failed::%s\n' "$message"
fi

exit "$status"
