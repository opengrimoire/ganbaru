#!/bin/sh
set -eu

# Removes the device linking firewall rules the app added. Upgrades keep them.
if [ "${1:-}" = "0" ] && [ -x /usr/lib/ganbaru/ganbaru-linking-firewall ]; then
  /usr/lib/ganbaru/ganbaru-linking-firewall --manage-linking-firewall revoke-all || true
fi

exit 0
