use x86_64::instructions::port::Port;
use crate::serial_println;

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

/// [MICT: MAP] - Scans the entire motherboard for hardware
pub fn enumerate_buses() {
    serial_println!("[MICT: MAP] Engaging PCIe Hardware Radar...");
    
    for bus in 0..=255 {
        for slot in 0..32 {
            let vendor = pci_read_word(bus, slot, 0, 0) & 0xFFFF;
            if vendor == 0xFFFF {
                continue; // 0xFFFF means the slot is empty
            }
            
            // Found a device! Check its functions.
            check_device(bus, slot, 0);
            
            // Check if it's a multi-function device (e.g., Audio + Video on one card)
            let header_type = (pci_read_word(bus, slot, 0, 0x0C) >> 16) & 0xFF;
            if (header_type & 0x80) != 0 {
                for func in 1..8 {
                    if (pci_read_word(bus, slot, func, 0) & 0xFFFF) != 0xFFFF {
                        check_device(bus, slot, func);
                    }
                }
            }
        }
    }
    serial_println!("[MICT: MAP] PCIe Bus Scan Complete.");
}

fn check_device(bus: u8, slot: u8, func: u8) {
    let word0 = pci_read_word(bus, slot, func, 0x00);
    let vendor_id = word0 & 0xFFFF;
    let device_id = word0 >> 16;
    
    let word2 = pci_read_word(bus, slot, func, 0x08);
    let class_code = (word2 >> 24) & 0xFF;
    let subclass = (word2 >> 16) & 0xFF;

    // We use serial_println! for the full hardware map so we don't clutter the blue screen
    crate::serial_println!(
        "  -> Hardware Found[Bus {:02X}, Slot {:02X}, Func {}] Vendor: {:04X}, Device: {:04X} | Class: {:02X}, Subclass: {:02X}",
        bus, slot, func, vendor_id, device_id, class_code, subclass
    );

    // Class 0x01 = Mass Storage Controller. Subclass 0x08 = Non-Volatile Memory (NVMe)
    if class_code == 0x01 && subclass == 0x08 {
        crate::serial_println!("     [*** TARGET LOCKED: NVMe Controller Identified! ***]");
        //crate::screen_println!("[*** TARGET LOCKED: NVMe Controller Identified! ***]");

        // [MICT: MAP] - Extract Base Address Registers (BARs)
        // BAR0 is at offset 0x10. BAR1 is at offset 0x14.
        let bar0 = pci_read_word(bus, slot, func, 0x10);
        let bar1 = pci_read_word(bus, slot, func, 0x14);

        // Check if it's a 64-bit Memory Space BAR (Bit 0 = 0, Bits 1:2 = 10b)
        let is_memory_space = (bar0 & 0x01) == 0;
        let is_64_bit = (bar0 & 0x06) == 0x04;

        if is_memory_space {
            let mmio_base: u64;
            if is_64_bit {
                // Mask out the bottom 4 flag bits of BAR0 and combine with BAR1
                mmio_base = ((bar1 as u64) << 32) | ((bar0 as u64) & 0xFFFF_FFF0);
            } else {
                mmio_base = (bar0 as u64) & 0xFFFF_FFF0;
            }

            crate::serial_println!("     -> NVMe MMIO Base Address: {:#018X}", mmio_base);
            //crate::screen_println!("     -> NVMe MMIO Base Address: {:#018X}", mmio_base);
        } else {
            crate::serial_println!("     -> ERROR: NVMe BAR0 is not mapped to Memory Space!");
        }
    }
}