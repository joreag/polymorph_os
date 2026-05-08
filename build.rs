use std::path::PathBuf;

fn main() {
    // Grab the compiled kernel binary
    let kernel_path = std::env::var("CARGO_BIN_FILE_KERNEL_polymorph_os").unwrap();
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    
    // Create the UEFI disk image
    let uefi_path = out_dir.join("genesis-uefi.img");
    bootloader::UefiBoot::new(&PathBuf::from(kernel_path))
        .create_disk_image(&uefi_path)
        .unwrap();

    // Pass the path to our QEMU runner
    println!("cargo:rustc-env=UEFI_IMAGE={}", uefi_path.display());
}