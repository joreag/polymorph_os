// kernel/src/virtqueue.rs
// The DMA Ring Buffer Substrate for VirtIO and AMD-style Command Rings

use core::sync::atomic::{AtomicU16, Ordering};

// --- 1. THE DESCRIPTOR TABLE ---
// This holds the actual pointers to our Command Packets in RAM.
pub const VIRTQ_DESC_F_NEXT: u16 = 1; // This descriptor continues to another one
pub const VIRTQ_DESC_F_WRITE: u16 = 2; // The device (GPU) is allowed to write to this memory

#[derive(Debug, Clone, Copy)]
#[repr(C, align(16))]
pub struct VirtqDesc {
    pub addr: u64,  // The physical memory address of our Command Packet
    pub len: u32,   // The length of the packet in bytes
    pub flags: u16, // Read/Write/Next flags
    pub next: u16,  // The index of the next descriptor in the chain
}

// --- 2. THE AVAILABLE RING ---
// The CPU writes to this ring to tell the GPU: "I put a new command at index X."
#[derive(Debug)]
#[repr(C, align(2))]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: AtomicU16,      // Where the CPU will write next
    pub ring:[u16; 64],    // The array of descriptor indices (Assume a queue size of 256 for now)
    pub used_event: u16,     // Only used if VIRTIO_F_EVENT_IDX is negotiated
}

// --- 3. THE USED RING ---
// The GPU writes to this ring to tell the CPU: "I finished the command at index X."
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VirtqUsedElem {
    pub id: u32,  // The index of the descriptor that finished
    pub len: u32, // The number of bytes the GPU wrote back (if any)
}

#[derive(Debug)]
#[repr(C, align(4))]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: AtomicU16,            // Where the GPU wrote last
    pub ring: [VirtqUsedElem; 64], // The array of finished commands
    pub avail_event: u16,
}

// --- 4. THE MASTER QUEUE STRUCT ---
// We tie it all together. This entire struct will be allocated in physical RAM.
#[repr(C, align(4096))] // Must be page-aligned!
pub struct VirtQueue {
    pub descriptors: [VirtqDesc; 64],
    pub available: VirtqAvail,
    // (In reality, there's padding here to reach the next page boundary, 
    // but we will calculate that dynamically during Phase 2 setup)
    pub used: VirtqUsed,
    
    // Internal driver tracking
    pub free_head: u16,
    pub num_free: u16,
    pub last_used_idx: u16,
}

impl VirtQueue {
    /// Setup the internal tracking for an empty queue
    pub fn new() -> Self {
        let mut vq = VirtQueue {
            descriptors:[VirtqDesc { addr: 0, len: 0, flags: 0, next: 0 }; 64],
            available: VirtqAvail { flags: 0, idx: AtomicU16::new(0), ring: [0; 64], used_event: 0 },
            used: VirtqUsed { flags: 0, idx: AtomicU16::new(0), ring:[VirtqUsedElem { id: 0, len: 0 }; 64], avail_event: 0 },
            free_head: 0,
            num_free: 64,
            last_used_idx: 0,
        };

        // Chain the free descriptors together (0 to 62 points to next)
        for i in 0..63 {
            vq.descriptors[i as usize].next = i + 1;
        }
        vq
    }
}