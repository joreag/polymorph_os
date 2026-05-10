#!/bin/bash
set -e

echo "====================================================="
echo " 🚀 Booting PolymorphOS (Standalone Review Candidate)"
echo "====================================================="

# 1. Create the NVMe Dummy Drive if it doesn't exist
NVME_FILE="genesis_nvme_dummy.img"
if[ ! -f "$NVME_FILE" ]; then
    echo "[+] Creating 10MB NVMe Dummy Storage Drive..."
    # Creates a 10MB blank file filled with zeros
    dd if=/dev/zero of=$NVME_FILE bs=1M count=10 2>/dev/null
fi

# 2. Build the Bare-Metal Kernel
echo "[+] Compiling Bare-Metal Kernel (x86_64-polymorph_os)..."
cargo bootimage

# 3. Launch QEMU
echo "[+] Launching QEMU Hypervisor..."
echo "[!] TIP: Click into the QEMU window to type. Kernel logs will appear here."
echo "-----------------------------------------------------"

# -serial stdio: Routes our serial_println! logs straight to this bash terminal!
# -drive id=nvme: Attaches our dummy file as a physical PCIe NVMe controller.
# -m 2G: Allocates 2GB of RAM for our memory allocator and 3D compositor.
qemu-system-x86_64 \
    -drive format=raw,file=target/x86_64-polymorph_os/debug/bootimage-polymorph_os.bin \
    -drive file=$NVME_FILE,format=raw,if=none,id=nvme_drive \
    -device nvme,serial=deadbeef,drive=nvme_drive \
    -serial stdio \
    -m 2G