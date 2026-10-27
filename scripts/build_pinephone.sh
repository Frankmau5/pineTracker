#!/usr/bin/env bash
# pineTracker v2.0 — Cross-Compilation & Bundle Script for PinePhone (aarch64 Cortex-A53 + NEON)
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

SYSROOT="${PINEPHONE_SYSROOT:-}"
BUNDLE_DIR="$ROOT_DIR/dist/pinephone-bundle"

echo "==> [1/4] Ensuring starter .wav samples, .toml synth profiles, and demo song exist..."
cargo run --features deck-dev -- --smoke-test

echo "==> [2/4] Checking Rust aarch64-unknown-linux-gnu target..."
rustup target add aarch64-unknown-linux-gnu

if [ -n "$SYSROOT" ] && [ -d "$SYSROOT/usr/lib" ]; then
    echo "==> [3/4] Cross-compiling pine_tracker (aarch64 Cortex-A53 + NEON) against sysroot: $SYSROOT"
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_SYSROOT_DIR="$SYSROOT"
    export PKG_CONFIG_PATH="$SYSROOT/usr/lib/pkgconfig:$SYSROOT/usr/share/pkgconfig"
    WRAPPER="$ROOT_DIR/target/aarch64-sysroot-gcc.sh"
    mkdir -p "$ROOT_DIR/target"
    cat > "$WRAPPER" <<EOF
#!/usr/bin/env bash
exec aarch64-linux-gnu-gcc --sysroot="$SYSROOT" -Wl,-rpath-link,"$SYSROOT/usr/lib" "\$@"
EOF
    chmod +x "$WRAPPER"
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$WRAPPER" \
        cargo build --release --target aarch64-unknown-linux-gnu
fi

echo "==> [4/4] Packaging PinePhone Stage 1/2 deployment bundle in $BUNDLE_DIR..."
rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/opt/pinetracker/data" \
         "$BUNDLE_DIR/projects" \
         "$BUNDLE_DIR/synths" \
         "$BUNDLE_DIR/usr/local/bin" \
         "$BUNDLE_DIR/etc/systemd/system"

if [ -f "target/aarch64-unknown-linux-gnu/release/pine_tracker" ]; then
    cp -f "target/aarch64-unknown-linux-gnu/release/pine_tracker" "$BUNDLE_DIR/opt/pinetracker/pine_tracker"
    chmod +x "$BUNDLE_DIR/opt/pinetracker/pine_tracker"
fi

cp -rf data/samples "$BUNDLE_DIR/opt/pinetracker/data/"
cp -rf data/projects "$BUNDLE_DIR/opt/pinetracker/data/"
cp -rf data/synths "$BUNDLE_DIR/opt/pinetracker/data/"
cp -f data/projects/*.toml "$BUNDLE_DIR/projects/"
cp -f data/synths/*.toml "$BUNDLE_DIR/synths/"
cp -f deploy/stage1-arch/pinetracker-rt-tune.sh "$BUNDLE_DIR/usr/local/bin/pinetracker-rt-tune.sh"
chmod +x "$BUNDLE_DIR/usr/local/bin/pinetracker-rt-tune.sh"
cp -f deploy/stage1-arch/pinetracker-init.sh "$BUNDLE_DIR/usr/local/bin/pinetracker-init"
chmod +x "$BUNDLE_DIR/usr/local/bin/pinetracker-init"
cp -f deploy/stage1-arch/pine-tracker.service "$BUNDLE_DIR/etc/systemd/system/pine-tracker.service"

echo "==> Bundle ready at: $BUNDLE_DIR"
