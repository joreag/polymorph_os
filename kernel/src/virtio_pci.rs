// kernel/src/virtio_pci.rs
// The Transport Layer for Modern (v1.0+) VirtIO Devices

use crate::pci::pci_read_word;
use core::ptr;
use x86_64::structures::paging::{FrameAllocator, Mapper, Size4KiB};
use x86_64::VirtAddr;

// --- VIRTIO STATUS FLAGS (The 7-Step Handshake) ---
pub const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
pub const VIRTIO_STATUS_DRIVER: u8 = 2;
pub const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
pub const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
pub const VIRTIO_STATUS_FAILED: u8 = 128;

// --- PCI CAPABILITY IDS ---
const PCI_CAP_ID_VNDR: u8 = 0x09; // Vendor Specific

// --- VIRTIO CAPABILITY TYPES ---
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;

/// Holds the physical addresses of the extracted configuration regions
#[derive(Debug)]
pub struct VirtioCapabilities {
    pub common_cfg_phys: u64,
    pub notify_cfg_phys: u64,
    pub isr_cfg_phys: u64,
    pub notify_off_multiplier: u32,
}

///[MICT: THE VIRTIO HANDSHAKE ORCHESTRATOR]
/// Maps the capabilities into memory and performs the 7-step wake-up sequence.
pub fn init_virtio_device(
    bus: u8, slot: u8, func: u8,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    phys_mem_offset: VirtAddr, 
) -> Result<(), &'static str> {
    crate::serial_println!("[VIRTIO] Initiating Device Handshake on Bus {:02X}, Slot {:02X}...", bus, slot);

    // 1. Find the physical addresses using our walker
    let caps = match find_capabilities(bus, slot, func) {
        Some(c) => c,
        None => return Err("Failed to find required VirtIO Capabilities."),
    };

    // 2. Map the Common Configuration Structure into Virtual Memory (1 page / 4096 bytes is enough)
    crate::serial_println!("[VIRTIO] Mapping Common Configuration...");
    let common_cfg_virt = unsafe {
        match crate::memory::map_mmio(caps.common_cfg_phys, 4096, mapper, frame_allocator) {
            Ok(addr) => addr,
            Err(_) => return Err("Failed to map Common Configuration MMIO space."),
        }
    };

    // The Virtio Common Config Structure Layout (v1.0 spec):
    // Offset 0x14: device_status (u8)
    let status_ptr = (common_cfg_virt.as_u64() + 0x14) as *mut u8;

    unsafe {
        // --- THE 7-STEP SPECIFICATION DANCE ---
        
        // Step 1: Reset the device (Write 0 to status)
        core::ptr::write_volatile(status_ptr, 0);
        
        // Step 2: Set ACKNOWLEDGE (We found the device)
        let mut status = core::ptr::read_volatile(status_ptr);
        status |= VIRTIO_STATUS_ACKNOWLEDGE;
        core::ptr::write_volatile(status_ptr, status);
        
        // Step 3: Set DRIVER (We know how to drive it)
        status |= VIRTIO_STATUS_DRIVER;
        core::ptr::write_volatile(status_ptr, status);

        crate::serial_println!("[VIRTIO] Device Acknowledged. Status set to DRIVER.");

        // Step 4: Feature Negotiation (Stub for now)
        // Step 5: Set FEATURES_OK
        status |= VIRTIO_STATUS_FEATURES_OK;
        core::ptr::write_volatile(status_ptr, status);
        
        // Check if the device accepted our features
        let check_status = core::ptr::read_volatile(status_ptr);
        if (check_status & VIRTIO_STATUS_FEATURES_OK) == 0 {
            return Err("Device refused our features!");
        }
        
        crate::serial_println!("[VIRTIO] Features Negotiated.");

        // --- Step 6: Setup Virtqueues ---
        // The Virtio Common Config Structure Layout (v1.0 spec):
        // Offset 0x16: queue_select (u16)  - Write here to select which queue to configure
        // Offset 0x18: queue_size (u16)    - Read here to see how big the GPU wants the queue
        // Offset 0x1C: queue_enable (u16)  - Write 1 here to turn the queue on
        // Offset 0x20: queue_notify_off (u16)
        // Offset 0x28: queue_desc (u64)    - Write Physical Addr of Descriptor Table here
        // Offset 0x30: queue_driver (u64)  - Write Physical Addr of Available Ring here
        // Offset 0x38: queue_device (u64)  - Write Physical Addr of Used Ring here

        // --- Step 6: Setup Virtqueues ---
        let q_select_ptr = (common_cfg_virt.as_u64() + 0x16) as *mut u16;
        let q_size_ptr = (common_cfg_virt.as_u64() + 0x18) as *mut u16;
        let q_enable_ptr = (common_cfg_virt.as_u64() + 0x1C) as *mut u16;
        let q_notify_off_ptr = (common_cfg_virt.as_u64() + 0x1E) as *mut u16; // Was 0x20!
        let q_desc_ptr = (common_cfg_virt.as_u64() + 0x20) as *mut u64;       // Was 0x28!
        let q_driver_ptr = (common_cfg_virt.as_u64() + 0x28) as *mut u64;     // Was 0x30!
        let q_device_ptr = (common_cfg_virt.as_u64() + 0x30) as *mut u64;     // Was 0x38!

        // Select Queue 0 (Control Queue)
        core::ptr::write_volatile(q_select_ptr, 0);

        let q_size = core::ptr::read_volatile(q_size_ptr);
        if q_size == 0 {
            return Err("GPU reports Control Queue size is 0.");
        }
        crate::serial_println!("[VIRTIO] Hardware requires Control Queue size: {}", q_size);

        // Map the Notify Configuration structure so we can talk to the Doorbell
        let notify_cfg_virt = match crate::memory::map_mmio(caps.notify_cfg_phys, 4096, mapper, frame_allocator) {
            Ok(addr) => addr,
            Err(_) => return Err("Failed to map Notify_Cfg MMIO space."),
        };

        // Create the Driver
        let mut gpu_driver = crate::virtio_gpu::VirtioGpuDriver::new(caps.common_cfg_phys as usize);

        // Ask the driver to allocate the physical memory, and get the EXACT addresses back
        let (desc_phys, avail_phys, used_phys) = match gpu_driver.setup_control_queue(frame_allocator, phys_mem_offset) {
            Ok(addrs) => addrs,
            Err(e) => return Err(e),
        };

        // Tell the GPU exactly where the rings are! No more guessing.
        core::ptr::write_volatile(q_desc_ptr, desc_phys); 
        core::ptr::write_volatile(q_driver_ptr, avail_phys); 
        core::ptr::write_volatile(q_device_ptr, used_phys); 

        // 3. Calculate and set the Doorbell Address
        let q_notify_off = core::ptr::read_volatile(q_notify_off_ptr);
        let doorbell_virt_addr = notify_cfg_virt.as_u64() + (q_notify_off as u64 * caps.notify_off_multiplier as u64);
        
        gpu_driver.set_notify_address(doorbell_virt_addr);
        crate::serial_println!("[VIRTIO] Doorbell wired to Virtual Address: {:#X}", doorbell_virt_addr);

        // 4. Enable the Queue
        core::ptr::write_volatile(q_enable_ptr, 1);
        crate::serial_println!("[VIRTIO] Control Queue (QID 0) Wired and Enabled.");

        // --- Step 7: Set DRIVER_OK ---
        status |= VIRTIO_STATUS_DRIVER_OK;
        core::ptr::write_volatile(status_ptr, status);
        crate::serial_println!("[VIRTIO] Handshake Complete. DRIVER_OK set. Device is LIVE.");

        // [MICT: SEND THE FIRST COMMAND]
        // We use gpu_driver locally BEFORE we give it away to the Mutex!
        if let Err(e) = gpu_driver.get_display_info() {
            crate::serial_println!("[VIRTIO FATAL] Failed to send command: {}", e);
        }

        // --- THE FIX: Lock into global state LAST ---
        // Now that the handshake and test are complete, we move ownership 
        // of the driver into the global Mutex for the rest of the OS to use.
        *crate::virtio_gpu::VIRTIO_GPU.lock() = Some(gpu_driver);
    }

    Ok(())
}

