use x86_64::instructions::port::Port;
use alloc::string::String;
use x86_64::structures::paging::{FrameAllocator, Mapper, Size4KiB, Translate};
use x86_64::VirtAddr;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// Reads a 32-bit word from the PCI Configuration Space
pub fn pci_read_word(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let address: u32 = 0x80000000 
        | ((bus as u32) << 16) 
        | ((slot as u32) << 11) 
        | ((func as u32) << 8) 
        | (offset as u32 & 0xFC);
    
    unsafe {
        let mut addr_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);
        
        addr_port.write(address);
        data_port.read()
    }
}

///[MICT: MAP] - Scans the entire motherboard for hardware
pub fn enumerate_buses(
    mapper: &mut (impl Mapper<Size4KiB> + Translate),
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    phys_mem_offset: VirtAddr,
) {
    crate::serial_println!("[MICT: MAP] Engaging PCIe Hardware Radar...");
    
    for bus in 0..=255 {
        for slot in 0..32 {
            let vendor = pci_read_word(bus, slot, 0, 0) & 0xFFFF;
            if vendor == 0xFFFF { continue; }
            
            check_device(bus, slot, 0, mapper, frame_allocator, phys_mem_offset); 
            
            let header_type = (pci_read_word(bus, slot, 0, 0x0C) >> 16) & 0xFF;
            if (header_type & 0x80) != 0 {
                for func in 1..8 {
                    if (pci_read_word(bus, slot, func, 0) & 0xFFFF) != 0xFFFF {
                        check_device(bus, slot, func, mapper, frame_allocator, phys_mem_offset);
                    }
                }
            }
        }
    }
    crate::serial_println!("[MICT: MAP] PCIe Bus Scan Complete.");
}

fn check_device(
    bus: u8, slot: u8, func: u8,
    mapper: &mut (impl Mapper<Size4KiB> + Translate),
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    phys_mem_offset: VirtAddr,
) {
    let word0 = pci_read_word(bus, slot, func, 0x00);
    let vendor_id = word0 & 0xFFFF;
    let device_id = word0 >> 16;
    
    let word2 = pci_read_word(bus, slot, func, 0x08);
    let class_code = (word2 >> 24) & 0xFF;
    let subclass = (word2 >> 16) & 0xFF;

    crate::serial_println!(
        "  -> Hardware Found[Bus {:02X}, Slot {:02X}, Func {}] Vendor: {:04X}, Device: {:04X} | Class: {:02X}, Subclass: {:02X}",
        bus, slot, func, vendor_id, device_id, class_code, subclass
    );

    // --- TARGET 1: NVMe STORAGE ---
    if class_code == 0x01 && subclass == 0x08 {
        crate::serial_println!("[*** TARGET LOCKED: NVMe Controller Identified! ***]");

        let bar0 = pci_read_word(bus, slot, func, 0x10);
        let bar1 = pci_read_word(bus, slot, func, 0x14);

        let is_memory_space = (bar0 & 0x01) == 0;
        let is_64_bit = (bar0 & 0x06) == 0x04;

        if is_memory_space {
            let mmio_base: u64 = if is_64_bit {
                ((bar1 as u64) << 32) | ((bar0 as u64) & 0xFFFF_FFF0)
            } else {
                (bar0 as u64) & 0xFFFF_FFF0
            };
            
            crate::serial_println!("     -> NVMe Physical Base: {:#018X}", mmio_base);
            
            // [MICT: DYNAMIC NVMe INITIALIZATION]
            unsafe {
                let nvme_virtual_addr = crate::memory::map_mmio(
                    mmio_base, 0x4000, mapper, frame_allocator
                ).expect("[FATAL] Failed to map NVMe MMIO space!");
                
                let mut nvme_drive = crate::nvme::NvmeController::new(nvme_virtual_addr.as_u64() as usize); 
                nvme_drive.ping();
                nvme_drive.disable();
                nvme_drive.configure_and_enable(mapper);
                nvme_drive.identify_controller(mapper);
                nvme_drive.setup_io_queues(mapper);

                *crate::nvme::NVME_DRIVE.lock() = Some(nvme_drive);
                crate::serial_println!("     -> [OK] NVMe Controller Initialized dynamically.");
            }
        }
    } 
    // --- TARGET 2: INTEL E1000 GIGABIT ETHERNET ---
    else if vendor_id == 0x8086 && device_id == 0x100E {
        crate::serial_println!("[*** TARGET LOCKED: Intel E1000 Network Card! ***]");
        let bar0 = pci_read_word(bus, slot, func, 0x10);
        
        if (bar0 & 0x01) == 0 {
            let mmio_base = (bar0 & 0xFFFF_FFF0) as u64;
            crate::serial_println!("     -> E1000 Physical Base: {:#010X}", mmio_base);
            
            // [MICT: MEMORY MAPPING] - Map the E1000 registers!
            unsafe { 
                if let Err(e) = crate::memory::map_mmio(mmio_base, 0x20000, mapper, frame_allocator) {
                    crate::serial_println!("     -> [FATAL] Failed to map E1000 MMIO: {:?}", e);
                } else {
                    crate::serial_println!("     ->[OK] E1000 MMIO Mapped.");
                    
                    // --- THE UPDATE: Pass the frame allocator and offset! ---
                    let driver = crate::e1000::E1000Driver::new(mmio_base as usize, frame_allocator, phys_mem_offset); 
                    
                    *crate::e1000::E1000_NET.lock() = Some(driver);
                    crate::serial_println!("     -> [OK] Intel E1000 Driver Initialized and Locked.");
                    
                }
            }
        }
    }
    

    // --- TARGET 3: VIRTIO GPU ---
    else if vendor_id == 0x1AF4 && device_id == 0x1050 {
        crate::serial_println!("[*** TARGET LOCKED: VirtIO Graphics Adapter! ***]");
        
        //[MICT: INITIATE THE HANDSHAKE]
        // Notice we are now passing `phys_mem_offset` at the very end!
        if let Err(e) = crate::virtio_pci::init_virtio_device(bus, slot, func, mapper, frame_allocator, phys_mem_offset) {
            crate::serial_println!("     -> [FATAL] VirtIO Handshake Failed: {}", e);
        } else {
            crate::serial_println!("     -> [OK] VirtIO Handshake process started.");
        }
    }

    

}

