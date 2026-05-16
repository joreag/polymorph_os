// kernel/src/e1000.rs

use core::ptr;
use spin::Mutex;
use x86_64::VirtAddr;
use x86_64::structures::paging::{FrameAllocator, Size4KiB};

pub static E1000_NET: Mutex<Option<E1000Driver>> = Mutex::new(None);

const CTRL: u32 = 0x00;
const STATUS: u32 = 0x08;
const RCTL: u32 = 0x100;
const TCTL: u32 = 0x400;
const TCTL_EN: u32 = 1 << 1;     
const TCTL_PSP: u32 = 1 << 3;    
const RDBAL: u32 = 0x2800; 
const RDBAH: u32 = 0x2804; 
const RDLEN: u32 = 0x2808; 
const RDH: u32 = 0x2810;   
const RDT: u32 = 0x2818;   
const TDBAL: u32 = 0x3800; 
const TDBAH: u32 = 0x3804; 
const TDLEN: u32 = 0x3808; 
const TDH: u32 = 0x3810;   
const TDT: u32 = 0x3818;   
const RAL0: u32 = 0x5400;
const RAH0: u32 = 0x5404;

const NUM_TX_DESCRIPTORS: usize = 32;
const NUM_RX_DESCRIPTORS: usize = 32;
const RCTL_EN: u32 = 1 << 1;     // Receiver Enable
const RCTL_UPE: u32 = 1 << 3;    //[NEW] Unicast Promiscuous Enable
const RCTL_MPE: u32 = 1 << 4;    // [NEW] Multicast Promiscuous Enable
const RCTL_BAM: u32 = 1 << 15;   // Broadcast Accept Mode
const RCTL_SECRC: u32 = 1 << 26; // Strip Ethernet CRC

#[derive(Debug, Copy, Clone)]
#[repr(C, packed)]
struct ReceiveDescriptor {
    buffer_addr: u64, length: u16, checksum: u16, status: u8, error: u8, special: u16,
}

const RD_DD: u8 = 1 << 0;

#[derive(Debug, Copy, Clone)]
#[repr(C, packed)]
struct TransmitDescriptor {
    buffer_addr: u64, length: u16, cso: u8, command: u8, status: u8, css: u8, special: u16,
}

const TD_CMD_EOP: u8 = 1 << 0; 
const TD_CMD_IFCS: u8 = 1 << 1; 
const TD_CMD_RS: u8 = 1 << 3;  

pub struct E1000Driver {
    mmio_base: usize,
    pub mac_address: [u8; 6],
    
    // DMA Pointers
    tx_ring_virt: *mut TransmitDescriptor,
    tx_buffer_virt: *mut u8,
    tx_buffer_phys: u64,
    tx_index: usize,

    // --- NEW: RX Pointers ---
    rx_ring_virt: *mut ReceiveDescriptor,
    // We need an array of pointers because every incoming packet needs its own buffer!
    rx_buffers_virt:[*mut u8; NUM_RX_DESCRIPTORS], 
    rx_index: usize,
}

unsafe impl Send for E1000Driver {}
unsafe impl Sync for E1000Driver {}

/// [MICT: INTERNET CHECKSUM]
/// Calculates the standard network byte order checksum for IPv4 and ICMP headers.
fn calculate_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < data.len() {
        let word0 = data[i] as u32;
        let word1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
        // Combine into a 16-bit word (Big Endian)
        sum += (word0 << 8) | word1;
        i += 2;
    }
    // Fold 32-bit sum to 16 bits
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16) // One's complement
}

