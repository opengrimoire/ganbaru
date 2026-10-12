#!/bin/sh
set -eu

source_key="/usr/lib/ganbaru-ai/package-repo/ganbaru-ai-package-repo.asc"
public_key="/etc/pki/rpm-gpg/RPM-GPG-KEY-ganbaru-ai"
dnf_repo_file="/etc/yum.repos.d/ganbaru-ai.repo"
zypper_repo_file="/etc/zypp/repos.d/ganbaru-ai.repo"
repo_url="https://opengrimoire.github.io/ganbaru-ai/packages/rpm"

write_repo_file() {
  repo_file="$1"
  mkdir -p "$(dirname "$repo_file")"
  cat > "$repo_file" <<EOF
[ganbaru-ai]
name=Ganbaru AI
baseurl=$repo_url
enabled=1
type=rpm-md
autorefresh=1
gpgcheck=0
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
