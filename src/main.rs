use std::process::{Command, Stdio};
use std::fs::File;
use std::path::Path;

fn main() {
    let uefi_image = env!("UEFI_IMAGE");
    let ovmf_pure_efi = ovmf_prebuilt::ovmf_pure_efi();

    //[MICT: CREATE DUMMY NVME DISK]
    let nvme_path = "genesis_nvme_dummy.img";
    if !Path::new(nvme_path).exists() {
        let f = File::create(nvme_path).expect("Failed to create dummy NVMe file");
        f.set_len(10 * 1024 * 1024).expect("Failed to set file size"); // 10MB
    }

    println!("Booting GenesisOS via QEMU (UEFI)...");
    
    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.arg("-drive").arg(format!("if=pflash,format=raw,readonly=on,file={}", ovmf_pure_efi.display()));
    cmd.arg("-drive").arg(format!("format=raw,file={}", uefi_image));
    
    // Attach the NVMe Drive to the PCIe Bus!
    cmd.arg("-drive").arg(format!("file={},if=none,id=nvmedrv,format=raw", nvme_path));
    cmd.arg("-device").arg("nvme,drive=nvmedrv,serial=genesis01");
    cmd.arg("-device").arg("e1000");
    cmd.arg("-vga").arg("virtio");
    cmd.arg("-global").arg("virtio-vga.xres=1920");
    cmd.arg("-global").arg("virtio-vga.yres=1080");

    // --- [MICT: THE KVM AFTERBURNER] ---
    cmd.arg("-enable-kvm");      // Bypass software emulation, use physical silicon!
    cmd.arg("-cpu").arg("host"); // Give the VM the exact Ultra 7 architecture!
    cmd.arg("-smp").arg("4");    // Give the OS 4 physical cores to play with!
    
    cmd.arg("-no-reboot");
    cmd.arg("-no-shutdown");
    cmd.arg("-serial").arg("tcp:127.0.0.1:4444,server=on,wait=off");

    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    
    let mut child = cmd.spawn().expect("Failed to spawn QEMU");
    child.wait().unwrap();
}