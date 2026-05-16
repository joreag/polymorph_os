#!/bin/bash
set -e

echo "====================================================="
echo " 🚀 Booting PolymorphOS (Standalone Review Candidate)"
echo "====================================================="

NVME_FILE="genesis_nvme_dummy.img"

if [ ! -f "$NVME_FILE" ]; then
    echo "[+] Creating 10MB NVMe Dummy Storage Drive..."
    dd if=/dev/zero of="$NVME_FILE" bs=1M count=10 2>/dev/null
fi

echo "[+] Compiling Bare-Metal Kernel..."
# Force Cargo to explicitly target the kernel, bypassing genesis_builder
cargo bootimage --manifest-path kernel/Cargo.toml

echo "[+] Locating compiled binary..."
BIN_PATH="target/x86_64-polymorph_os/debug/bootimage-polymorph_os.bin"

# If Cargo placed the target folder inside the kernel directory, update the path
if [ ! -f "$BIN_PATH" ]; then
    BIN_PATH="kernel/target/x86_64-polymorph_os/debug/bootimage-polymorph_os.bin"
fi

echo "[+] Launching QEMU Hypervisor..."
echo "[!] TIP: Click into the QEMU window to type. Logs will appear here."
echo "-----------------------------------------------------"

qemu-system-x86_64 \
    -drive format=raw,file=$BIN_PATH \
    -drive file=$NVME_FILE,format=raw,if=none,id=nvme_drive \
    -device nvme,serial=deadbeef,drive=nvme_drive \
    -device e1000 \
    -vga virtio \
    -serial stdio \
    -m 2G