impl E1000Driver {
    pub unsafe fn new(
        mmio_base: usize,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
        phys_mem_offset: VirtAddr,
    ) -> Self {
        crate::serial_println!("[E1000] Initializing Gigabit Network Controller...");

        let (tx_ring_phys, tx_ring_virt) = crate::memory::allocate_dma_frames(frame_allocator, phys_mem_offset, 1).unwrap();
        let (tx_buf_phys, tx_buf_virt) = crate::memory::allocate_dma_frames(frame_allocator, phys_mem_offset, 1).unwrap();

        // --- NEW: ALLOCATE RX RING & BUFFERS ---
        let (rx_ring_phys, rx_ring_virt) = crate::memory::allocate_dma_frames(frame_allocator, phys_mem_offset, 1).unwrap();
        
        let mut driver = E1000Driver {
            mmio_base,
            mac_address: [0; 6],
            tx_ring_virt: tx_ring_virt.as_mut_ptr(),
            tx_buffer_virt: tx_buf_virt.as_mut_ptr(),
            tx_buffer_phys: tx_buf_phys.as_u64(),
            tx_index: 0,
            rx_ring_virt: rx_ring_virt.as_mut_ptr(),
            rx_buffers_virt:[core::ptr::null_mut(); NUM_RX_DESCRIPTORS],
            rx_index: 0,
        };

        driver.read_mac_address();
        
        // Initialize the rings
        driver.init_tx_ring(tx_ring_phys.as_u64());
        driver.init_rx_ring(rx_ring_phys.as_u64(), frame_allocator, phys_mem_offset); // <-- NEW CALL
        
        driver.enable_card();
        driver
    }

    unsafe fn read_reg(&self, register: u32) -> u32 {
        ptr::read_volatile((self.mmio_base + register as usize) as *const u32)
    }

    unsafe fn write_reg(&self, register: u32, data: u32) {
        ptr::write_volatile((self.mmio_base + register as usize) as *mut u32, data);
    }

    unsafe fn read_mac_address(&mut self) {
        let mac_low = self.read_reg(RAL0);
        let mac_high = self.read_reg(RAH0);
        
        self.mac_address =[
            mac_low as u8, (mac_low >> 8) as u8, (mac_low >> 16) as u8, (mac_low >> 24) as u8,
            mac_high as u8, (mac_high >> 8) as u8,
        ];
        crate::serial_println!("[E1000] MAC Address Extracted: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", 
            self.mac_address[0], self.mac_address[1], self.mac_address[2], 
            self.mac_address[3], self.mac_address[4], self.mac_address[5]);
    }

    unsafe fn init_tx_ring(&mut self, tx_ring_phys: u64) {
        crate::serial_println!("[E1000] Wiring TX DMA Ring at Phys: {:#X}", tx_ring_phys);

        // Tell the hardware where the ring is (High and Low 32 bits)
        self.write_reg(TDBAL, (tx_ring_phys & 0xFFFFFFFF) as u32);
        self.write_reg(TDBAH, (tx_ring_phys >> 32) as u32);

        // Tell the hardware how long the ring is in bytes
        let ring_len = (NUM_TX_DESCRIPTORS * core::mem::size_of::<TransmitDescriptor>()) as u32;
        self.write_reg(TDLEN, ring_len);

        // Set Head and Tail to 0 (Queue is empty)
        self.write_reg(TDH, 0);
        self.write_reg(TDT, 0);

        // Turn on Transmit Enable (TCTL_EN) and Pad Short Packets (TCTL_PSP)
        let tctl = self.read_reg(TCTL);
        self.write_reg(TCTL, tctl | TCTL_EN | TCTL_PSP);
    }

    unsafe fn init_rx_ring(
        &mut self, 
        rx_ring_phys: u64, 
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
        phys_mem_offset: VirtAddr,
    ) {
        crate::serial_println!("[E1000] Wiring RX DMA Ring at Phys: {:#X}", rx_ring_phys);

        // Tell hardware where the ring is
        self.write_reg(RDBAL, (rx_ring_phys & 0xFFFFFFFF) as u32);
        self.write_reg(RDBAH, (rx_ring_phys >> 32) as u32);
        self.write_reg(RDLEN, (NUM_RX_DESCRIPTORS * core::mem::size_of::<ReceiveDescriptor>()) as u32);

        self.write_reg(RDH, 0);
        self.write_reg(RDT, (NUM_RX_DESCRIPTORS - 1) as u32); // Tell card we have 31 empty buckets

        // Wire up the individual packet buffers
        for i in 0..NUM_RX_DESCRIPTORS {
            // Allocate 1 page (4KB) per receiving packet bucket
            let (buf_phys, buf_virt) = crate::memory::allocate_dma_frames(frame_allocator, phys_mem_offset, 1).unwrap();
            
            self.rx_buffers_virt[i] = buf_virt.as_mut_ptr();
            
            let desc_ptr = self.rx_ring_virt.add(i);
            (*desc_ptr).buffer_addr = buf_phys.as_u64();
            (*desc_ptr).status = 0; // Hardware will change this to RD_DD when it writes a packet!
        }

        // Enable Receiver, Broadcast Accept, Strip CRC
        let rctl = self.read_reg(RCTL);
        self.write_reg(RCTL, rctl | RCTL_EN | RCTL_BAM | RCTL_SECRC | RCTL_UPE | RCTL_MPE);
    }

