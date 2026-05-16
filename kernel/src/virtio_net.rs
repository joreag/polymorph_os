// kernel/src/virtio_net.rs

use spin::Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

pub static VIRTIO_NET: Mutex<Option<VirtioNetDriver>> = Mutex::new(None);

/// [MICT: THE REQUIRED VIRTIO HEADER]
/// Every packet sent to or received from VirtIO-Net MUST start with this header!
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct VirtioNetHeader {
    pub flags: u8,
    pub gso_type: u8,
    pub hdr_len: u16,
    pub gso_size: u16,
    pub csum_start: u16,
    pub csum_offset: u16,
    pub num_buffers: u16, // Only used if VIRTIO_NET_F_MRG_RXBUF is negotiated
}

pub struct VirtioNetDriver {
    pub mmio_base: usize,
    pub mac_address: [u8; 6],
    
    pub rx_queue_virt: Option<u64>,
    pub rx_notify_addr: Option<u64>,
    pub tx_queue_virt: Option<u64>,
    pub tx_notify_addr: Option<u64>,
    
    // --- NEW: Track where we stopped reading ---
    rx_last_used: u16, 
}

impl VirtioNetDriver {
    pub fn new(mmio_base: usize) -> Self {
        VirtioNetDriver {
            mmio_base,
            mac_address: [0; 6], 
            rx_queue_virt: None, rx_notify_addr: None,
            tx_queue_virt: None, tx_notify_addr: None,
            rx_last_used: 0, // Start at 0
        }
    }

    // Helper for IP/ICMP Checksums
    fn calculate_checksum(data: &[u8]) -> u16 {
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < data.len() {
            let word0 = data[i] as u32;
            let word1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
            sum += (word0 << 8) | word1;
            i += 2;
        }
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        !(sum as u16)
    }

    /// [MICT: VIRTIO-NET DMA TRANSMITTER]
    /// Wraps a raw Ethernet frame in a VirtioNetHeader and drops it into the TX Virtqueue (Queue 1).
    pub unsafe fn transmit_packet(&mut self, payload: &[u8]) {
        let vq_virt = self.tx_queue_virt.expect("[FATAL] TX Queue not initialized!");
        let vq = &mut *(vq_virt as *mut crate::virtqueue::VirtQueue);

        if vq.num_free == 0 {
            crate::serial_println!("[VIRTIO-NET] TX Queue Full! Dropping packet.");
            return;
        }

        crate::serial_println!("[VIRTIO-NET] Forging Transmit Descriptor...");

        // 1. We need physical memory for the header + payload.
        // For testing, let's pretend we have a dedicated transmit buffer at a known physical address.
        // (In a real OS, you'd use your frame allocator here).
        let tx_buffer_phys: u64 = 0x3000000; // HARDCODED STUB FOR NOW
        let tx_buffer_virt: u64 = tx_buffer_phys + 0xFFFF_8000_0000_0000; // Assuming standard offset

        // 2. Write the mandatory VirtIO Header
        let header_ptr = tx_buffer_virt as *mut VirtioNetHeader;
        core::ptr::write_bytes(header_ptr as *mut u8, 0, core::mem::size_of::<VirtioNetHeader>());
        // (Default header of all 0s is perfectly fine for basic transmission)

        // 3. Write the actual Ethernet payload right after the header
        let payload_ptr = (tx_buffer_virt + core::mem::size_of::<VirtioNetHeader>() as u64) as *mut u8;
        core::ptr::copy_nonoverlapping(payload.as_ptr(), payload_ptr, payload.len());

        let total_len = core::mem::size_of::<VirtioNetHeader>() + payload.len();

        // 4. Put it in the ring buffer!
        let head_idx = vq.free_head as usize;
        vq.free_head = vq.descriptors[head_idx].next;
        vq.num_free -= 1;

        vq.descriptors[head_idx].addr = tx_buffer_phys;
        vq.descriptors[head_idx].len = total_len as u32;
        vq.descriptors[head_idx].flags = 0; // Device only reads this

        let avail_idx = vq.available.idx.load(core::sync::atomic::Ordering::SeqCst) as usize;
        vq.available.ring[avail_idx % 64] = head_idx as u16;

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        vq.available.idx.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

        // 5. Ring the TX Doorbell
        if let Some(n) = self.tx_notify_addr { 
            crate::serial_println!("[VIRTIO-NET] Ringing TX Doorbell!");
            core::ptr::write_volatile(n as *mut u16, 1); // Queue index 1 for TX
        }
        
        // Blind reclaim for testing
        vq.descriptors[head_idx].next = vq.free_head;
        vq.free_head = head_idx as u16;
        vq.num_free += 1;
    }

