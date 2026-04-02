#!/usr/bin/env bash
set -euo pipefail

REPO=/mnt/c/ITS/Routing/Hanoi-Routing
source ~/.cargo/env

echo "=== Building InertialFlowCutter ==="
cd "$REPO/rust_road_router/lib/InertialFlowCutter"
rm -rf build
mkdir -p build
cd build
cmake -DCMAKE_BUILD_TYPE=Release -DGIT_SUBMODULE=OFF -DUSE_KAHIP=OFF .. 2>&1
make -j"$(nproc)" console 2>&1
echo "InertialFlowCutter built successfully"

echo ""
echo "=== Building CCH-Hanoi workspace ==="
cd "$REPO/CCH-Hanoi"
cargo build --release --workspace 2>&1
echo "CCH-Hanoi built successfully"
