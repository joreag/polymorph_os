// kernel/src/e1000.rs

use core::ptr;
use alloc::vec::Vec;
use spin::Mutex;

pub static E1000_NET: Mutex<Option<E1000Driver>> = Mutex::new(None);


// --- 1. INTEL E1000 HARDWARE REGISTERS ---
const CTRL: u32 = 0x00;
const STATUS: u32 = 0x08;

// Receive Control
const RCTL: u32 = 0x100;
const RCTL_EN: u32 = 1 << 1;     // Receiver Enable
const RCTL_BAM: u32 = 1 << 15;   // Broadcast Accept Mode
const RCTL_SECRC: u32 = 1 << 26; // Strip Ethernet CRC

// Transmit Control
const TCTL: u32 = 0x400;
const TCTL_EN: u32 = 1 << 1;     // Transmit Enable
const TCTL_PSP: u32 = 1 << 3;    // Pad Short Packets

// Receive Ring Registers
const RDBAL: u32 = 0x2800; // Rx Descriptor Base Address Low
const RDBAH: u32 = 0x2804; // Rx Descriptor Base Address High
const RDLEN: u32 = 0x2808; // Rx Descriptor Length
const RDH: u32 = 0x2810;   // Rx Descriptor Head
const RDT: u32 = 0x2818;   // Rx Descriptor Tail

// Transmit Ring Registers
const TDBAL: u32 = 0x3800; // Tx Descriptor Base Address Low
const TDBAH: u32 = 0x3804; // Tx Descriptor Base Address High
const TDLEN: u32 = 0x3808; // Tx Descriptor Length
const TDH: u32 = 0x3810;   // Tx Descriptor Head
const TDT: u32 = 0x3818;   // Tx Descriptor Tail

// MAC Address Registers
const RAL0: u32 = 0x5400;
const RAH0: u32 = 0x5404;

// --- 2. DIRECT MEMORY ACCESS (DMA) STRUCTS ---
// These MUST be #[repr(C, packed)] so Rust doesn't change the memory layout!
#[derive(Debug, Copy, Clone)]
#[repr(C, packed)]
struct ReceiveDescriptor {
    buffer_addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    error: u8,
    special: u16,
}

#[derive(Debug, Copy, Clone)]
#[repr(C, packed)]
struct TransmitDescriptor {
    buffer_addr: u64,
    length: u16,
    cso: u8,
    command: u8,
    status: u8,
    css: u8,
    special: u16,
}

// Commands for the Transmit Descriptor
const TD_CMD_EOP: u8 = 1 << 0; // End of Packet
const TD_CMD_IFCS: u8 = 1 << 1; // Insert FCS (Ethernet CRC)
const TD_CMD_RS: u8 = 1 << 3;  // Report Status

// --- 3. THE POLYMORPH OS DRIVER ---
pub struct E1000Driver {
    mmio_base: usize,
    mac_address: [u8; 6],
    
    // In a real implementation, these will be pinned memory regions
    // allocated by your MictGlobalAllocator.
    tx_ring: Vec<TransmitDescriptor>,
    rx_ring: Vec<ReceiveDescriptor>,
    tx_index: usize,
    rx_index: usize,
}

impl E1000Driver {
    /// Initialize the card using the BAR0 address found by our PCI Radar
    pub unsafe fn new(bar0_address: usize) -> Self {
        let mut driver = E1000Driver {
            mmio_base: bar0_address,
            mac_address: [0; 6],
            tx_ring: Vec::with_capacity(16),
            rx_ring: Vec::with_capacity(16),
            tx_index: 0,
            rx_index: 0,
        };

        driver.read_mac_address();
        // driver.init_rx_ring();
        // driver.init_tx_ring();
        // driver.enable_card();
        
        driver
    }

    /// Pure hardware MMIO read
    unsafe fn read_reg(&self, register: u32) -> u32 {
        ptr::read_volatile((self.mmio_base + register as usize) as *const u32)
    }

    /// Pure hardware MMIO write
    unsafe fn write_reg(&self, register: u32, data: u32) {
        ptr::write_volatile((self.mmio_base + register as usize) as *mut u32, data);
    }

    unsafe fn read_mac_address(&mut self) {
        let mac_low = self.read_reg(RAL0);
        let mac_high = self.read_reg(RAH0);
        
        self.mac_address =[
            mac_low as u8,
            (mac_low >> 8) as u8,
            (mac_low >> 16) as u8,
            (mac_low >> 24) as u8,
            mac_high as u8,
            (mac_high >> 8) as u8,
        ];
        crate::serial_println!("[E1000] MAC Address Extracted: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", 
            self.mac_address[0], self.mac_address[1], self.mac_address[2], 
            self.mac_address[3], self.mac_address[4], self.mac_address[5]);
    }

    /// [MICT: THE OMZTA TRANSMIT GATEWAY]
    /// This function accepts a raw ethernet frame. It should ONLY be called
    /// by the AST evaluator after `NetworkSocket.mdo` validates the payload!
    pub unsafe fn transmit_packet_secure(&mut self, payload: &[u8]) {
        if payload.len() > 1518 {
            crate::serial_println!("[E1000 ERROR] Payload exceeds standard MTU.");
            return;
        }

        // 1. Get the current descriptor from the ring
        // 2. Point it to the payload buffer in RAM
        // 3. Set the End of Packet (EOP) and Insert Checksum (IFCS) flags
        // 4. Update the Transmit Tail (TDT) register to ring the hardware doorbell!
        
        crate::serial_println!("[E1000] Ringing hardware doorbell. Transmitting {} bytes.", payload.len());
        // self.write_reg(TDT, self.tx_index as u32);
    }
}