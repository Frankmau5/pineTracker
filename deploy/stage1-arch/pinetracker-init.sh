#!/bin/sh
# pineTracker v2.0 — Ultra-Fast PID 1 Appliance Init for Pine64 PinePhone (Allwinner A64 + KB151)
# Bypasses systemd/udev/NetworkManager/modem bloat to boot directly into pineTracker in ~2-3s
# and power off cleanly in <0.5s.

export PATH=/usr/local/sbin:/usr/local/bin:/usr/bin:/bin:/sbin
export HOME=/root
export PINE_TRACKER_PROJECTS_DIR=/projects

# 1. Mount essential virtual filesystems (devtmpfs auto-populates /dev/fb0, /dev/input/*, /dev/snd/*)
mount -t proc proc /proc 2>/dev/null || true
mount -t sysfs sys /sys 2>/dev/null || true
mount -t devtmpfs dev /dev 2>/dev/null || true
mkdir -p /dev/pts /dev/shm /run /tmp /projects /synths
mount -t devpts devpts /dev/pts 2>/dev/null || true
mount -t tmpfs tmpfs /run 2>/dev/null || true
mount -t tmpfs tmpfs /tmp 2>/dev/null || true

# Ensure root filesystem is writable for project saves (remounted ro on poweroff)
mount -o remount,rw,noatime / 2>/dev/null || true

# Silence kernel printk on console and hide blinking VT cursor
echo 0 > /proc/sys/kernel/printk 2>/dev/null || true
echo 0 > /sys/class/graphics/fbcon/cursor_blink 2>/dev/null || true
printf '\033[?25l\033[2J' > /dev/tty0 2>/dev/null || true
printf '\033[?25l\033[2J' > /dev/tty1 2>/dev/null || true

# 2. Load PinePhone Volume buttons (sun4i-lradc-keys = 1c21800.lradc) synchronously (<15ms)
#    so /dev/input/event* for Volume Up / Volume Down exists before pine_tracker opens evdev
modprobe sun4i-lradc-keys 2>/dev/null || true
modprobe adc-keys 2>/dev/null || true

# 3. Load PinePhone A64 internal audio codec, USB DAC/MIDI, and KB151 battery modules in parallel
(
    for mod in \
        sun4i-lradc-keys \
        snd-soc-core \
        snd-pcm-dmaengine \
        sun8i-adda-pr-regmap \
        sun50i-codec-analog \
        sun8i-codec \
        sun4i-i2s \
        snd-soc-simple-amplifier \
        snd-soc-ec25 \
        snd-soc-bt-sco \
        snd-soc-simple-card-utils \
        snd-soc-simple-card \
        snd-rawmidi \
        snd-usbmidi-lib \
        snd-usb-audio \
        ppkb-manager; do
        modprobe "$mod" 2>/dev/null || true
    done
    /usr/local/bin/pinetracker-rt-tune.sh >/dev/null 2>&1 || true
) &

# 4. Launch pineTracker directly on /dev/fb0 + /dev/input/event*
cd /opt/pinetracker || cd /
/opt/pinetracker/pine_tracker

# 4. Instant clean power-off (<0.5s) when user presses Power button or taps [OFF]
sync
mount -o remount,ro / 2>/dev/null || true
/usr/bin/poweroff -f 2>/dev/null || /sbin/poweroff -f 2>/dev/null || echo o > /proc/sysrq-trigger
exec /bin/sh
