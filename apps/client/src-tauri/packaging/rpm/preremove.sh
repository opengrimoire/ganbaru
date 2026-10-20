#!/bin/sh
set -eu

# Removes the device linking firewall rules the app added. Upgrades keep them.
if [ "${1:-}" = "0" ] && [ -x /usr/bin/ganbaru ]; then
  /usr/bin/ganbaru --manage-linking-firewall revoke-all || true
fi

exit 0