    unsafe fn enable_card(&self) {
        // Clear the reset flags, link up!
        crate::serial_println!("[E1000] Controller Linked and Enabled.");
    }

    ///[MICT: RAW PACKET FORGE v2 (Mathematical Perfection)]
    pub unsafe fn send_ping(&mut self, dest_mac:[u8; 6], dest_ip: [u8; 4], source_ip:[u8; 4]) {
        let payload = b"PolymorphOS Bare-Metal Ping Test!";
        let packet_size = 42 + payload.len();
        let mut packet = alloc::vec![0u8; packet_size]; 

        // --- 1. ETHERNET FRAME (14 bytes) ---
        packet[0..6].copy_from_slice(&dest_mac);
        packet[6..12].copy_from_slice(&self.mac_address);
        packet[12] = 0x08; packet[13] = 0x00; // EtherType: IPv4

        // --- 2. IPv4 HEADER (20 bytes) ---
        packet[14] = 0x45; // Version 4, IHL 5
        packet[15] = 0x00; // DSCP
        
        let ip_len = (20 + 8 + payload.len()) as u16;
        packet[16] = (ip_len >> 8) as u8; packet[17] = (ip_len & 0xFF) as u8; // Total Length
        
        packet[18] = 0x12; packet[19] = 0x34; // Identification
        packet[20] = 0x40; packet[21] = 0x00; // Flags (Don't Fragment)
        packet[22] = 0x40; // TTL (64)
        packet[23] = 0x01; // Protocol: ICMP (1)
        
        packet[26..30].copy_from_slice(&source_ip);
        packet[30..34].copy_from_slice(&dest_ip);

        // Calculate IPv4 Checksum
        packet[24] = 0; packet[25] = 0; 
        let ip_csum = calculate_checksum(&packet[14..34]);
        packet[24] = (ip_csum >> 8) as u8; packet[25] = (ip_csum & 0xFF) as u8;

        // --- 3. ICMP ECHO REQUEST (8 bytes header + payload) ---
        packet[34] = 0x08; // Type: 8 (Echo Request)
        packet[35] = 0x00; // Code: 0
        
        packet[38] = 0x00; packet[39] = 0x01; // Identifier
        packet[40] = 0x00; packet[41] = 0x01; // Sequence Number

        // Insert Payload
        packet[42..42+payload.len()].copy_from_slice(payload);

        // Calculate ICMP Checksum
        packet[36] = 0; packet[37] = 0; 
        let icmp_csum = calculate_checksum(&packet[34..packet_size]);
        packet[36] = (icmp_csum >> 8) as u8; packet[37] = (icmp_csum & 0xFF) as u8;

        // Transmit!
        self.transmit_packet_secure(&packet);
    }

