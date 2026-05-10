@echo off
echo =====================================================
echo  Booting PolymorphOS (Standalone Review Candidate)
echo =====================================================

IF NOT EXIST "genesis_nvme_dummy.img" (
    echo [+] Creating 10MB NVMe Dummy Storage Drive...
    fsutil file createnew genesis_nvme_dummy.img 10485760
)

echo [+] Compiling Bare-Metal Kernel...
cargo bootimage

echo [+] Launching QEMU Hypervisor...
echo [!] TIP: Click into the QEMU window to type. Kernel logs will appear here.
echo -----------------------------------------------------

qemu-system-x86_64 ^
    -drive format=raw,file=target\x86_64-polymorph_os\debug\bootimage-polymorph_os.bin ^
    -drive file=genesis_nvme_dummy.img,format=raw,if=none,id=nvme_drive ^
    -device nvme,serial=deadbeef,drive=nvme_drive ^
    -serial stdio ^
    -m 2G