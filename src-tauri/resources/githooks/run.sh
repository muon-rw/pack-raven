#!/bin/sh
# Runs modsync.py with the first working Python; the Windows python3 Store alias fails the probe.
for py in python3 python; do
  if "$py" -c "" >/dev/null 2>&1; then
    exec "$py" "$(git rev-parse --show-toplevel)/.githooks/modsync.py" "$@"
  fi
done
echo "modsync: no Python found, mods were not synced" >&2
exit 1