    /// [MICT: VIRTIO-NET RECEIVER]
    /// Polls the RX Virtqueue. If a packet arrived, strips the header and returns the raw Ethernet frame.
    pub unsafe fn poll_receive(&mut self) -> Option<&[u8]> {
        let vq_virt = self.rx_queue_virt?;
        let vq = &mut *(vq_virt as *mut crate::virtqueue::VirtQueue);

        let current_used = vq.used.idx.load(core::sync::atomic::Ordering::Acquire);
        
        // If the GPU's index is different from our index, a packet arrived!
        if self.rx_last_used != current_used {
            let used_elem = vq.used.ring[(self.rx_last_used % 64) as usize];
            let desc_idx = used_elem.id as usize;
            let total_len = used_elem.len as usize;

            self.rx_last_used = self.rx_last_used.wrapping_add(1);

            // Get the physical address of the buffer that holds the packet
            let buf_phys = vq.descriptors[desc_idx].addr;
            // (Assuming standard 0xFFFF_8000_0000_0000 offset for virtual mapping)
            let buf_virt = buf_phys + 0xFFFF_8000_0000_0000; 

            // VirtIO-Net packets ALWAYS start with a 12-byte VirtioNetHeader.
            // We skip the header to get the pure Ethernet frame.
            let header_size = core::mem::size_of::<VirtioNetHeader>();
            let packet_len = total_len.saturating_sub(header_size);
            
            let packet_data = core::slice::from_raw_parts((buf_virt + header_size as u64) as *const u8, packet_len);

            // --- RECLAIM THE DESCRIPTOR ---
            // Give the empty bucket back to the hypervisor so it can catch the next packet!
            let avail_idx = vq.available.idx.load(core::sync::atomic::Ordering::SeqCst);
            vq.available.ring[(avail_idx % 64) as usize] = desc_idx as u16;
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            vq.available.idx.store(avail_idx.wrapping_add(1), core::sync::atomic::Ordering::SeqCst);

            // Ring the RX Doorbell (Queue 0) to tell the hypervisor a bucket is empty
            if let Some(n) = self.rx_notify_addr { 
                core::ptr::write_volatile(n as *mut u16, 0); 
            }

            return Some(packet_data);
        }
        
        None // No mail today
    }

    /// [MICT: RAW PACKET FORGE v3]
    pub unsafe fn send_ping(&mut self, dest_mac:[u8; 6], dest_ip: [u8; 4], source_ip:[u8; 4]) {
        let payload = b"PolymorphOS VirtIO Ping!";
        let packet_size = 42 + payload.len();
        let mut packet = alloc::vec![0u8; packet_size]; 

        // 1. ETHERNET FRAME
        packet[0..6].copy_from_slice(&dest_mac);
        packet[6..12].copy_from_slice(&self.mac_address);
        packet[12] = 0x08; packet[13] = 0x00; // EtherType: IPv4

        // 2. IPv4 HEADER
        packet[14] = 0x45; packet[15] = 0x00; 
        let ip_len = (20 + 8 + payload.len()) as u16;
        packet[16] = (ip_len >> 8) as u8; packet[17] = (ip_len & 0xFF) as u8; 
        packet[18] = 0x12; packet[19] = 0x34; 
        packet[20] = 0x40; packet[21] = 0x00; 
        packet[22] = 0x40; // TTL
        packet[23] = 0x01; // ICMP
        packet[26..30].copy_from_slice(&source_ip);
        packet[30..34].copy_from_slice(&dest_ip);

        packet[24] = 0; packet[25] = 0; 
        let ip_csum = Self::calculate_checksum(&packet[14..34]);
        packet[24] = (ip_csum >> 8) as u8; packet[25] = (ip_csum & 0xFF) as u8;

        // 3. ICMP HEADER
        packet[34] = 0x08; packet[35] = 0x00; 
        packet[38] = 0x00; packet[39] = 0x01; 
        packet[40] = 0x00; packet[41] = 0x01; 
        packet[42..42+payload.len()].copy_from_slice(payload);

        packet[36] = 0; packet[37] = 0; 
        let icmp_csum = Self::calculate_checksum(&packet[34..packet_size]);
        packet[36] = (icmp_csum >> 8) as u8; packet[37] = (icmp_csum & 0xFF) as u8;

        self.transmit_packet(&packet);
    }

    pub unsafe fn send_arp_request(&mut self, target_ip:[u8; 4], source_ip: [u8; 4]) {
        let mut packet =[0u8; 42];
        packet[0..6].copy_from_slice(&[0xFF; 6]);
        packet[6..12].copy_from_slice(&self.mac_address);
        packet[12] = 0x08; packet[13] = 0x06;
        packet[14] = 0x00; packet[15] = 0x01;
        packet[16] = 0x08; packet[17] = 0x00;
        packet[18] = 0x06; packet[19] = 0x04;
        packet[20] = 0x00; packet[21] = 0x01;
        packet[22..28].copy_from_slice(&self.mac_address);
        packet[28..32].copy_from_slice(&source_ip);
        packet[32..38].copy_from_slice(&[0x00; 6]);
        packet[38..42].copy_from_slice(&target_ip);

        crate::serial_println!("[VIRTIO-NET] Forging ARP Request...");
        self.transmit_packet(&packet);
    }
}