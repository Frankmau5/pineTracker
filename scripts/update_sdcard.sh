#!/usr/bin/env bash
# pineTracker v2.0 — Fast 2-Second SD Card Updater (/dev/sda2)
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

distrobox-host-exec gdbus call --system --dest org.freedesktop.UDisks2 \
    --object-path /org/freedesktop/UDisks2/block_devices/sda \
    --method org.freedesktop.UDisks2.Block.Rescan '{}' >/dev/null 2>&1 || true
sleep 1

SD_MNT="/run/media/deck/8d55e7a4-61cc-446f-b84b-fceb7dacd139"
if [[ ! -d "$SD_MNT/opt/pinetracker" ]]; then
    distrobox-host-exec udisksctl mount -b /dev/sda2 >/dev/null 2>&1 || true
    sleep 1
fi

cp -f target/aarch64-unknown-linux-gnu/release/pine_tracker "$SD_MNT/opt/pinetracker/pine_tracker"
chmod +x "$SD_MNT/opt/pinetracker/pine_tracker"
cp -f data/projects/cyber_pine_demo.toml "$SD_MNT/projects/cyber_pine_demo.toml"
cp -f data/projects/cyber_pine_demo.toml "$SD_MNT/opt/pinetracker/data/projects/cyber_pine_demo.toml"
cp -f deploy/stage1-arch/pinetracker-init.sh "$SD_MNT/usr/local/bin/pinetracker-init"
chmod +x "$SD_MNT/usr/local/bin/pinetracker-init"
cp -f deploy/stage1-arch/pinetracker-rt-tune.sh "$SD_MNT/usr/local/bin/pinetracker-rt-tune.sh"
chmod +x "$SD_MNT/usr/local/bin/pinetracker-rt-tune.sh"

sync
distrobox-host-exec udisksctl unmount -b /dev/sda2
echo "==> SD Card (/dev/sda2) updated and cleanly unmounted!"