    ///[MICT: ARP REQUEST FORGE]
    /// Shouts into the network to find the MAC address of a target IP.
    pub unsafe fn send_arp_request(&mut self, target_ip:[u8; 4], source_ip: [u8; 4]) {
        let mut packet =[0u8; 42]; // Standard ARP Request size

        // --- 1. ETHERNET FRAME ---
        // Destination: Broadcast MAC (Shout to everyone)
        packet[0..6].copy_from_slice(&[0xFF; 6]);
        // Source: Our MAC
        packet[6..12].copy_from_slice(&self.mac_address);
        // EtherType: ARP (0x0806)
        packet[12] = 0x08; packet[13] = 0x06;

        // --- 2. ARP PAYLOAD ---
        // Hardware Type: Ethernet (1)
        packet[14] = 0x00; packet[15] = 0x01;
        // Protocol Type: IPv4 (0x0800)
        packet[16] = 0x08; packet[17] = 0x00;
        // Hardware Address Length: 6 (MAC)
        packet[18] = 0x06;
        // Protocol Address Length: 4 (IPv4)
        packet[19] = 0x04;
        // Operation: ARP Request (1)
        packet[20] = 0x00; packet[21] = 0x01;

        // Sender Hardware Address (Our MAC)
        packet[22..28].copy_from_slice(&self.mac_address);
        // Sender Protocol Address (Our IP)
        packet[28..32].copy_from_slice(&source_ip);

        // Target Hardware Address (Unknown, leave as 0)
        packet[32..38].copy_from_slice(&[0x00; 6]);
        // Target Protocol Address (The IP we are looking for)
        packet[38..42].copy_from_slice(&target_ip);

        crate::serial_println!("[E1000] Forging ARP Request for IP {}.{}.{}.{}...", target_ip[0], target_ip[1], target_ip[2], target_ip[3]);
        
        self.transmit_packet_secure(&packet);
    }

    /// [MICT: THE OMZTA TRANSMIT GATEWAY]
    pub unsafe fn transmit_packet_secure(&mut self, payload: &[u8]) {
        if payload.len() > 1518 {
            crate::serial_println!("[E1000 ERROR] Payload exceeds standard MTU (1518 bytes).");
            return;
        }

        crate::serial_println!("[E1000] Forging Network Packet ({} bytes)...", payload.len());

        // 1. Copy the raw ethernet frame into our physical DMA staging buffer
        core::ptr::copy_nonoverlapping(payload.as_ptr(), self.tx_buffer_virt, payload.len());

        // 2. Get the next available descriptor
        let desc_ptr = self.tx_ring_virt.add(self.tx_index);
        
        // 3. Fill out the descriptor instructions
        (*desc_ptr).buffer_addr = self.tx_buffer_phys; // Point to the staging buffer
        (*desc_ptr).length = payload.len() as u16;
        (*desc_ptr).command = TD_CMD_EOP | TD_CMD_IFCS | TD_CMD_RS; // End of Packet, Insert Checksum
        (*desc_ptr).status = 0;

        // Force CPU to flush cache to RAM before telling the NIC to read it!
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // 4. Move the index forward
        self.tx_index = (self.tx_index + 1) % NUM_TX_DESCRIPTORS;

        // 5. RING THE DOORBELL!
        // Writing the new index to the Transmit Tail (TDT) register wakes the silicon up!
        crate::serial_println!("[E1000] Ringing TX Doorbell!");
        self.write_reg(TDT, self.tx_index as u32);
    }

    ///[MICT: RECEIVE RADAR]
    pub unsafe fn poll_receive(&mut self) -> Option<&[u8]> {
        let desc_ptr = self.rx_ring_virt.add(self.rx_index);
        
        //[MICT: VOLATILE READ]
        // Force the CPU to check physical RAM, bypassing the L1 cache!
        let status = core::ptr::read_volatile(&raw const (*desc_ptr).status);

        // Did the hardware flip the "Descriptor Done" bit?
        if (status & RD_DD) != 0 {
            // Read length cleanly
            let packet_len = core::ptr::read_volatile(&raw const (*desc_ptr).length) as usize;
            
            crate::serial_println!("[E1000] 📬 INCOMING PACKET: {} bytes!", packet_len);

            // 1. Grab the packet data
            let buffer_ptr = self.rx_buffers_virt[self.rx_index];
            let packet_data = core::slice::from_raw_parts(buffer_ptr, packet_len);

            // 2. Clear the descriptor status using VOLATILE WRITE
            core::ptr::write_volatile(&raw mut (*desc_ptr).status, 0);

            // 3. Move our ring index forward
            let old_index = self.rx_index;
            self.rx_index = (self.rx_index + 1) % NUM_RX_DESCRIPTORS;

            // 4. Ring the Tail Doorbell to tell the card it can reuse the old bucket
            self.write_reg(RDT, old_index as u32);

            return Some(packet_data);
        }
        
        None // No packets waiting
    }
}