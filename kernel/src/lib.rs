#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;
use core::panic::PanicInfo;

pub mod allocator;
pub mod gdt;
pub mod interrupts;
pub mod memory;
pub mod serial;
pub mod task;
pub mod vga_buffer;
pub mod pci;
pub mod gpu_driver;
pub mod nvme;
pub mod splat;
pub mod mfs;
pub mod e1000;
pub mod virtio_gpu;
pub mod virtio_pci;
pub mod virtqueue;


pub fn init() {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };

    //[MICT: HARDWARE FIREWALL & MOUSE INITIALIZATION]
    unsafe {
        use x86_64::instructions::port::Port;
        
        // 1. Wake the Mouse (PS/2 Controller)
        let mut cmd_port = Port::<u8>::new(0x64);
        let mut data_port = Port::<u8>::new(0x60);
        
        cmd_port.write(0xD4); // Tell controller: "Next byte goes to the mouse"
        data_port.write(0xF4); // Tell mouse: "Enable Data Reporting"
        
        // Read the ACK byte to clear the buffer
        let _ack: u8 = data_port.read(); 

        // 2. Drop the Master PIC Firewalls (Port 0x21)
        let mut pic1_data = Port::<u8>::new(0x21);
        let mut mask1 = pic1_data.read();
        mask1 &= !(1 << 4); // Unmask IRQ 4 (Serial)
        mask1 &= !(1 << 1); // Unmask IRQ 1 (Keyboard)
        mask1 &= !(1 << 2); // Unmask IRQ 2 (Cascade to Slave PIC)
        pic1_data.write(mask1);

        // 3. Drop the Slave PIC Firewalls (Port 0xA1)
        // The mouse is on IRQ 12. Since Slave handles IRQ 8-15, IRQ 12 is Bit 4.
        let mut pic2_data = Port::<u8>::new(0xA1);
        let mut mask2 = pic2_data.read();
        mask2 &= !(1 << 4); // Unmask IRQ 12 (Mouse)
        pic2_data.write(mask2);
    }

    x86_64::instructions::interrupts::enable();
}
pub trait Testable {
    fn run(&self) -> ();
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        serial_print!("{}...\t", core::any::type_name::<T>());
        self();
        serial_println!("[ok]");
    }
}

pub fn test_runner(tests: &[&dyn Testable]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;

    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[cfg(test)]
use bootloader_api::{entry_point, BootInfo, config::Mapping};

#[cfg(test)]
pub static TEST_BOOTLOADER_CONFIG: bootloader_api::BootloaderConfig = {
    let mut config = bootloader_api::BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config
};

#[cfg(test)]
entry_point!(test_kernel_main, config = &TEST_BOOTLOADER_CONFIG);

/// Entry point for `cargo test`
#[cfg(test)]
fn test_kernel_main(_boot_info: &'static mut BootInfo) -> ! {
    init();
    test_main();
    hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}