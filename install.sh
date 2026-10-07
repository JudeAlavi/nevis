#!/usr/bin/env bash
# Build nevis and install it for the current user:
#   ~/.local/bin/nevis, its desktop entry, and a systemd user service that keeps it running.
# Set NEVIS_DEFAULT=0 to skip making it the default app for .txt and .md files.
set -euo pipefail
cd "$(dirname "$0")"

bindir="$HOME/.local/bin"
apps="$HOME/.local/share/applications"
units="$HOME/.config/systemd/user"

# Capped parallelism: an all-core build overheats this laptop.
cargo build --release -j "${JOBS:-6}"

install -Dm755 target/release/nevis "$bindir/nevis"
mkdir -p "$apps" "$units"
first_install=true
[[ -e "$units/nevis.service" ]] && first_install=false
sed "s|@BINDIR@|$bindir|" data/dev.shirvani.Nevis.desktop.in > "$apps/dev.shirvani.Nevis.desktop"
sed "s|@BINDIR@|$bindir|" data/nevis.service.in > "$units/nevis.service"
update-desktop-database -q "$apps" || true

if [[ "${NEVIS_DEFAULT:-1}" == 1 ]]; then
    xdg-mime default dev.shirvani.Nevis.desktop text/plain text/markdown text/x-markdown
fi

systemctl --user daemon-reload
# The service is only switched on by the first install; later installs leave it as it is.
if $first_install; then
    systemctl --user enable --now nevis.service
elif systemctl --user is-active --quiet nevis.service; then
    echo "nevis is running the previous build. To switch (this closes its windows):"
    echo "    systemctl --user restart nevis.service"
fi
echo "Installed $bindir/nevis"
