#!/usr/bin/env bash
set -euo pipefail

REPO=/mnt/c/ITS/Routing/Hanoi-Routing

echo "=== Fixing line endings ==="
find "$REPO/RoutingKit" -name "generate_make_file" -exec dos2unix {} \;
find "$REPO/scripts" -type f -exec dos2unix {} \;
find "$REPO/CCH-Generator/scripts" -type f -exec dos2unix {} \;
dos2unix "$REPO/rust_road_router/flow_cutter_cch_order.sh"
dos2unix "$REPO/rust_road_router/flow_cutter_cch_cut_order.sh"
dos2unix "$REPO/rust_road_router/flow_cutter_cch_cut_reorder.sh"
echo "Line endings fixed"

echo ""
echo "=== Phase 1: Building RoutingKit ==="
cd "$REPO/RoutingKit"
python3 generate_make_file
make -j"$(nproc)"
echo "RoutingKit built successfully"

echo ""
echo "=== Phase 2: Building CCH-Generator ==="
cd "$REPO/CCH-Generator"
mkdir -p build lib
cmake -S . -B build \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_RUNTIME_OUTPUT_DIRECTORY="${PWD}/lib"
cmake --build build -j"$(nproc)"
echo "CCH-Generator built successfully"

echo ""
echo "=== Phase 3: Building InertialFlowCutter ==="
cd "$REPO/rust_road_router/lib/InertialFlowCutter"
mkdir -p build
cd build
cmake -DCMAKE_BUILD_TYPE=Release -DGIT_SUBMODULE=OFF -DUSE_KAHIP=OFF ..
make -j"$(nproc)" console
echo "InertialFlowCutter built successfully"

echo ""
echo "=== Phase 4: Building CCH-Hanoi workspace ==="
source ~/.cargo/env
cd "$REPO/CCH-Hanoi"
cargo build --release --workspace 2>&1
echo "CCH-Hanoi built successfully"

echo ""
echo "=== All builds completed ==="
