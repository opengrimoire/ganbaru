#!/bin/sh
set -eu

if [ "${1:-}" = "0" ]; then
  rm -f /etc/yum.repos.d/ganbaru.repo
  rm -f /etc/zypp/repos.d/ganbaru.repo
  rm -f /etc/pki/rpm-gpg/RPM-GPG-KEY-ganbaru
fi

exit 0