/// [MICT: DYNAMIC RADAR] - Sweeps the PCIe bus in real-time without initializing drivers.
pub fn scan_pci_dynamic() -> String {
    let mut output = String::from("[MICT: MAP] Initiating Dynamic PCIe Radar...\n");
    
    for bus in 0..=255 {
        for slot in 0..32 {
            let vendor = pci_read_word(bus, slot, 0, 0) & 0xFFFF;
            if vendor == 0xFFFF {
                continue; // Slot is empty
            }
            
            // Format the main device
            output.push_str(&probe_device_to_string(bus, slot, 0));
            
            // Check for multi-function devices
            let header_type = (pci_read_word(bus, slot, 0, 0x0C) >> 16) & 0xFF;
            if (header_type & 0x80) != 0 {
                for func in 1..8 {
                    if (pci_read_word(bus, slot, func, 0) & 0xFFFF) != 0xFFFF {
                        output.push_str(&probe_device_to_string(bus, slot, func));
                    }
                }
            }
        }
    }
    output.push_str("[MICT: MAP] Dynamic Scan Complete.\n");
    output
}

/// Helper function to translate hardware IDs into human-readable strings
fn probe_device_to_string(bus: u8, slot: u8, func: u8) -> String {
    let word0 = pci_read_word(bus, slot, func, 0x00);
    let vendor_id = word0 & 0xFFFF;
    let device_id = word0 >> 16;
    
    let word2 = pci_read_word(bus, slot, func, 0x08);
    let class_code = (word2 >> 24) & 0xFF;
    let subclass = (word2 >> 16) & 0xFF;

    // Friendly names for our known hardware
    let mut name = "Unknown Device";
    if class_code == 0x01 && subclass == 0x08 {
        name = "NVMe Controller";
    } else if vendor_id == 0x8086 && device_id == 0x100E {
        name = "Intel E1000 Gigabit Network";
    } else if vendor_id == 0x8086 && device_id == 0x1237 {
        name = "Intel Host Bridge";
    } else if class_code == 0x03 && subclass == 0x00 {
        name = "VGA Compatible Controller";
    }

    alloc::format!(
        "  -> Bus {:02X}, Slot {:02X}, Func {} | Ven: {:04X}, Dev: {:04X}[{}]\n",
        bus, slot, func, vendor_id, device_id, name
    )
}