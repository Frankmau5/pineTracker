#!/usr/bin/env bash
# pineTracker v2.0 — PinePhone (Allwinner A64 + KB151) Real-Time Hardware Tuning
# Executed automatically before pine_tracker starts via pine-tracker.service.
set -u

echo "[pineTracker-rt] Applying Allwinner A64 + KB151 real-time hardware tuning..."

# 1. Lock CPU frequency governor to 'performance' across all available cores
for gov in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
    if [ -w "$gov" ]; then
        echo performance > "$gov" 2>/dev/null || true
    fi
done

# 2. Disable ASoC codec power-down pop (snd_soc_core.pmdown_time=0)
if [ -w /sys/module/snd_soc_core/parameters/pmdown_time ]; then
    echo 0 > /sys/module/snd_soc_core/parameters/pmdown_time 2>/dev/null || true
fi

# 3. Pin USB, I2C (KB151 matrix @ 0x15 + IP5209), and MMC IRQs to Cores 0-1 (smp_affinity=3)
#    leaving Isolated Cores 2-3 dedicated to Thread 2 (48kHz 128-frame DSP + 24/192 PPQN clock).
for irq_dir in /proc/irq/[0-9]*; do
    [ -d "$irq_dir" ] || continue
    aff="$irq_dir/smp_affinity"
    if [ -w "$aff" ]; then
        echo 3 > "$aff" 2>/dev/null || true
    fi
done

# 4. Unmute Allwinner A64 Internal 3.5mm Audio Codec (hw:0,0) at 0 dB unity digital gain (160)
if command -v amixer >/dev/null 2>&1; then
    amixer -c 0 sset 'Headphone' 70% unmute 2>/dev/null || true
    amixer -c 0 sset 'Headphone Source' 'DAC' 2>/dev/null || true
    amixer -c 0 sset 'Line Out' 70% unmute 2>/dev/null || true
    amixer -c 0 sset 'AIF1 Slot 0 Digital DAC' unmute 2>/dev/null || true
    amixer -c 0 sset 'AIF1 DA0' 160 2>/dev/null || true
    amixer -c 0 sset 'DAC' 160 unmute 2>/dev/null || true
fi

# 5. Disable console cursor blink on tty1 for clean KMSDRM display
if [ -w /sys/class/graphics/fbcon/cursor_blink ]; then
    echo 0 > /sys/class/graphics/fbcon/cursor_blink 2>/dev/null || true
fi

echo "[pineTracker-rt] Real-time tuning complete."
exit 0
