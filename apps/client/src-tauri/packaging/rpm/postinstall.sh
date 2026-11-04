#!/bin/sh
set -eu

source_key="/usr/lib/ganbaru/package-repo/ganbaru-package-repo.asc"
public_key="/etc/pki/rpm-gpg/RPM-GPG-KEY-ganbaru"
dnf_repo_file="/etc/yum.repos.d/ganbaru.repo"
zypper_repo_file="/etc/zypp/repos.d/ganbaru.repo"
repo_url="https://opengrimoire.github.io/ganbaru/packages/rpm"

# $1 is 1 on a first install and 2 or more on an upgrade or reinstall. Repository files are
# removed only on a full erase, so a missing one during an upgrade means the user removed it.
install_count="${1:-1}"

# Prints the enabled value to write: 0 when the existing repository file disables it.
enabled_value() {
  repo_file="$1"
  if [ -f "$repo_file" ] && grep -Eiq '^[[:space:]]*enabled[[:space:]]*=[[:space:]]*(0|no|false|off)[[:space:]]*$' "$repo_file"; then
    echo "0"
  else
    echo "1"
  fi
}

write_repo_file() {
  repo_file="$1"
  if [ "$install_count" != "1" ] && [ ! -e "$repo_file" ]; then
    return 0
  fi
  enabled="$(enabled_value "$repo_file")"
  mkdir -p "$(dirname "$repo_file")"
  cat > "$repo_file" <<EOF
[ganbaru]
name=Ganbaru
baseurl=$repo_url
enabled=$enabled
type=rpm-md
autorefresh=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=file://$public_key
metadata_expire=6h
EOF
}

if [ -r "$source_key" ]; then
  install -D -m 0644 "$source_key" "$public_key"
  write_repo_file "$dnf_repo_file"
  if [ -d /etc/zypp ]; then
    write_repo_file "$zypper_repo_file"
  fi
fi

exit 0