/// [MICT: THE CAPABILITY WALKER]
pub fn find_capabilities(bus: u8, slot: u8, func: u8) -> Option<VirtioCapabilities> {
    crate::serial_println!("  [VIRTIO-PCI] Walking Capability List...");

    let cap_ptr = (pci_read_word(bus, slot, func, 0x34) & 0xFF) as u8;
    if cap_ptr == 0 {
        return None;
    }

    let mut current_cap = cap_ptr;
    let mut caps = VirtioCapabilities {
        common_cfg_phys: 0, notify_cfg_phys: 0, isr_cfg_phys: 0, notify_off_multiplier: 0,
    };

    for _ in 0..48 {
        if current_cap == 0 { break; }

        let word0 = pci_read_word(bus, slot, func, current_cap);
        let cap_id = (word0 & 0xFF) as u8;
        let next_cap = ((word0 >> 8) & 0xFF) as u8;

        if cap_id == PCI_CAP_ID_VNDR {
            let cfg_type = ((word0 >> 24) & 0xFF) as u8;
            
            let word1 = pci_read_word(bus, slot, func, current_cap + 4);
            let bar_idx = (word1 & 0xFF) as u8;
            let offset = pci_read_word(bus, slot, func, current_cap + 8) as u64;

            let bar_val = pci_read_word(bus, slot, func, 0x10 + (bar_idx * 4));
            
            //[MICT: 64-BIT BAR FIX]
            let is_64_bit = (bar_val & 0x06) == 0x04;
            let mut phys_base = (bar_val & 0xFFFF_FFF0) as u64; 
            
            if is_64_bit {
                let bar_val_high = pci_read_word(bus, slot, func, 0x14 + (bar_idx * 4));
                phys_base |= (bar_val_high as u64) << 32;
            }

            let final_phys_addr = phys_base + offset;

            match cfg_type {
                VIRTIO_PCI_CAP_COMMON_CFG => {
                    crate::serial_println!("    -> Found Common_Cfg at Phys: {:#010X}", final_phys_addr);
                    caps.common_cfg_phys = final_phys_addr;
                },
                VIRTIO_PCI_CAP_NOTIFY_CFG => {
                    crate::serial_println!("    -> Found Notify_Cfg at Phys: {:#010X}", final_phys_addr);
                    caps.notify_cfg_phys = final_phys_addr;
                    caps.notify_off_multiplier = pci_read_word(bus, slot, func, current_cap + 12);
                },
                VIRTIO_PCI_CAP_ISR_CFG => {
                    crate::serial_println!("    -> Found ISR_Cfg at Phys: {:#010X}", final_phys_addr);
                    caps.isr_cfg_phys = final_phys_addr;
                },
                _ => {} 
            }
        }
        current_cap = next_cap;
    }

    if caps.common_cfg_phys != 0 && caps.notify_cfg_phys != 0 {
        Some(caps)
    } else {
        None
    }
}