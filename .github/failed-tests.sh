#!/usr/bin/env bash
# The failed tests and errors of a `cargo test` output ($1) as one error annotation: a job's log
# needs a GitHub login, its annotations are public (`/check-runs/<job id>/annotations`).
set -u
pattern="^test .* FAILED$|panicked at|^error|process didn't exit|STATUS_|memory allocation|has overflowed"
lines=$(grep -E -A1 "$pattern" "$1" | grep -v '^--$' | head -80 \
  | sed -e 's/%/%25/g' -e 's/\r//g' | sed ':a;N;$!ba;s/\n/%0A/g')
echo "::error title=Failed tests::${lines:-no failed test found in the output}"
