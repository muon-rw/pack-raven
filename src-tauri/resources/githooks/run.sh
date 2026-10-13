#!/bin/sh
# packraven records its own path in each clone's git config, since it differs per machine.
packraven=$(git config --get packraven.path)
if [ -z "$packraven" ] || [ ! -f "$packraven" ]; then
  echo "modsync: this clone isn't linked to packraven, so mods are not synced; open the pack in packraven first" >&2
  exit 1
fi
exec "$packraven" modsync "$@"
